use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use getrandom::getrandom;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::Mutex,
    time::timeout,
};
use url::Url;
use warp_ztna_err::prelude::*;

const REDIRECT_PORT: u16 = 37_123;
const CALLBACK_TIMEOUT: Duration = Duration::from_mins(3);
const ISSUER_PROBE_TIMEOUT: Duration = Duration::from_secs(2);
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

/// OIDC client for the API resource-server audience.
///
/// Access and refresh tokens are kept in memory only. Callers share one
/// serialized entry point ([`ApiOidc::access_token`]) so concurrent requests
/// never open two browser authorization flows at once.
#[derive(Clone)]
pub struct ApiOidc {
    config: Arc<OidcConfig>,
    cache: Arc<Mutex<Option<CachedToken>>>,
    acquire_lock: Arc<Mutex<()>>,
}

impl ApiOidc {
    pub fn new(config: OidcConfig) -> Self {
        Self {
            config: Arc::new(config),
            cache: Arc::new(Mutex::new(None)),
            acquire_lock: Arc::new(Mutex::new(())),
        }
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
        Self::new(config)
    }

    pub async fn access_token(&self) -> Result<String> {
        if !self.is_configured() {
            return Err(explain(Reason::InvalidArgument, "OIDC issuer is not configured"));
        }

        log::info!("api_oidc: acquiring API access token");
        let _guard = self.acquire_lock.lock().await;
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

        let tokens = self.acquire().await?;
        let access = tokens.access_token.clone();
        self.cache_tokens(tokens).await;
        Ok(access)
    }

    pub async fn clear(&self) -> Result<()> {
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
        reqwest::Client::new()
            .get(self.config.discovery_url())
            .timeout(ISSUER_PROBE_TIMEOUT)
            .send()
            .await
            .map(|_| ())
            .or_err(Reason::Unavailable, "reach the OIDC issuer")
    }

    async fn acquire(&self) -> Result<TokenResponse> {
        self.ensure_issuer_reachable().await?;
        let listener = loopback_listener().await?;
        let silent = self.pending_flow(Some("none"))?;
        let interactive = self.pending_flow(None)?;

        log::info!("api_oidc: starting silent authorization against issuer={}", self.config.issuer);
        open::that_detached(&silent.url).or_err(Reason::External, "open OIDC authorization")?;

        let outcome = wait_for_callback(listener, &silent, &interactive).await?;
        match outcome.callback {
            AuthCallback::Code(code) => self.exchange_code(&code, &outcome.verifier).await,
            AuthCallback::Error(error) => {
                Err(explain(Reason::Unauthorized, format!("OIDC authorization failed: {error}")))
            }
        }
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
        let result = reqwest::Client::new()
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
        let response = reqwest::Client::new()
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

async fn loopback_listener() -> Result<TcpListener> {
    let socket = SocketAddr::from(([127, 0, 0, 1], REDIRECT_PORT));
    TcpListener::bind(socket).await.or_err(Reason::Unavailable, "bind loopback OIDC callback")
}

async fn wait_for_callback(
    listener: TcpListener,
    silent: &PendingFlow,
    interactive: &PendingFlow,
) -> Result<CallbackOutcome> {
    let accept = async {
        loop {
            let (stream, _) =
                listener.accept().await.or_err(Reason::External, "accept OIDC callback")?;
            match handle_callback(stream, silent, interactive).await? {
                CallbackParse::Ignore => {}
                CallbackParse::SilentCode(code) => {
                    return Ok(CallbackOutcome {
                        callback: AuthCallback::Code(code),
                        verifier: silent.verifier.clone(),
                    });
                }
                CallbackParse::InteractiveCode(code) => {
                    return Ok(CallbackOutcome {
                        callback: AuthCallback::Code(code),
                        verifier: interactive.verifier.clone(),
                    });
                }
                CallbackParse::Error(error) => {
                    return Ok(CallbackOutcome {
                        callback: AuthCallback::Error(error),
                        verifier: String::new(),
                    });
                }
            }
        }
    };

    timeout(CALLBACK_TIMEOUT, accept).await.or_err(Reason::External, "OIDC callback timed out")?
}

async fn handle_callback(
    stream: TcpStream,
    silent: &PendingFlow,
    interactive: &PendingFlow,
) -> Result<CallbackParse> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).await.or_err(Reason::External, "read OIDC callback")?;
    let mut stream = reader.into_inner();

    let Some(params) = callback_params(&request_line) else {
        write_not_found(&mut stream).await?;
        return Ok(CallbackParse::Ignore);
    };
    let Some(state) = params.state else {
        write_error_page(&mut stream, "missing state").await?;
        return Err(explain(Reason::Unauthorized, "OIDC callback missing state"));
    };

    if state == silent.state {
        if let Some(code) = params.code {
            write_self_closing_page(&mut stream).await?;
            log::debug!("api_oidc: silent authorization code received");
            return Ok(CallbackParse::SilentCode(code));
        }
        if let Some(error) = params.error {
            if is_interaction_required(&error) {
                write_redirect(&mut stream, &interactive.url).await?;
                log::info!(
                    "api_oidc: silent login unavailable, redirecting to interactive authorization"
                );
                return Ok(CallbackParse::Ignore);
            }
            write_error_page(&mut stream, &error).await?;
            return Ok(CallbackParse::Error(error));
        }
        write_self_closing_page(&mut stream).await?;
        return Ok(CallbackParse::Ignore);
    }

    if state == interactive.state {
        if let Some(code) = params.code {
            write_self_closing_page(&mut stream).await?;
            log::debug!("api_oidc: interactive authorization code received");
            return Ok(CallbackParse::InteractiveCode(code));
        }
        if let Some(error) = params.error {
            write_error_page(&mut stream, &error).await?;
            return Ok(CallbackParse::Error(error));
        }
        write_self_closing_page(&mut stream).await?;
        return Ok(CallbackParse::Ignore);
    }

    log::warn!("api_oidc: callback state did not match either pending flow");
    write_error_page(&mut stream, "state mismatch").await?;
    Err(explain(Reason::Unauthorized, "OIDC callback state mismatch"))
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
}
