use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use getrandom::getrandom;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::{Mutex, Notify},
    time::timeout,
};
use url::Url;
use warp_ztna_err::prelude::*;

const REDIRECT_PORT: u16 = 37_123;
const CALLBACK_TIMEOUT: Duration = Duration::from_mins(5);
const CALLBACK_READ_TIMEOUT: Duration = Duration::from_secs(5);

type BrowserOpener = Arc<dyn Fn(&str) -> std::io::Result<()> + Send + Sync>;
const ISSUER_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const TOKEN_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_OIDC_ISSUER: &str = "https://ztna-kc.corp.dy-sec.com/realms/warp-ztna";

#[derive(Debug, Clone)]
pub struct OidcConfig {
    pub issuer: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub scope: String,
}

impl OidcConfig {
    pub fn from_env() -> Self {
        Self {
            issuer: DEFAULT_OIDC_ISSUER.to_owned(),
            client_id: std::env::var("WARP_OIDC_CLIENT_ID")
                .unwrap_or_else(|_| "warp-ztna-app".to_owned()),
            redirect_uri: "http://127.0.0.1:37123/oauth/callback".to_owned(),
            scope: "openid profile email offline_access warp-ztna-api".to_owned(),
        }
    }

    fn authorize_url(&self, verifier: &str, state: &str, prompt: Option<&str>) -> Result<String> {
        let mut url = Url::parse(&format!(
            "{}/protocol/openid-connect/auth",
            self.issuer.trim_end_matches('/')
        ))
        .or_err(Reason::InvalidArgument, "invalid OIDC issuer")?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("client_id", &self.client_id);
            query.append_pair("redirect_uri", &self.redirect_uri);
            query.append_pair("response_type", "code");
            query.append_pair("scope", &self.scope);
            query.append_pair("state", state);
            query.append_pair("code_challenge", &code_challenge(verifier));
            query.append_pair("code_challenge_method", "S256");
            if let Some(prompt) = prompt {
                query.append_pair("prompt", prompt);
            }
        }
        Ok(url.to_string())
    }

    fn discovery_url(&self) -> String {
        format!("{}/.well-known/openid-configuration", self.issuer.trim_end_matches('/'))
    }

    fn token_url(&self) -> String {
        format!("{}/protocol/openid-connect/token", self.issuer.trim_end_matches('/'))
    }

    fn revocation_url(&self) -> String {
        format!("{}/protocol/openid-connect/revoke", self.issuer.trim_end_matches('/'))
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenErrorBody {
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Clone)]
struct CachedToken {
    access_token: String,
    expires_at: Instant,
    refresh_token: Option<String>,
}

#[derive(Debug)]
enum AuthCallback {
    Code(String),
    Error(String),
}

struct CallbackOutcome {
    callback: AuthCallback,
    verifier: String,
}

#[derive(Debug)]
struct PendingFlow {
    verifier: String,
    state: String,
    url: String,
}

enum CallbackParse {
    Ignore,
    SilentCode(String),
    InteractiveCode(String),
    Error(String),
}

#[derive(Debug, PartialEq, Eq)]
enum CallbackDirective {
    NotFound,
    BadRequest(&'static str),
    SilentCode(String),
    SilentInteractionRequired,
    SilentError(String),
    InteractiveCode(String),
    InteractiveError(String),
    Ignore,
}

enum CallbackWait {
    Outcome(CallbackOutcome),
    Cancelled,
    TimedOut,
}

#[derive(Debug)]
pub(crate) enum ApiSessionError {
    Cancelled,
    TimedOut,
    Busy,
    BrowserFailed(Error),
    Failed(Box<Error>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TokenAttempt {
    Token(String),
    Absent,
    Busy,
}

#[derive(Default)]
struct CancelSignal {
    epoch: AtomicU64,
    notify: Notify,
}

impl CancelSignal {
    fn attempt(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }

    fn cancel(&self) {
        self.epoch.fetch_add(1, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    fn is_cancelled(&self, attempt: u64) -> bool {
        self.epoch.load(Ordering::SeqCst) != attempt
    }

    async fn cancelled(&self, attempt: u64) {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_cancelled(attempt) {
                return;
            }
            notified.await;
        }
    }
}

/// OIDC client for the API resource-server audience.
///
/// Access and refresh tokens are kept in memory only. [`ApiOidc::acquire_interactive`]
/// is the only entry point that may open a browser flow; passive callers use
/// [`ApiOidc::try_access_token`] and never start one.
#[derive(Clone)]
pub struct ApiOidc {
    config: Arc<OidcConfig>,
    cache: Arc<Mutex<Option<CachedToken>>>,
    acquire_lock: Arc<Mutex<()>>,
    cancel: Arc<CancelSignal>,
    opener: BrowserOpener,
    http: reqwest::Client,
}

impl ApiOidc {
    pub fn new(config: OidcConfig) -> Self {
        Self {
            config: Arc::new(config),
            cache: Arc::new(Mutex::new(None)),
            acquire_lock: Arc::new(Mutex::new(())),
            cancel: Arc::new(CancelSignal::default()),
            opener: Arc::new(|url: &str| open::that_detached(url)),
            http: reqwest::Client::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_opener(mut self, opener: BrowserOpener) -> Self {
        self.opener = opener;
        self
    }

    pub fn is_configured(&self) -> bool {
        !self.config.issuer.trim().is_empty()
    }

    pub fn issuer(&self) -> &str {
        &self.config.issuer
    }

    pub fn with_issuer(&self, issuer: &str) -> Self {
        let mut config = (*self.config).clone();
        config.issuer = normalize_issuer(issuer);
        Self {
            config: Arc::new(config),
            cache: Arc::new(Mutex::new(None)),
            acquire_lock: Arc::new(Mutex::new(())),
            cancel: Arc::clone(&self.cancel),
            opener: Arc::clone(&self.opener),
            http: self.http.clone(),
        }
    }

    pub(crate) async fn acquire_interactive(&self) -> std::result::Result<String, ApiSessionError> {
        if !self.is_configured() {
            return Err(ApiSessionError::Failed(Box::new(explain(
                Reason::InvalidArgument,
                "OIDC issuer is not configured",
            ))));
        }

        log::info!("api_oidc: acquiring API access token");
        let attempt = self.cancel.attempt();
        if let Some(token) = self.cached_access_token().await {
            log::debug!("api_oidc: using cached access token");
            return Ok(token);
        }
        let Ok(_guard) = self.acquire_lock.try_lock() else {
            return Err(ApiSessionError::Busy);
        };
        if let Some(token) = self.cached_access_token().await {
            log::debug!("api_oidc: using cached access token");
            return Ok(token);
        }
        if let Some(refresh_token) = self.cached_refresh_token().await {
            log::info!("api_oidc: refresh token found in memory, exchanging it");
            if let Ok(tokens) = self.exchange_refresh(&refresh_token).await {
                let access = tokens.access_token.clone();
                self.cache_tokens(tokens).await;
                return Ok(access);
            }
            log::warn!("api_oidc: refresh token exchange failed, discarding in-memory session");
            *self.cache.lock().await = None;
        }

        let tokens = self.acquire(attempt).await?;
        let access = tokens.access_token.clone();
        self.cache_tokens(tokens).await;
        Ok(access)
    }

    pub(crate) async fn try_access_token(&self) -> TokenAttempt {
        if !self.is_configured() {
            return TokenAttempt::Absent;
        }
        if let Some(token) = self.cached_access_token().await {
            return TokenAttempt::Token(token);
        }
        let Ok(_guard) = self.acquire_lock.try_lock() else {
            return TokenAttempt::Busy;
        };
        if let Some(token) = self.cached_access_token().await {
            return TokenAttempt::Token(token);
        }
        if let Some(refresh_token) = self.cached_refresh_token().await {
            if let Ok(tokens) = self.exchange_refresh(&refresh_token).await {
                let access = tokens.access_token.clone();
                self.cache_tokens(tokens).await;
                return TokenAttempt::Token(access);
            }
            *self.cache.lock().await = None;
        }
        TokenAttempt::Absent
    }

    pub(crate) fn cancel(&self) {
        self.cancel.cancel();
    }

    pub async fn clear(&self) -> Result<()> {
        self.cancel.cancel();
        let _guard = self.acquire_lock.lock().await;
        if let Some(refresh_token) = self.cached_refresh_token().await {
            log::info!("api_oidc: revoking in-memory refresh token");
            self.revoke_refresh_token(&refresh_token).await;
        }
        *self.cache.lock().await = None;
        Ok(())
    }

    async fn cached_access_token(&self) -> Option<String> {
        let cache = self.cache.lock().await;
        cache
            .as_ref()
            .filter(|token| Instant::now() < token.expires_at)
            .map(|token| token.access_token.clone())
    }

    async fn cached_refresh_token(&self) -> Option<String> {
        self.cache.lock().await.as_ref().and_then(|token| token.refresh_token.clone())
    }

    async fn cache_tokens(&self, tokens: TokenResponse) {
        let access_token = tokens.access_token;
        let refresh_token = tokens.refresh_token;
        let lifetime = tokens.expires_in.unwrap_or(300).saturating_sub(60).max(5);
        let expires_at = Instant::now() + Duration::from_secs(lifetime);
        *self.cache.lock().await = Some(CachedToken { access_token, expires_at, refresh_token });
    }

    async fn ensure_issuer_reachable(&self) -> Result<()> {
        log::debug!("api_oidc: probing issuer {}", self.config.issuer);
        self.http
            .get(self.config.discovery_url())
            .timeout(ISSUER_PROBE_TIMEOUT)
            .send()
            .await
            .map(|_| ())
            .or_err(Reason::Unavailable, "reach the OIDC issuer")
    }

    async fn acquire(&self, attempt: u64) -> std::result::Result<TokenResponse, ApiSessionError> {
        self.acquire_within(attempt, CALLBACK_TIMEOUT, CALLBACK_READ_TIMEOUT).await
    }

    async fn acquire_within(
        &self,
        attempt: u64,
        callback_timeout: Duration,
        read_timeout: Duration,
    ) -> std::result::Result<TokenResponse, ApiSessionError> {
        match tokio::select! {
            biased;
            () = self.cancel.cancelled(attempt) => return Err(ApiSessionError::Cancelled),
            result = self.ensure_issuer_reachable() => result,
        } {
            Ok(()) => {}
            Err(error) => return Err(ApiSessionError::Failed(Box::new(error))),
        }
        if self.cancel.is_cancelled(attempt) {
            return Err(ApiSessionError::Cancelled);
        }
        let address =
            self.redirect_addr().map_err(|error| ApiSessionError::Failed(Box::new(error)))?;
        let listener = loopback_listener(address)
            .await
            .map_err(|error| ApiSessionError::Failed(Box::new(error)))?;
        let silent = self
            .pending_flow(Some("none"))
            .map_err(|error| ApiSessionError::Failed(Box::new(error)))?;
        let interactive =
            self.pending_flow(None).map_err(|error| ApiSessionError::Failed(Box::new(error)))?;
        if self.cancel.is_cancelled(attempt) {
            return Err(ApiSessionError::Cancelled);
        }

        log::info!("api_oidc: starting silent authorization against issuer={}", self.config.issuer);
        if let Err(source) = (self.opener)(&silent.url) {
            return Err(ApiSessionError::BrowserFailed(because(
                Reason::External,
                "open OIDC authorization",
                source,
            )));
        }

        let wait = wait_for_callback(
            listener,
            &silent,
            &interactive,
            &self.cancel,
            attempt,
            callback_timeout,
            read_timeout,
        )
        .await?;
        let outcome = match wait {
            CallbackWait::Outcome(outcome) => outcome,
            CallbackWait::Cancelled => return Err(ApiSessionError::Cancelled),
            CallbackWait::TimedOut => return Err(ApiSessionError::TimedOut),
        };
        match outcome.callback {
            AuthCallback::Code(code) => self
                .exchange_code(&code, &outcome.verifier)
                .await
                .map_err(|error| ApiSessionError::Failed(Box::new(error))),
            AuthCallback::Error(error) => Err(ApiSessionError::Failed(Box::new(explain(
                Reason::Unauthorized,
                format!("OIDC authorization failed: {error}"),
            )))),
        }
    }

    fn redirect_addr(&self) -> Result<SocketAddr> {
        let url = Url::parse(&self.config.redirect_uri)
            .or_err(Reason::InvalidArgument, "invalid OIDC redirect URI")?;
        let host = url.host_str().unwrap_or("127.0.0.1");
        let ip = host
            .parse::<std::net::IpAddr>()
            .or_err(Reason::InvalidArgument, "invalid OIDC redirect host")?;
        if !ip.is_loopback() {
            return Err(explain(Reason::InvalidArgument, "OIDC redirect URI must be loopback"));
        }
        let port = url.port_or_known_default().unwrap_or(REDIRECT_PORT);
        Ok(SocketAddr::new(ip, port))
    }

    fn pending_flow(&self, prompt: Option<&str>) -> Result<PendingFlow> {
        let verifier = random_urlsafe(64)?;
        let state = random_urlsafe(32)?;
        let url = self.config.authorize_url(&verifier, &state, prompt)?;
        Ok(PendingFlow { verifier, state, url })
    }

    async fn exchange_code(&self, code: &str, verifier: &str) -> Result<TokenResponse> {
        self.token_request(&[
            ("grant_type", "authorization_code"),
            ("client_id", self.config.client_id.as_str()),
            ("redirect_uri", self.config.redirect_uri.as_str()),
            ("code", code),
            ("code_verifier", verifier),
        ])
        .await
    }

    async fn exchange_refresh(&self, refresh_token: &str) -> Result<TokenResponse> {
        self.token_request(&[
            ("grant_type", "refresh_token"),
            ("client_id", self.config.client_id.as_str()),
            ("refresh_token", refresh_token),
        ])
        .await
    }

    async fn revoke_refresh_token(&self, refresh_token: &str) {
        let result = self
            .http
            .post(self.config.revocation_url())
            .form(&[
                ("client_id", self.config.client_id.as_str()),
                ("token", refresh_token),
                ("token_type_hint", "refresh_token"),
            ])
            .timeout(TOKEN_TIMEOUT)
            .send()
            .await;

        match result {
            Ok(response) if response.status().is_success() => {
                log::info!("api_oidc: refresh token revoked");
            }
            Ok(response) => {
                log::warn!("api_oidc: token revocation returned HTTP {}", response.status());
            }
            Err(error) => {
                log::warn!("api_oidc: token revocation request failed: {error}");
            }
        }
    }

    async fn token_request(&self, form: &[(&str, &str)]) -> Result<TokenResponse> {
        let response = self
            .http
            .post(self.config.token_url())
            .form(form)
            .timeout(TOKEN_TIMEOUT)
            .send()
            .await
            .or_err(Reason::External, "OIDC token request")?;

        if response.status().is_success() {
            return response
                .json::<TokenResponse>()
                .await
                .or_err(Reason::External, "decode OIDC token response");
        }

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let detail = serde_json::from_str::<TokenErrorBody>(&body)
            .ok()
            .and_then(|body| body.error_description.or(body.error))
            .unwrap_or_else(|| body.chars().take(200).collect());
        log::warn!("api_oidc: token endpoint returned {status}: {detail}");
        Err(explain(
            Reason::Unauthorized,
            format!("OIDC token endpoint returned {status}: {detail}"),
        ))
    }
}

async fn loopback_listener(address: SocketAddr) -> Result<TcpListener> {
    TcpListener::bind(address).await.or_err(Reason::Unavailable, "bind loopback OIDC callback")
}

async fn wait_for_callback(
    listener: TcpListener,
    silent: &PendingFlow,
    interactive: &PendingFlow,
    cancel: &CancelSignal,
    attempt: u64,
    budget: Duration,
    read_budget: Duration,
) -> std::result::Result<CallbackWait, ApiSessionError> {
    let accept = async {
        loop {
            let (stream, _) = listener.accept().await.map_err(|error| {
                ApiSessionError::Failed(Box::new(because(
                    Reason::External,
                    "accept OIDC callback",
                    error,
                )))
            })?;
            match handle_callback(stream, silent, interactive, read_budget).await {
                Ok(CallbackParse::Ignore) => {}
                Ok(CallbackParse::SilentCode(code)) => {
                    return Ok(CallbackWait::Outcome(CallbackOutcome {
                        callback: AuthCallback::Code(code),
                        verifier: silent.verifier.clone(),
                    }));
                }
                Ok(CallbackParse::InteractiveCode(code)) => {
                    return Ok(CallbackWait::Outcome(CallbackOutcome {
                        callback: AuthCallback::Code(code),
                        verifier: interactive.verifier.clone(),
                    }));
                }
                Ok(CallbackParse::Error(error)) => {
                    return Ok(CallbackWait::Outcome(CallbackOutcome {
                        callback: AuthCallback::Error(error),
                        verifier: String::new(),
                    }));
                }
                Err(error) => {
                    log::debug!("api_oidc: callback connection failed: {error}");
                }
            }
        }
    };

    tokio::select! {
        biased;
        () = cancel.cancelled(attempt) => Ok(CallbackWait::Cancelled),
        result = accept => result,
        () = tokio::time::sleep(budget) => Ok(CallbackWait::TimedOut),
    }
}

fn classify_callback(
    request_line: &str,
    silent: &PendingFlow,
    interactive: &PendingFlow,
) -> CallbackDirective {
    let Some(params) = callback_params(request_line) else {
        return CallbackDirective::NotFound;
    };
    let Some(state) = params.state else {
        return CallbackDirective::BadRequest("missing state");
    };

    if state == silent.state {
        if let Some(code) = params.code {
            return CallbackDirective::SilentCode(code);
        }
        if let Some(error) = params.error {
            return if is_interaction_required(&error) {
                CallbackDirective::SilentInteractionRequired
            } else {
                CallbackDirective::SilentError(error)
            };
        }
        return CallbackDirective::Ignore;
    }

    if state == interactive.state {
        if let Some(code) = params.code {
            return CallbackDirective::InteractiveCode(code);
        }
        if let Some(error) = params.error {
            return CallbackDirective::InteractiveError(error);
        }
        return CallbackDirective::Ignore;
    }

    CallbackDirective::BadRequest("state mismatch")
}

async fn handle_callback(
    stream: TcpStream,
    silent: &PendingFlow,
    interactive: &PendingFlow,
    read_budget: Duration,
) -> Result<CallbackParse> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    match timeout(read_budget, reader.read_line(&mut request_line)).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            log::debug!("api_oidc: reading the callback failed: {error}");
            return Ok(CallbackParse::Ignore);
        }
        Err(_) => {
            log::debug!("api_oidc: reading the callback timed out");
            return Ok(CallbackParse::Ignore);
        }
    }
    let mut stream = reader.into_inner();

    let (parse, response) = match classify_callback(&request_line, silent, interactive) {
        CallbackDirective::NotFound => (CallbackParse::Ignore, write_not_found(&mut stream).await),
        CallbackDirective::BadRequest(reason) => {
            if reason == "state mismatch" {
                log::warn!("api_oidc: callback state did not match either pending flow");
            }
            (CallbackParse::Ignore, write_error_page(&mut stream, reason).await)
        }
        CallbackDirective::SilentCode(code) => {
            log::debug!("api_oidc: silent authorization code received");
            (CallbackParse::SilentCode(code), write_self_closing_page(&mut stream).await)
        }
        CallbackDirective::SilentInteractionRequired => {
            log::info!(
                "api_oidc: silent login unavailable, redirecting to interactive authorization"
            );
            (CallbackParse::Ignore, write_redirect(&mut stream, &interactive.url).await)
        }
        CallbackDirective::SilentError(error) | CallbackDirective::InteractiveError(error) => {
            (CallbackParse::Error(error.clone()), write_error_page(&mut stream, &error).await)
        }
        CallbackDirective::InteractiveCode(code) => {
            log::debug!("api_oidc: interactive authorization code received");
            (CallbackParse::InteractiveCode(code), write_self_closing_page(&mut stream).await)
        }
        CallbackDirective::Ignore => {
            (CallbackParse::Ignore, write_self_closing_page(&mut stream).await)
        }
    };
    if let Err(error) = response {
        log::debug!("api_oidc: writing the callback response failed: {error}");
    }
    Ok(parse)
}

async fn write_self_closing_page(stream: &mut TcpStream) -> Result<()> {
    let body = "<!doctype html><html><head><meta charset=\"utf-8\"><script>window.open('', '_self').close();window.close();</script></head><body></body></html>";
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await.or_err(Reason::External, "write OIDC callback")?;
    stream.write_all(body.as_bytes()).await.or_err(Reason::External, "write OIDC callback")?;
    stream.shutdown().await.or_err(Reason::External, "close OIDC callback")
}

async fn write_redirect(stream: &mut TcpStream, location: &str) -> Result<()> {
    let header = format!(
        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nCache-Control: no-store\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(header.as_bytes()).await.or_err(Reason::External, "write OIDC callback")?;
    stream.shutdown().await.or_err(Reason::External, "close OIDC callback")
}

async fn write_error_page(stream: &mut TcpStream, error: &str) -> Result<()> {
    let body = format!("Authorization failed: {error}");
    let header = format!(
        "HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await.or_err(Reason::External, "write OIDC callback")?;
    stream.write_all(body.as_bytes()).await.or_err(Reason::External, "write OIDC callback")?;
    stream.shutdown().await.or_err(Reason::External, "close OIDC callback")
}

async fn write_not_found(stream: &mut TcpStream) -> Result<()> {
    stream
        .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .await
        .or_err(Reason::External, "write OIDC callback")?;
    stream.shutdown().await.or_err(Reason::External, "close OIDC callback")
}

fn is_interaction_required(error: &str) -> bool {
    matches!(error, "login_required" | "interaction_required")
}

struct CallbackParams {
    code: Option<String>,
    error: Option<String>,
    state: Option<String>,
}

fn callback_params(request_line: &str) -> Option<CallbackParams> {
    let target = request_line.split_whitespace().nth(1)?;
    let url = Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    if url.path() != "/oauth/callback" {
        return None;
    }

    let mut code = None;
    let mut error = None;
    let mut state = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            _ => {}
        }
    }
    Some(CallbackParams { code, error, state })
}

fn normalize_issuer(value: &str) -> String {
    value.trim().trim_end_matches('/').to_owned()
}

fn random_urlsafe(len: usize) -> Result<String> {
    let mut bytes = vec![0_u8; len];
    getrandom(&mut bytes).or_err(Reason::Internal, "generate random bytes")?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn code_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

pub(crate) fn email_claim(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let email = claims.get("email")?.as_str()?.trim();
    if email.is_empty() { None } else { Some(email.to_owned()) }
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncReadExt;

    use super::*;

    fn signed_token(payload: &str) -> String {
        format!("h.{}.s", URL_SAFE_NO_PAD.encode(payload))
    }

    fn test_config() -> OidcConfig {
        OidcConfig {
            issuer: "https://ztna-kc.corp.dy-sec.com/realms/warp-ztna".to_owned(),
            client_id: "warp-ztna-app".to_owned(),
            redirect_uri: "http://127.0.0.1:37123/oauth/callback".to_owned(),
            scope: "openid profile email offline_access warp-ztna-api".to_owned(),
        }
    }

    #[test]
    fn default_issuer_is_the_production_realm() {
        assert_eq!(OidcConfig::from_env().issuer, DEFAULT_OIDC_ISSUER);
    }

    #[tokio::test]
    async fn with_issuer_preserves_configuration_and_resets_the_token_cache() {
        let client = ApiOidc::new(test_config());
        client
            .cache_tokens(TokenResponse {
                access_token: "access".to_owned(),
                expires_in: Some(300),
                refresh_token: Some("refresh".to_owned()),
            })
            .await;

        let next = client.with_issuer("  https://idp.example/realms/x/  ");
        assert_eq!(next.issuer(), "https://idp.example/realms/x");
        assert_eq!(next.config.client_id, test_config().client_id);
        assert_eq!(next.config.redirect_uri, test_config().redirect_uri);
        assert_eq!(next.config.scope, test_config().scope);
        assert!(next.cached_access_token().await.is_none());
        assert!(client.cached_access_token().await.is_some());
    }

    #[test]
    fn discovery_url_targets_the_issuer_realm() {
        assert_eq!(
            test_config().discovery_url(),
            "https://ztna-kc.corp.dy-sec.com/realms/warp-ztna/.well-known/openid-configuration"
        );

        let trailing =
            OidcConfig { issuer: "https://idp.example/realms/x/".to_owned(), ..test_config() };
        assert_eq!(
            trailing.discovery_url(),
            "https://idp.example/realms/x/.well-known/openid-configuration"
        );
    }

    #[test]
    fn email_claim_reads_the_jwt_payload() {
        let token = signed_token(r#"{"email":"alice@corp.example"}"#);
        assert_eq!(email_claim(&token).as_deref(), Some("alice@corp.example"));
    }

    #[test]
    fn email_claim_trims_and_rejects_blank() {
        assert_eq!(
            email_claim(&signed_token(r#"{"email":"  alice@corp.example  "}"#)).as_deref(),
            Some("alice@corp.example")
        );
        assert_eq!(email_claim(&signed_token(r#"{"email":"   "}"#)), None);
        assert_eq!(email_claim(&signed_token(r#"{"email":""}"#)), None);
    }

    #[test]
    fn email_claim_rejects_malformed_tokens() {
        assert_eq!(email_claim(""), None);
        assert_eq!(email_claim("opaque-token"), None);
        assert_eq!(email_claim("a.b"), None);
        assert_eq!(email_claim("header.!!!not-base64!!!.sig"), None);
        assert_eq!(email_claim(&format!("h.{}.s", URL_SAFE_NO_PAD.encode("not json"))), None);
    }

    #[test]
    fn email_claim_rejects_missing_or_non_string_email() {
        assert_eq!(email_claim(&signed_token(r#"{"preferred_username":"alice"}"#)), None);
        assert_eq!(email_claim(&signed_token(r#"{"email":42}"#)), None);
    }

    #[test]
    fn code_challenge_matches_rfc7636_vector() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(code_challenge(verifier), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn authorize_url_contains_pkce_and_prompt() {
        let url = test_config().authorize_url("verifier", "state", Some("none")).unwrap();
        let url = Url::parse(&url).unwrap();
        let pairs: Vec<(String, String)> =
            url.query_pairs().map(|(key, value)| (key.into_owned(), value.into_owned())).collect();

        assert!(pairs.contains(&("client_id".to_owned(), "warp-ztna-app".to_owned())));
        assert!(pairs.contains(&("code_challenge".to_owned(), code_challenge("verifier"))));
        assert!(pairs.contains(&("code_challenge_method".to_owned(), "S256".to_owned())));
        assert!(pairs.contains(&("prompt".to_owned(), "none".to_owned())));
    }

    #[test]
    fn authorize_url_omits_prompt_for_interactive_flow() {
        let url = test_config().authorize_url("verifier", "state", None).unwrap();
        let url = Url::parse(&url).unwrap();
        let pairs: Vec<(String, String)> =
            url.query_pairs().map(|(key, value)| (key.into_owned(), value.into_owned())).collect();

        assert!(!pairs.iter().any(|(key, _)| key == "prompt"));
    }

    #[test]
    fn token_response_deserializes_with_and_without_refresh() {
        let with_refresh: TokenResponse =
            serde_json::from_str(r#"{"access_token":"a","expires_in":300,"refresh_token":"r"}"#)
                .unwrap();
        assert_eq!(with_refresh.access_token, "a");
        assert_eq!(with_refresh.expires_in, Some(300));
        assert_eq!(with_refresh.refresh_token.as_deref(), Some("r"));

        let without_refresh: TokenResponse =
            serde_json::from_str(r#"{"access_token":"b"}"#).unwrap();
        assert_eq!(without_refresh.access_token, "b");
        assert!(without_refresh.expires_in.is_none());
        assert!(without_refresh.refresh_token.is_none());
    }

    #[test]
    fn callback_params_parses_code_error_and_state() {
        let params = callback_params("GET /oauth/callback?code=c&state=s HTTP/1.1").unwrap();
        assert_eq!(params.code.as_deref(), Some("c"));
        assert_eq!(params.state.as_deref(), Some("s"));
        assert!(params.error.is_none());

        let params =
            callback_params("GET /oauth/callback?error=login_required&state=s HTTP/1.1").unwrap();
        assert_eq!(params.error.as_deref(), Some("login_required"));
        assert_eq!(params.state.as_deref(), Some("s"));
        assert!(params.code.is_none());
    }

    #[test]
    fn callback_params_ignores_non_callback_path() {
        assert!(callback_params("GET /favicon.ico HTTP/1.1").is_none());
    }

    #[test]
    fn interaction_required_errors_trigger_interactive_fallback() {
        assert!(is_interaction_required("login_required"));
        assert!(is_interaction_required("interaction_required"));
        assert!(!is_interaction_required("access_denied"));
    }

    #[tokio::test]
    async fn memory_cache_holds_access_and_refresh_tokens_until_clear() {
        let client = ApiOidc::new(test_config());
        client
            .cache_tokens(TokenResponse {
                access_token: "access".to_owned(),
                expires_in: Some(300),
                refresh_token: Some("refresh".to_owned()),
            })
            .await;

        assert_eq!(client.cached_access_token().await.as_deref(), Some("access"));
        assert_eq!(client.cached_refresh_token().await.as_deref(), Some("refresh"));

        client.clear().await.unwrap();
        assert!(client.cached_access_token().await.is_none());
        assert!(client.cached_refresh_token().await.is_none());
    }

    fn flow(name: &str) -> PendingFlow {
        PendingFlow {
            verifier: format!("verifier-{name}"),
            state: format!("state-{name}"),
            url: format!("https://idp.example/{name}"),
        }
    }

    #[test]
    fn callback_timeout_is_five_minutes() {
        assert_eq!(CALLBACK_TIMEOUT, Duration::from_mins(5));
    }

    #[test]
    fn classify_callback_ignores_stale_and_missing_state() {
        let silent = flow("silent");
        let interactive = flow("interactive");

        assert_eq!(
            classify_callback("GET /favicon.ico HTTP/1.1", &silent, &interactive),
            CallbackDirective::NotFound
        );
        assert_eq!(
            classify_callback("GET /oauth/callback?code=x HTTP/1.1", &silent, &interactive),
            CallbackDirective::BadRequest("missing state")
        );
        assert_eq!(
            classify_callback(
                "GET /oauth/callback?state=stale&code=x HTTP/1.1",
                &silent,
                &interactive
            ),
            CallbackDirective::BadRequest("state mismatch")
        );
    }

    #[test]
    fn classify_callback_matches_silent_and_interactive_flows() {
        let silent = flow("silent");
        let interactive = flow("interactive");

        assert_eq!(
            classify_callback(
                "GET /oauth/callback?state=state-silent&code=sc HTTP/1.1",
                &silent,
                &interactive
            ),
            CallbackDirective::SilentCode("sc".to_owned())
        );
        assert_eq!(
            classify_callback(
                "GET /oauth/callback?state=state-silent&error=interaction_required HTTP/1.1",
                &silent,
                &interactive
            ),
            CallbackDirective::SilentInteractionRequired
        );
        assert_eq!(
            classify_callback(
                "GET /oauth/callback?state=state-silent&error=access_denied HTTP/1.1",
                &silent,
                &interactive
            ),
            CallbackDirective::SilentError("access_denied".to_owned())
        );
        assert_eq!(
            classify_callback(
                "GET /oauth/callback?state=state-silent HTTP/1.1",
                &silent,
                &interactive
            ),
            CallbackDirective::Ignore
        );
        assert_eq!(
            classify_callback(
                "GET /oauth/callback?state=state-interactive&code=ic HTTP/1.1",
                &silent,
                &interactive
            ),
            CallbackDirective::InteractiveCode("ic".to_owned())
        );
        assert_eq!(
            classify_callback(
                "GET /oauth/callback?state=state-interactive&error=access_denied HTTP/1.1",
                &silent,
                &interactive
            ),
            CallbackDirective::InteractiveError("access_denied".to_owned())
        );
        assert_eq!(
            classify_callback(
                "GET /oauth/callback?state=state-interactive HTTP/1.1",
                &silent,
                &interactive
            ),
            CallbackDirective::Ignore
        );
    }

    #[test]
    fn cancel_signal_only_affects_recorded_attempts() {
        let signal = CancelSignal::default();
        let attempt = signal.attempt();
        assert!(!signal.is_cancelled(attempt));

        signal.cancel();
        assert!(signal.is_cancelled(attempt));
        assert!(!signal.is_cancelled(signal.attempt()));
    }

    #[tokio::test]
    async fn cancel_signal_wakes_waiters_and_is_idempotent() {
        let signal = Arc::new(CancelSignal::default());
        let attempt = signal.attempt();
        let waiter = Arc::clone(&signal);
        let task = tokio::spawn(async move { waiter.cancelled(attempt).await });

        tokio::time::sleep(Duration::from_millis(10)).await;
        signal.cancel();
        signal.cancel();
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("cancelled waiter should wake")
            .expect("waiter task");
        assert!(signal.is_cancelled(attempt));
    }

    #[tokio::test]
    async fn wait_for_callback_returns_cancelled() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let signal = Arc::new(CancelSignal::default());
        let attempt = signal.attempt();
        let silent = flow("silent");
        let interactive = flow("interactive");
        let waiter = Arc::clone(&signal);
        let task = tokio::spawn(async move {
            wait_for_callback(
                listener,
                &silent,
                &interactive,
                &waiter,
                attempt,
                Duration::from_secs(5),
                Duration::from_millis(50),
            )
            .await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        signal.cancel();
        let result = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("cancel should end the wait")
            .expect("waiter task")
            .expect("wait result");
        assert!(matches!(result, CallbackWait::Cancelled));
    }

    #[tokio::test]
    async fn wait_for_callback_times_out() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let signal = CancelSignal::default();
        let attempt = signal.attempt();
        let result = wait_for_callback(
            listener,
            &flow("silent"),
            &flow("interactive"),
            &signal,
            attempt,
            Duration::from_millis(50),
            Duration::from_millis(50),
        )
        .await
        .expect("wait result");
        assert!(matches!(result, CallbackWait::TimedOut));
    }

    #[tokio::test]
    async fn stale_callback_does_not_end_the_current_flow() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let signal = CancelSignal::default();
        let attempt = signal.attempt();
        let silent = flow("silent");
        let interactive = flow("interactive");
        let expected_verifier = interactive.verifier.clone();
        let task = tokio::spawn(async move {
            wait_for_callback(
                listener,
                &silent,
                &interactive,
                &signal,
                attempt,
                Duration::from_secs(2),
                Duration::from_millis(200),
            )
            .await
        });

        let mut stale = TcpStream::connect(address).await.unwrap();
        stale
            .write_all(b"GET /oauth/callback?state=stale&code=x HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();

        let mut valid = TcpStream::connect(address).await.unwrap();
        valid
            .write_all(
                b"GET /oauth/callback?state=state-interactive&code=abc HTTP/1.1\r\nHost: x\r\n\r\n",
            )
            .await
            .unwrap();

        let result = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("valid callback should end the wait")
            .expect("waiter task")
            .expect("wait result");
        let CallbackWait::Outcome(outcome) = result else {
            panic!("expected a callback outcome");
        };
        assert_eq!(outcome.verifier, expected_verifier);
        match outcome.callback {
            AuthCallback::Code(code) => assert_eq!(code, "abc"),
            AuthCallback::Error(error) => panic!("unexpected callback error: {error}"),
        }
    }

    #[tokio::test]
    async fn passive_token_reports_token_absent_and_busy() {
        let client = ApiOidc::new(test_config());
        assert_eq!(client.try_access_token().await, TokenAttempt::Absent);

        let guard = client.acquire_lock.lock().await;
        assert_eq!(client.try_access_token().await, TokenAttempt::Busy);
        drop(guard);

        client
            .cache_tokens(TokenResponse {
                access_token: "access".to_owned(),
                expires_in: Some(300),
                refresh_token: None,
            })
            .await;
        assert_eq!(client.try_access_token().await, TokenAttempt::Token("access".to_owned()));
    }

    #[tokio::test]
    async fn interactive_acquisition_does_not_queue_behind_a_busy_lock() {
        let client = ApiOidc::new(test_config());
        let guard = client.acquire_lock.lock().await;
        let result = client.acquire_interactive().await;
        assert!(matches!(result, Err(ApiSessionError::Busy)));
        drop(guard);
    }

    #[tokio::test]
    async fn issuer_swap_shares_the_cancel_signal() {
        let client = ApiOidc::new(test_config());
        let attempt = client.cancel.attempt();
        let next = client.with_issuer("https://idp.example/realms/x");
        next.cancel();
        assert!(client.cancel.is_cancelled(attempt));
    }

    #[tokio::test]
    async fn clear_cancels_the_pending_flow() {
        let client = ApiOidc::new(test_config());
        let attempt = client.cancel.attempt();
        client.clear().await.unwrap();
        assert!(client.cancel.is_cancelled(attempt));
    }

    fn free_loopback_port() -> u16 {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("free port");
        listener.local_addr().expect("local addr").port()
    }

    fn config_for_redirect(issuer: String, port: u16) -> OidcConfig {
        OidcConfig {
            issuer,
            client_id: "warp-ztna-app".to_owned(),
            redirect_uri: format!("http://127.0.0.1:{port}/oauth/callback"),
            scope: "openid profile email offline_access warp-ztna-api".to_owned(),
        }
    }

    async fn mock_issuer() -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("mock issuer");
        let address = listener.local_addr().expect("mock address");
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut reader = BufReader::new(stream);
                    let mut request_line = String::new();
                    if reader.read_line(&mut request_line).await.is_err() {
                        return;
                    }
                    let mut content_length = 0_usize;
                    loop {
                        let mut header = String::new();
                        match reader.read_line(&mut header).await {
                            Ok(0) | Err(_) => break,
                            Ok(_) if header == "\r\n" => break,
                            Ok(_) => {
                                if let Some(value) =
                                    header.to_ascii_lowercase().strip_prefix("content-length:")
                                {
                                    content_length = value.trim().parse().unwrap_or(0);
                                }
                            }
                        }
                    }
                    if content_length > 0 {
                        let mut body = vec![0_u8; content_length];
                        if reader.read_exact(&mut body).await.is_err() {
                            return;
                        }
                    }
                    let body = if request_line.contains("/.well-known/") {
                        "{}".to_owned()
                    } else {
                        r#"{"access_token":"test-access","expires_in":300,"refresh_token":"test-refresh"}"#
                            .to_owned()
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let mut stream = reader.into_inner();
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        (format!("http://{address}/realms/test"), task)
    }

    fn query_param(url: &str, key: &str) -> Option<String> {
        let parsed = Url::parse(url).ok()?;
        parsed.query_pairs().find(|(name, _)| name == key).map(|(_, value)| value.into_owned())
    }

    async fn loopback_get(port: u16, query: &str) -> reqwest::Response {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("loopback client")
            .get(format!("http://127.0.0.1:{port}/oauth/callback?{query}"))
            .send()
            .await
            .expect("loopback request")
    }

    #[test]
    fn redirect_addr_follows_the_configured_uri() {
        let mut config = test_config();
        config.redirect_uri = "http://127.0.0.1:41234/oauth/callback".to_owned();
        let client = ApiOidc::new(config);
        assert_eq!(client.redirect_addr().unwrap().port(), 41_234);
    }

    #[test]
    fn redirect_addr_rejects_non_loopback_hosts() {
        let mut config = test_config();
        config.redirect_uri = "http://idp.example:37123/oauth/callback".to_owned();
        let client = ApiOidc::new(config);
        assert!(client.redirect_addr().is_err());
    }

    #[tokio::test]
    async fn interactive_flow_handles_stale_callbacks_and_exchanges_the_code() {
        let (issuer, server) = mock_issuer().await;
        let port = free_loopback_port();
        let (urls, mut opened) = tokio::sync::mpsc::unbounded_channel::<String>();
        let client = ApiOidc::new(config_for_redirect(issuer, port)).with_opener(Arc::new(
            move |url: &str| {
                let _ = urls.send(url.to_owned());
                Ok(())
            },
        ));
        let attempt = client.cancel.attempt();
        let task = tokio::spawn(async move {
            client.acquire_within(attempt, Duration::from_secs(2), Duration::from_millis(200)).await
        });

        let silent = opened.recv().await.expect("silent authorization URL");
        assert_eq!(query_param(&silent, "prompt").as_deref(), Some("none"));
        assert_eq!(query_param(&silent, "code_challenge_method").as_deref(), Some("S256"));
        let silent_state = query_param(&silent, "state").expect("silent state");

        let stale = loopback_get(port, "state=stale&code=x").await;
        assert_eq!(stale.status(), reqwest::StatusCode::BAD_REQUEST);

        let redirect =
            loopback_get(port, &format!("state={silent_state}&error=interaction_required")).await;
        assert_eq!(redirect.status(), reqwest::StatusCode::FOUND);
        let interactive = redirect
            .headers()
            .get(reqwest::header::LOCATION)
            .expect("interactive location")
            .to_str()
            .expect("location text")
            .to_owned();
        assert!(query_param(&interactive, "prompt").is_none());
        let interactive_state = query_param(&interactive, "state").expect("interactive state");
        assert_ne!(interactive_state, silent_state);

        let completed =
            loopback_get(port, &format!("state={interactive_state}&code=test-code")).await;
        assert!(completed.status().is_success());

        let token = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("flow should finish")
            .expect("flow task")
            .expect("token");
        assert_eq!(token.access_token, "test-access");
        assert_eq!(token.refresh_token.as_deref(), Some("test-refresh"));
        server.abort();
    }

    #[tokio::test]
    async fn cancel_ends_the_interactive_flow() {
        let (issuer, server) = mock_issuer().await;
        let port = free_loopback_port();
        let client =
            ApiOidc::new(config_for_redirect(issuer, port)).with_opener(Arc::new(|_: &str| Ok(())));
        let attempt = client.cancel.attempt();
        let runner = client.clone();
        let task = tokio::spawn(async move {
            runner.acquire_within(attempt, Duration::from_secs(5), Duration::from_millis(200)).await
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        client.cancel();
        let result = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("cancel should end the flow")
            .expect("flow task");
        assert!(matches!(result, Err(ApiSessionError::Cancelled)));
        server.abort();
    }

    #[tokio::test]
    async fn interactive_flow_times_out_without_a_callback() {
        let (issuer, server) = mock_issuer().await;
        let port = free_loopback_port();
        let client =
            ApiOidc::new(config_for_redirect(issuer, port)).with_opener(Arc::new(|_: &str| Ok(())));
        let attempt = client.cancel.attempt();
        let result = client
            .acquire_within(attempt, Duration::from_millis(80), Duration::from_millis(50))
            .await;
        assert!(matches!(result, Err(ApiSessionError::TimedOut)));
        server.abort();
    }

    #[tokio::test]
    async fn browser_failure_is_reported_before_waiting() {
        let (issuer, server) = mock_issuer().await;
        let port = free_loopback_port();
        let client =
            ApiOidc::new(config_for_redirect(issuer, port)).with_opener(Arc::new(|_: &str| {
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no browser"))
            }));
        let attempt = client.cancel.attempt();
        let result = client
            .acquire_within(attempt, Duration::from_secs(1), Duration::from_millis(200))
            .await;
        assert!(matches!(result, Err(ApiSessionError::BrowserFailed(_))));
        server.abort();
    }
}
