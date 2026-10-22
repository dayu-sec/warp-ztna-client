use std::sync::{Arc, RwLock};
use std::time::Duration;

use log::LevelFilter;
use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Emitter, Manager,
    image::Image,
    menu::{MenuBuilder, MenuEvent, MenuItemBuilder, SubmenuBuilder},
    tray::TrayIconBuilder,
};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind, TimezoneStrategy};
use time::macros::format_description;
use tokio::sync::Mutex;

mod api_oidc;
mod error;
#[cfg(target_os = "macos")]
mod macos_chrome;
#[cfg(target_os = "macos")]
mod macos_tray;
mod process;
mod service_config;
mod ssh_login;
mod sso_tab;
mod status_hub;

use api_oidc::{ApiOidc, OidcConfig};
use error::{
    CmdResult, CommandError, SSH_CERT_NOT_READY_CODE, SSH_CLIENT_MISSING_CODE,
    SSH_NO_LOCAL_KEY_CODE, SSH_NO_REGISTERED_HOST_CODE, SSH_PRINCIPAL_MISMATCH_CODE, classify,
    lift_mesh, root_message,
};
use status_hub::{StatusDto, StatusHub};
use warp_ztna_cfg::prelude::AgentConfig;
use warp_ztna_err::prelude::*;
use warp_ztna_nbrpc::prelude::{ActiveProfile, ConnectionState, MeshAgentRpc, ProfileInfo};

const API_TIMEOUT: Duration = Duration::from_secs(10);

const API_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

const CLIENT_CONFIG_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone)]
struct PlatformService {
    portal_url: String,
    api_auth: ApiOidc,
}

#[derive(Clone)]
struct DesktopState {
    daemon_addr: String,
    api_base_url: String,
    api_token: Option<String>,
    platform: Arc<RwLock<PlatformService>>,
    user_email: Arc<Mutex<Option<String>>>,
    token_email: Arc<Mutex<Option<String>>>,
    hub: Arc<StatusHub>,
    task: Arc<std::sync::Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
}

impl DesktopState {
    fn platform(&self) -> PlatformService {
        match self.platform.read() {
            Ok(platform) => platform.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SsoStartDto {
    Ready,
    SsoRequired { user_code: String, verification_uri: String, verification_uri_complete: String },
}

#[derive(Debug, Serialize)]
struct ExtendStartDto {
    user_code: String,
    device_code: String,
    verification_uri: String,
    verification_uri_complete: String,
    expires_in: i64,
}

#[derive(Debug, Serialize)]
struct ProfileDto {
    id: String,
    name: String,
    url: String,
    is_active: bool,
}

pub(crate) fn state_name(state: ConnectionState) -> &'static str {
    match state {
        ConnectionState::Unknown => "unknown",
        ConnectionState::Disconnected => "disconnected",
        ConnectionState::Connecting => "connecting",
        ConnectionState::NeedsLogin => "needs_login",
        ConnectionState::Connected => "connected",
        ConnectionState::Error => "error",
    }
}

fn os_username() -> String {
    std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default()
}

async fn connect_client(state: &DesktopState) -> Result<MeshAgentRpc> {
    lift_mesh(MeshAgentRpc::connect_str(&state.daemon_addr).await, "connect netbird daemon")
}

fn stop_status_stream(state: &DesktopState) {
    let Ok(mut slot) = state.task.lock() else {
        return;
    };
    if let Some(task) = slot.take() {
        task.abort();
    }
}

async fn status_now(app: &AppHandle, state: &DesktopState) -> CmdResult<StatusDto> {
    let mut client = connect_client(state).await?;
    let view = lift_mesh(client.status().await, "agent status")?;
    Ok(status_hub::publish_view(app, &state.hub, &mut client, view).await)
}

#[tauri::command]
async fn agent_status(
    app: AppHandle,
    state: tauri::State<'_, DesktopState>,
) -> CmdResult<StatusDto> {
    if let Some(dto) = state.hub.cached_if_live().await
        && dto.cacheable()
    {
        log::debug!("agent_status: served from the status stream state={}", dto.state);
        return Ok(dto);
    }
    let dto = status_now(&app, &state).await?;
    log::debug!("agent_status: dialed the daemon state={}", dto.state);
    Ok(dto)
}

#[tauri::command]
async fn up(app: AppHandle, state: tauri::State<'_, DesktopState>) -> CmdResult<StatusDto> {
    log::info!("connect: requested");
    let mut client = connect_client(&state).await?;
    lift_mesh(client.up().await, "connect")?;
    let view = lift_mesh(client.status().await, "agent status")?;
    Ok(status_hub::publish_view(&app, &state.hub, &mut client, view).await)
}

#[tauri::command]
async fn disconnect(app: AppHandle, state: tauri::State<'_, DesktopState>) -> CmdResult<StatusDto> {
    log::info!("disconnect: requested");
    let mut client = connect_client(&state).await?;
    lift_mesh(client.down().await, "disconnect")?;
    let dto = StatusDto::disconnected(state.hub.stream_health().await);
    state.hub.publish(dto.clone()).await;
    status_hub::emit_status(&app, &dto);
    Ok(dto)
}

const DEFAULT_MANAGEMENT_URL: &str = "https://ztna.corp.dy-sec.com:443";

fn is_netbird_cloud_management_url(url: &str) -> bool {
    matches!(
        url.trim().trim_end_matches('/'),
        "https://api.netbird.io:443" | "https://api.netbird.io"
    )
}

fn effective_management_url(profile_url: Option<&str>, from_env: &str) -> String {
    if let Some(url) = profile_url {
        let url = url.trim();
        if !url.is_empty() && !is_netbird_cloud_management_url(url) {
            return url.to_owned();
        }
    }
    let from_env = from_env.trim();
    if !from_env.is_empty() {
        return from_env.to_owned();
    }
    DEFAULT_MANAGEMENT_URL.to_owned()
}

fn placeholder_replacement(profile_url: &str, from_env: &str) -> Option<String> {
    if !is_netbird_cloud_management_url(profile_url) {
        return None;
    }
    let resolved = effective_management_url(None, from_env);
    if is_netbird_cloud_management_url(&resolved) {
        return None;
    }
    Some(resolved)
}

async fn resolve_management_url(
    client: &mut MeshAgentRpc,
    active: Option<&ActiveProfile>,
) -> String {
    let profile_url = if let Some(active) = active {
        client.get_profile_config(&active.username, &active.name).await.ok()
    } else {
        None
    };
    let from_env = std::env::var("WARP_NETBIRD_MANAGEMENT_URL").unwrap_or_default();
    effective_management_url(profile_url.as_deref(), &from_env)
}

fn open_sso_browser(verification_uri: &str, verification_uri_complete: &str) -> Result<()> {
    let url = if verification_uri_complete.trim().is_empty() {
        verification_uri.trim()
    } else {
        verification_uri_complete.trim()
    };
    if url.is_empty() {
        return Err(explain(Reason::External, "management did not return a sign-in URL"));
    }
    open::that_detached(url).or_err(Reason::External, "open browser for sso")
}

async fn login_outcome(client: &mut MeshAgentRpc) -> CmdResult<SsoStartDto> {
    let active = client.get_active_profile().await.ok();
    let url = resolve_management_url(client, active.as_ref()).await;
    let username = active
        .as_ref()
        .map(|profile| profile.username.as_str())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            let name = os_username();
            if name.is_empty() { None } else { Some(name) }
        });
    let profile_name = active.as_ref().map(|profile| profile.name.as_str());
    match lift_mesh(
        client.login_sso(Some(&url), None, profile_name, username.as_deref()).await,
        "login sso",
    )? {
        warp_ztna_nbrpc::prelude::LoginOutcome::Ready => Ok(SsoStartDto::Ready),
        warp_ztna_nbrpc::prelude::LoginOutcome::SsoRequired {
            user_code,
            verification_uri,
            verification_uri_complete,
        } => {
            Ok(SsoStartDto::SsoRequired { user_code, verification_uri, verification_uri_complete })
        }
    }
}

/// Probe only: never open the browser. Ready means skip the welcome page.
#[tauri::command]
async fn check_login(state: tauri::State<'_, DesktopState>) -> CmdResult<SsoStartDto> {
    log::debug!("check_login: probing existing login");
    let mut client = connect_client(&state).await?;
    login_outcome(&mut client).await
}

#[tauri::command]
async fn start_sso(state: tauri::State<'_, DesktopState>) -> CmdResult<SsoStartDto> {
    log::info!("start_sso: initiating browser sign-in");
    let mut client = connect_client(&state).await?;
    let outcome = login_outcome(&mut client).await?;
    if let SsoStartDto::SsoRequired { verification_uri, verification_uri_complete, .. } = &outcome {
        open_sso_browser(verification_uri, verification_uri_complete)?;
    }
    Ok(outcome)
}

#[tauri::command]
async fn extend_sso(state: tauri::State<'_, DesktopState>) -> CmdResult<ExtendStartDto> {
    log::info!("extend_sso: requesting session extension");
    let mut client = connect_client(&state).await?;
    let start = lift_mesh(client.request_extend_session(None).await, "extend session")?;
    open_sso_browser(&start.verification_uri, &start.verification_uri_complete)?;
    Ok(ExtendStartDto {
        user_code: start.user_code,
        device_code: start.device_code,
        verification_uri: start.verification_uri,
        verification_uri_complete: start.verification_uri_complete,
        expires_in: start.expires_in,
    })
}

#[tauri::command]
async fn wait_extend_sso(
    app: AppHandle,
    state: tauri::State<'_, DesktopState>,
    device_code: String,
    user_code: String,
) -> CmdResult<StatusDto> {
    log::debug!("wait_extend_sso: waiting for browser approval");
    let mut client = connect_client(&state).await?;
    lift_mesh(client.wait_extend_session(&device_code, &user_code).await, "wait extend session")?;
    sso_tab::close_netbird_redirect_tabs();
    focus_visible_main_window(&app);
    let view = lift_mesh(client.status().await, "agent status")?;
    Ok(status_hub::publish_view(&app, &state.hub, &mut client, view).await)
}

#[tauri::command]
async fn dismiss_session_warning(state: tauri::State<'_, DesktopState>) -> CmdResult<()> {
    log::debug!("dismiss_session_warning: dismissing daemon warning");
    let mut client = connect_client(&state).await?;
    let _ = client.dismiss_session_warning().await;
    Ok(())
}

#[tauri::command]
async fn wait_sso(
    app: AppHandle,
    state: tauri::State<'_, DesktopState>,
    user_code: String,
) -> CmdResult<StatusDto> {
    log::info!("wait_sso: waiting for browser sign-in");
    let mut client = connect_client(&state).await?;
    match client.wait_sso_login(&user_code, None).await {
        Ok(email) => {
            let email = email.trim();
            if !email.is_empty() {
                *state.user_email.lock().await = Some(email.to_owned());
            }
        }
        Err(error) => {
            if error.is_pending_approval() {
                sso_tab::close_netbird_redirect_tabs();
            }
            if let Some(classified) = classify(&error, "wait sso") {
                return Err(classified);
            }
            lift_mesh(Err(error), "wait sso")?;
        }
    }
    sso_tab::close_netbird_redirect_tabs();
    focus_visible_main_window(&app);
    let view = lift_mesh(client.status().await, "agent status")?;
    Ok(status_hub::publish_view(&app, &state.hub, &mut client, view).await)
}

#[tauri::command]
async fn list_profiles(state: tauri::State<'_, DesktopState>) -> CmdResult<Vec<ProfileDto>> {
    log::debug!("list_profiles: loading profile list");
    let mut client = connect_client(&state).await?;
    let user = os_username();
    let from_env = std::env::var("WARP_NETBIRD_MANAGEMENT_URL").unwrap_or_default();
    let profiles: Vec<ProfileInfo> = lift_mesh(client.list_profiles(&user).await, "list profiles")?;
    let mut out = Vec::new();
    for p in profiles {
        let mut url = client.get_profile_config(&user, &p.name).await.unwrap_or_default();
        if p.is_active
            && let Some(replacement) = placeholder_replacement(&url, &from_env)
        {
            match client.set_profile_url(&user, &p.name, &replacement).await {
                Ok(()) => {
                    log::info!("list_profiles: replaced cloud default for profile {}", p.name);
                    url = replacement;
                }
                Err(error) => {
                    log::warn!("list_profiles: could not write back management url: {error}");
                }
            }
        }
        out.push(ProfileDto { id: p.id, name: p.name, url, is_active: p.is_active });
    }
    Ok(out)
}

#[tauri::command]
async fn add_profile(
    state: tauri::State<'_, DesktopState>,
    name: String,
    url: String,
) -> CmdResult<String> {
    log::info!("add_profile: name={name}");
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err(explain(Reason::InvalidArgument, "management URL must be http(s)").into());
    }
    let mut client = connect_client(&state).await?;
    lift_mesh(client.add_profile(&os_username(), &name, &url).await, "add profile")
        .map_err(Into::into)
}

#[tauri::command]
async fn update_profile(
    state: tauri::State<'_, DesktopState>,
    handle: String,
    name: String,
    url: String,
) -> CmdResult<()> {
    log::info!("update_profile: handle={handle} name={name}");
    let mut client = connect_client(&state).await?;
    lift_mesh(
        client.update_profile(&os_username(), &handle, Some(&name), Some(&url)).await,
        "update profile",
    )
    .map_err(Into::into)
}

#[tauri::command]
async fn remove_profile(state: tauri::State<'_, DesktopState>, handle: String) -> CmdResult<()> {
    log::info!("remove_profile: handle={handle}");
    let mut client = connect_client(&state).await?;
    lift_mesh(client.remove_profile(&os_username(), &handle).await, "remove profile")
        .map_err(Into::into)
}

#[tauri::command]
async fn switch_profile(
    state: tauri::State<'_, DesktopState>,
    handle: String,
) -> CmdResult<String> {
    log::info!("switch_profile: handle={handle}");
    let mut client = connect_client(&state).await?;
    let _ = client.down().await;
    lift_mesh(client.switch_profile(&os_username(), &handle).await, "switch profile")
        .map_err(Into::into)
}

async fn note_token_email(state: &DesktopState, token: &str) {
    let Some(email) = api_oidc::email_claim(token) else {
        return;
    };
    let mut slot = state.token_email.lock().await;
    if slot.as_deref() != Some(email.as_str()) {
        *slot = Some(email);
    }
}

async fn api_context(state: &DesktopState, context: &str) -> CmdResult<Option<(String, String)>> {
    let base = state.api_base_url.clone();
    let platform = state.platform();
    let token = if let Some(token) = &state.api_token {
        token.clone()
    } else if platform.api_auth.is_configured() {
        platform.api_auth.access_token().await?
    } else {
        log::debug!("{context}: OIDC is not configured; continuing without it");
        return Ok(None);
    };
    note_token_email(state, &token).await;
    Ok(Some((base, token)))
}

fn address_key(value: &str) -> String {
    let normalised = value.trim().to_ascii_lowercase();
    normalised
        .strip_suffix("/32")
        .or_else(|| normalised.strip_suffix("/128"))
        .unwrap_or(&normalised)
        .to_owned()
}

async fn api_get(
    base: &str,
    path: &str,
    token: &str,
    context: &'static str,
) -> CmdResult<serde_json::Value> {
    let url = format!("{}/{}", base.trim_end_matches('/'), path.trim_start_matches('/'));
    let response = reqwest::Client::new()
        .get(url)
        .bearer_auth(token)
        .timeout(API_TIMEOUT)
        .send()
        .await
        .or_err(Reason::External, context)?;
    if !response.status().is_success() {
        log::warn!("{context}: API returned HTTP {}", response.status());
        let reason = match response.status().as_u16() {
            401 => Reason::Unauthorized,
            403 => Reason::Forbidden,
            _ => Reason::External,
        };
        return Err(explain(reason, format!("api HTTP {}", response.status())).into());
    }
    response.json().await.or_err(Reason::External, context).map_err(Into::into)
}

#[derive(Debug, Deserialize)]
struct ClientConfigResponse {
    #[serde(default)]
    portal_url: Option<String>,
    oidc_issuer: String,
}

#[derive(Debug, PartialEq, Eq)]
enum ClientConfigStatus {
    Present,
    Missing,
    Failed,
}

fn classify_client_config_status(code: u16) -> ClientConfigStatus {
    if (200..300).contains(&code) {
        ClientConfigStatus::Present
    } else if matches!(code, 404 | 405) {
        ClientConfigStatus::Missing
    } else {
        ClientConfigStatus::Failed
    }
}

fn normalized_issuer(value: &str) -> &str {
    value.trim().trim_end_matches('/')
}

fn accepted_portal_url(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() || service_config::validate_endpoint(value).is_err() {
        return None;
    }
    Some(value.to_owned())
}

fn apply_client_config(platform: &mut PlatformService, response: &ClientConfigResponse) -> bool {
    if let Some(portal_url) = accepted_portal_url(response.portal_url.as_deref()) {
        platform.portal_url = portal_url;
    }
    let issuer = normalized_issuer(&response.oidc_issuer);
    if issuer.is_empty() || issuer == platform.api_auth.issuer() {
        return false;
    }
    platform.api_auth = platform.api_auth.with_issuer(issuer);
    true
}

async fn fetch_client_config(base: &str, state: &DesktopState) -> Result<(), String> {
    let url = format!("{base}/api/v1/client-config");
    let response =
        reqwest::Client::new().get(&url).timeout(CLIENT_CONFIG_TIMEOUT).send().await.map_err(
            |cause| root_message(&because(Reason::External, "fetch client config", cause)),
        )?;
    let status = response.status().as_u16();
    match classify_client_config_status(status) {
        ClientConfigStatus::Present => {}
        ClientConfigStatus::Missing => {
            log::info!("api_probe: client config endpoint is absent; keeping fallbacks");
            return Ok(());
        }
        ClientConfigStatus::Failed => return Err(format!("api client config HTTP {status}")),
    }
    let config = response
        .json::<ClientConfigResponse>()
        .await
        .map_err(|error| format!("decode client config: {error}"))?;
    let issuer_changed = {
        let mut platform = match state.platform.write() {
            Ok(platform) => platform,
            Err(poisoned) => poisoned.into_inner(),
        };
        let changed = apply_client_config(&mut platform, &config);
        drop(platform);
        changed
    };
    if issuer_changed {
        *state.token_email.lock().await = None;
    }
    log::info!("api_probe: client config applied (issuer_changed={issuer_changed})");
    Ok(())
}

#[derive(Debug, Serialize)]
struct ApiProbeDto {
    status: &'static str,
    detail: Option<String>,
}

impl ApiProbeDto {
    fn unconfigured() -> Self {
        Self { status: "unconfigured", detail: None }
    }

    fn from_http_status(code: u16) -> Self {
        let (status, detail) = classify_probe_response(code);
        Self { status, detail }
    }

    fn unreachable(detail: String) -> Self {
        Self { status: "unreachable", detail: Some(detail) }
    }
}

fn classify_probe_response(code: u16) -> (&'static str, Option<String>) {
    if (200..300).contains(&code) {
        ("ok", None)
    } else {
        ("unreachable", Some(format!("api HTTP {code}")))
    }
}

#[tauri::command]
async fn api_probe(state: tauri::State<'_, DesktopState>) -> CmdResult<ApiProbeDto> {
    let base = state.api_base_url.trim_end_matches('/').to_owned();
    let url = format!("{base}/health/ready");
    log::info!("api_probe: probing {url}");
    let status = match reqwest::Client::new().get(&url).timeout(API_PROBE_TIMEOUT).send().await {
        Ok(response) => response.status().as_u16(),
        Err(cause) => {
            let error = because(Reason::External, "probe platform API", cause);
            let outcome = ApiProbeDto::unreachable(root_message(&error));
            log::info!("api_probe: status={} detail={:?}", outcome.status, outcome.detail);
            return Ok(outcome);
        }
    };
    if !(200..300).contains(&status) {
        let outcome = ApiProbeDto::from_http_status(status);
        log::info!("api_probe: status={} detail={:?}", outcome.status, outcome.detail);
        return Ok(outcome);
    }
    if let Err(detail) = fetch_client_config(&base, &state).await {
        log::warn!("api_probe: client config unavailable: {detail}");
        return Ok(ApiProbeDto::unreachable(detail));
    }
    let platform = state.platform();
    if state.api_token.is_none() && !platform.api_auth.is_configured() {
        log::warn!("api_probe: no API token source configured (WARP_API_TOKEN or OIDC)");
        return Ok(ApiProbeDto::unconfigured());
    }
    let outcome = ApiProbeDto::from_http_status(status);
    log::info!("api_probe: status={} detail={:?}", outcome.status, outcome.detail);
    Ok(outcome)
}

#[tauri::command]
async fn my_access_resources(
    state: tauri::State<'_, DesktopState>,
) -> CmdResult<serde_json::Value> {
    log::info!("my_access_resources: loading granted resources");
    let empty_catalog = serde_json::json!({ "groups": [], "addresses": [] });
    let Some((base, token)) = api_context(&state, "my_access_resources").await? else {
        return Ok(
            serde_json::json!({ "requests": [], "catalog": empty_catalog, "group_grants": [] }),
        );
    };
    let (authorized, approved, grants) = tokio::join!(
        api_get(
            &base,
            "api/v1/access-requests?status=authorized&limit=200&all=false",
            &token,
            "fetch authorized requests"
        ),
        api_get(
            &base,
            "api/v1/access-requests?status=approved&limit=200&all=false",
            &token,
            "fetch approved requests"
        ),
        api_get(&base, "api/v1/me/group-grants", &token, "fetch group grants"),
    );
    let mut requests = Vec::new();
    for payload in [authorized?, approved?] {
        if let Some(items) = payload.get("items").and_then(serde_json::Value::as_array) {
            requests.extend(items.iter().cloned());
        }
    }
    let catalog = match api_get(
        &base,
        "api/v1/assets/access-request-catalog",
        &token,
        "fetch access request catalog",
    )
    .await
    {
        Ok(payload) => payload,
        Err(error) => {
            log::warn!("my_access_resources: catalog unavailable: {}", error.message);
            empty_catalog
        }
    };
    let group_grants = match grants {
        Ok(payload) => {
            payload.get("items").and_then(serde_json::Value::as_array).cloned().unwrap_or_default()
        }
        Err(error) => {
            log::warn!("my_access_resources: group grants unavailable: {}", error.message);
            Vec::new()
        }
    };
    log::info!(
        "my_access_resources: loaded {} granted requests and {} group grants",
        requests.len(),
        group_grants.len()
    );
    Ok(
        serde_json::json!({ "requests": requests, "catalog": catalog, "group_grants": group_grants }),
    )
}

async fn fetch_mesh_status(state: &DesktopState) -> CmdResult<serde_json::Value> {
    let Some((base, token)) = api_context(state, "my_mesh_status").await? else {
        return Ok(serde_json::json!({ "status": "unknown" }));
    };
    let url = format!("{}/api/v1/me/mesh-status", base.trim_end_matches('/'));
    let response = reqwest::Client::new()
        .get(url)
        .bearer_auth(token)
        .timeout(API_TIMEOUT)
        .send()
        .await
        .or_err(Reason::External, "fetch my mesh status")?;
    if !response.status().is_success() {
        log::warn!("my_mesh_status: API returned HTTP {}", response.status());
        let reason = match response.status().as_u16() {
            401 => Reason::Unauthorized,
            403 => Reason::Forbidden,
            _ => Reason::External,
        };
        return Err(explain(reason, format!("api HTTP {}", response.status())).into());
    }
    let payload: serde_json::Value =
        response.json().await.or_err(Reason::External, "decode API mesh status")?;
    let status = payload.get("status").and_then(serde_json::Value::as_str).unwrap_or("unknown");
    log::info!("my_mesh_status: status={status}");
    Ok(payload)
}

#[tauri::command]
async fn my_mesh_status(state: tauri::State<'_, DesktopState>) -> CmdResult<serde_json::Value> {
    log::info!("my_mesh_status: loading admission status");
    fetch_mesh_status(&state).await
}

#[derive(Debug, Serialize)]
struct CurrentUserDto {
    email: Option<String>,
}

#[tauri::command]
async fn current_user(state: tauri::State<'_, DesktopState>) -> CmdResult<CurrentUserDto> {
    log::info!("current_user: resolving signed-in user");
    let cached = state.user_email.lock().await.clone();
    if let Some(email) = cached {
        log::debug!("current_user: using email captured at sign-in");
        return Ok(CurrentUserDto { email: Some(email) });
    }
    let from_token = state.token_email.lock().await.clone();
    if let Some(email) = from_token {
        log::debug!("current_user: using email from the API access token");
        return Ok(CurrentUserDto { email: Some(email) });
    }
    let email = match fetch_mesh_status(&state).await {
        Ok(payload) => {
            payload.get("email").and_then(serde_json::Value::as_str).map(ToOwned::to_owned)
        }
        Err(error) => {
            log::debug!("current_user: mesh status unavailable: {}", error.message);
            None
        }
    };
    if let Some(email) = email {
        return Ok(CurrentUserDto { email: Some(email) });
    }
    let stashed = state.token_email.lock().await.clone();
    if stashed.is_some() {
        log::debug!("current_user: using email from the API access token after mesh lookup");
    }
    Ok(CurrentUserDto { email: stashed })
}

#[tauri::command]
async fn acquire_api_session(state: tauri::State<'_, DesktopState>) -> CmdResult<()> {
    log::info!("acquire_api_session: ensuring API token");
    let platform = state.platform();
    if state.api_token.is_some() || !platform.api_auth.is_configured() {
        return Ok(());
    }
    let _ = platform.api_auth.access_token().await?;
    Ok(())
}

#[tauri::command]
async fn clear_api_session(state: tauri::State<'_, DesktopState>) -> CmdResult<()> {
    log::info!("clear_api_session: clearing API session");
    state.platform().api_auth.clear().await?;
    Ok(())
}

#[tauri::command]
fn open_url(url: String) -> CmdResult<()> {
    log::debug!("open_url: opening destination");
    open::that(url).or_err(Reason::External, "open url").map_err(Into::into)
}

fn portal_view_url(base: &str, view: &str) -> String {
    let separator = if base.contains('?') { '&' } else { '?' };
    format!("{base}{separator}view={view}")
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn open_portal(state: tauri::State<'_, DesktopState>, view: Option<String>) -> CmdResult<()> {
    let platform = state.platform();
    let url = match view.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        Some(view) => portal_view_url(&platform.portal_url, view),
        None => platform.portal_url.clone(),
    };
    log::debug!("open_portal: opening access portal");
    open::that(url).or_err(Reason::External, "open portal").map_err(Into::into)
}

fn portal_help_url(base: &str) -> String {
    format!("{}/help/", base.trim_end_matches('/'))
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn open_help(state: tauri::State<'_, DesktopState>) -> CmdResult<()> {
    log::debug!("open_help: opening the help centre");
    open::that(portal_help_url(&state.platform().portal_url))
        .or_err(Reason::External, "open help")
        .map_err(Into::into)
}

#[allow(clippy::too_many_lines)]
async fn prepare_ssh_launch(
    app: &tauri::AppHandle,
    state: &DesktopState,
    request_id: String,
    address: String,
    operation: &str,
) -> CmdResult<ssh_login::LaunchRequest> {
    log::info!("{operation}: preparing an ssh session");
    let Some((base, token)) = api_context(state, operation).await? else {
        return Err(CommandError::coded(
            Reason::Unauthorized.code(),
            "the API session is not available",
        ));
    };
    let certificates =
        api_get(&base, "api/v1/me/ssh-certificates", &token, "fetch ssh certificates").await?;
    let config =
        api_get(&base, "api/v1/me/ssh-client-config", &token, "fetch ssh client config").await?;
    let wanted = address_key(&address);
    let host = config.get("hosts").and_then(serde_json::Value::as_array).and_then(|hosts| {
        hosts.iter().find(|host| {
            host.get("address")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|value| address_key(value) == wanted)
        })
    });
    let account = host
        .and_then(|host| host.get("login_account"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    let host_principal = host
        .and_then(|host| host.get("host_principal"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    let Some(account) = account else {
        return Err(CommandError::coded(
            SSH_NO_REGISTERED_HOST_CODE,
            "no registered ssh host covers this address",
        ));
    };
    let expected_principal = host_principal.unwrap_or_else(|| account.clone());
    let candidates = certificates
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|item| {
                    item.get("access_request_id").and_then(serde_json::Value::as_str)
                        == Some(request_id.as_str())
                        && item.get("status").and_then(serde_json::Value::as_str) == Some("issued")
                        && item
                            .get("public_key_fingerprint")
                            .and_then(serde_json::Value::as_str)
                            .is_some()
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if candidates.is_empty() {
        return Err(CommandError::coded(
            SSH_CERT_NOT_READY_CODE,
            "no issued certificate is available for this ticket",
        ));
    }
    let mut fingerprints = Vec::new();
    let mut serials = Vec::new();
    for candidate in &candidates {
        let Some(fingerprint) =
            candidate.get("public_key_fingerprint").and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        if fingerprints.iter().any(|known| known == fingerprint) {
            continue;
        }
        fingerprints.push(fingerprint.to_owned());
        serials.push((
            fingerprint.to_owned(),
            candidate.get("serial").and_then(serde_json::Value::as_str).map(ToOwned::to_owned),
        ));
    }
    if !ssh_login::client_available() {
        return Err(CommandError::coded(
            SSH_CLIENT_MISSING_CODE,
            "no ssh client was found on this machine",
        ));
    }
    let home = app
        .path()
        .home_dir()
        .map_err(|error| CommandError::coded(SSH_NO_LOCAL_KEY_CODE, error.to_string()))?;
    let platform = ssh_login::Platform::current();
    let resolution = {
        let home = home.clone();
        let account = account.clone();
        let address = address.clone();
        let request_id = request_id.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let key =
                ssh_login::resolve_signing_key(&home, platform, &account, &address, &fingerprints)?;
            let serial = serials
                .iter()
                .find(|(fingerprint, _)| fingerprint == &key.fingerprint)
                .and_then(|(_, serial)| serial.clone());
            let on_disk = ssh_login::ticket_certificate_path(&home, &request_id)
                .and_then(|path| ssh_login::existing_certificate(&path, serial.as_deref()));
            Some((key, on_disk))
        })
        .await
        .ok()
        .flatten()
    };
    let Some((key, on_disk)) = resolution else {
        return Err(CommandError::coded(
            SSH_NO_LOCAL_KEY_CODE,
            "no local private key matches the certificate",
        ));
    };
    let matched = candidates.iter().find(|candidate| {
        candidate.get("public_key_fingerprint").and_then(serde_json::Value::as_str)
            == Some(key.fingerprint.as_str())
    });
    let Some(certificate) = matched else {
        return Err(CommandError::coded(
            SSH_NO_LOCAL_KEY_CODE,
            "the local private key has no certificate for this ticket",
        ));
    };
    let text = if let Some(text) = on_disk {
        text
    } else {
        let Some(id) = certificate.get("id").and_then(serde_json::Value::as_str) else {
            return Err(CommandError::coded(
                SSH_CERT_NOT_READY_CODE,
                "the certificate has no identifier",
            ));
        };
        api_get(
            &base,
            &format!("api/v1/me/ssh-certificates/{id}/download"),
            &token,
            "download ssh certificate",
        )
        .await?
        .get("certificate")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            CommandError::coded(SSH_CERT_NOT_READY_CODE, "the certificate has no downloadable body")
        })?
    };
    let principals = ssh_login::certificate_principals(&text).unwrap_or_default();
    if !principals.iter().any(|principal| principal == &expected_principal || principal == &account)
    {
        return Err(CommandError::coded(
            SSH_PRINCIPAL_MISMATCH_CODE,
            "the certificate does not cover this host",
        ));
    }
    let known_hosts_line = config
        .get("known_hosts")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            CommandError::coded(SSH_CERT_NOT_READY_CODE, "the API returned no host authority line")
        })?;
    Ok(ssh_login::LaunchRequest {
        request_id,
        home,
        account,
        host: address,
        key,
        certificate: text,
        certificate_serial: certificate
            .get("serial")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        certificate_valid_before: certificate
            .get("valid_before")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        known_hosts_line,
    })
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
async fn ssh_launch(
    app: tauri::AppHandle,
    state: tauri::State<'_, DesktopState>,
    request_id: String,
    address: String,
) -> CmdResult<ssh_login::SshLaunchDto> {
    let request = prepare_ssh_launch(&app, &state, request_id, address, "ssh_launch").await?;
    tauri::async_runtime::spawn_blocking(move || ssh_login::launch(&request))
        .await
        .map_err(|error| CommandError::coded(Reason::Internal.code(), error.to_string()))?
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
async fn ssh_prepare(
    app: tauri::AppHandle,
    state: tauri::State<'_, DesktopState>,
    request_id: String,
    address: String,
) -> CmdResult<ssh_login::SshPrepareDto> {
    let request = prepare_ssh_launch(&app, &state, request_id, address, "ssh_prepare").await?;
    tauri::async_runtime::spawn_blocking(move || ssh_login::prepare_dto(&request))
        .await
        .map_err(|error| CommandError::coded(Reason::Internal.code(), error.to_string()))?
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn start_dragging(window: tauri::Window) -> CmdResult<()> {
    window.start_dragging().or_err(Reason::Internal, "window chrome").map_err(Into::into)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn minimize_window(window: tauri::Window) -> CmdResult<()> {
    window.minimize().or_err(Reason::Internal, "window chrome").map_err(Into::into)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn toggle_maximize_window(window: tauri::Window) -> CmdResult<()> {
    if window.is_maximized().or_err(Reason::Internal, "window chrome")? {
        window.unmaximize().or_err(Reason::Internal, "window chrome").map_err(Into::into)
    } else {
        window.maximize().or_err(Reason::Internal, "window chrome").map_err(Into::into)
    }
}

#[cfg(windows)]
fn apply_win11_corners(hwnd: isize) {
    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWCP_ROUND: u32 = 2;
    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: isize,
            attr: u32,
            value: *const core::ffi::c_void,
            size: u32,
        ) -> i32;
    }
    let pref = DWMWCP_ROUND;
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            core::ptr::from_ref(&pref).cast(),
            4,
        );
    }
}

#[tauri::command]
async fn quit_app(app: tauri::AppHandle, state: tauri::State<'_, DesktopState>) -> CmdResult<()> {
    log::info!("quit: requested");
    stop_status_stream(&state);
    let _ = state.platform().api_auth.clear().await;
    if let Ok(mut client) = connect_client(&state).await {
        let _ = client.down().await;
    }
    app.exit(0);
    Ok(())
}

fn logging_plugin_builder() -> tauri_plugin_log::Builder {
    let log_time_format = format_description!("[year]-[month]-[day]_[hour]-[minute]-[second]");
    tauri_plugin_log::Builder::new()
        .targets([Target::new(TargetKind::LogDir { file_name: Some("agent".into()) })])
        .level(LevelFilter::Info)
        .level_for(
            "warp_ztna_app_lib",
            if cfg!(debug_assertions) { LevelFilter::Debug } else { LevelFilter::Info },
        )
        .rotation_strategy(RotationStrategy::KeepSome(5))
        .max_file_size(5 * 1024 * 1024)
        .timezone_strategy(TimezoneStrategy::UseLocal)
        .format(move |out, message, record| {
            let module = record.module_path().unwrap_or_else(|| record.target());
            let location = match (record.file(), record.line()) {
                (Some(file), Some(line)) => format!("{}:{line}", log_file_name(file)),
                (Some(file), None) => log_file_name(file).to_owned(),
                _ => "-".to_owned(),
            };
            out.finish(format_args!(
                "{}[{}][{}] {} [{}]",
                TimezoneStrategy::UseLocal.get_now().format(&log_time_format).unwrap(),
                module,
                record.level(),
                message,
                location,
            ));
        })
}

fn log_file_name(file: &str) -> &str {
    file.rsplit(['/', '\\']).next().unwrap_or(file)
}

const TRAY_ID: &str = "main";

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        #[cfg(target_os = "macos")]
        macos_chrome::activate();
        emit_window_visibility(&window, true);
    }
}

fn focus_visible_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main")
        && window.is_visible().unwrap_or(false)
        && !window.is_minimized().unwrap_or(false)
    {
        let _ = window.set_focus();
        #[cfg(target_os = "macos")]
        macos_chrome::activate();
    }
}

fn emit_window_visibility(window: &impl Emitter<tauri::Wry>, visible: bool) {
    log::debug!("window: visibility changed visible={visible}");
    let _ = window.emit("window-visibility", visible);
}

fn tray_open_settings(app: &AppHandle) {
    show_main_window(app);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit("tray-open-settings", ());
    }
}

fn tray_connect(app: &AppHandle) {
    let Some(state) = app.try_state::<DesktopState>() else {
        return;
    };
    let daemon_addr = state.daemon_addr.clone();
    log::info!("tray: connect requested");
    tauri::async_runtime::spawn(async move {
        if let Ok(mut client) = MeshAgentRpc::connect_str(&daemon_addr).await {
            let _ = client.up().await;
        }
    });
}

fn tray_disconnect(app: &AppHandle) {
    let Some(state) = app.try_state::<DesktopState>() else {
        return;
    };
    let daemon_addr = state.daemon_addr.clone();
    log::info!("tray: disconnect requested");
    tauri::async_runtime::spawn(async move {
        if let Ok(mut client) = MeshAgentRpc::connect_str(&daemon_addr).await {
            let _ = client.down().await;
        }
    });
}

fn tray_quit(app: &AppHandle) {
    let handle = app.clone();
    log::info!("tray: quit requested");
    tauri::async_runtime::spawn(async move {
        if let Some(state) = handle.try_state::<DesktopState>() {
            stop_status_stream(&state);
            let _ = state.platform().api_auth.clear().await;
            if let Ok(mut client) = MeshAgentRpc::connect_str(&state.daemon_addr).await {
                let _ = client.down().await;
            }
        }
        handle.exit(0);
    });
}

fn tray_request_access(app: &AppHandle) {
    let Some(state) = app.try_state::<DesktopState>() else {
        return;
    };
    let platform = state.platform();
    let url = portal_view_url(&platform.portal_url, "request");
    log::info!("tray: request access requested");
    if let Err(cause) = open::that(url) {
        log::warn!("tray: open access portal failed: {cause}");
    }
}

fn handle_tray_menu_event(app: &AppHandle, event: &MenuEvent) {
    match event.id().as_ref() {
        "open" | "resources-more" => show_main_window(app),
        "request" => tray_request_access(app),
        "connect" => tray_connect(app),
        "disconnect" => tray_disconnect(app),
        "settings" => tray_open_settings(app),
        "quit" => tray_quit(app),
        _ => {}
    }
}

fn tray_icon_bytes(theme: &str) -> &'static [u8] {
    if theme == "light" {
        include_bytes!("../../src/assets/icon-black.png")
    } else {
        include_bytes!("../../src/assets/icon-white.png")
    }
}

fn set_tray_icon(app: &AppHandle, theme: &str) -> CmdResult<()> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };
    let icon = Image::from_bytes(tray_icon_bytes(theme)).or_err(Reason::Internal, "tray icon")?;
    tray.set_icon(Some(icon)).or_err(Reason::Internal, "set tray icon")?;
    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn set_tray_icon_theme(app: AppHandle, theme: String) -> CmdResult<()> {
    set_tray_icon(&app, &theme)
}

#[derive(serde::Deserialize)]
struct TrayMenuLabels {
    open: String,
    request: String,
    resources: String,
    connect: String,
    disconnect: String,
    settings: String,
    quit: String,
    more: String,
}

#[derive(serde::Deserialize)]
struct TrayResource {
    label: String,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    status: String,
}

fn tray_menu(
    app: &AppHandle,
    labels: &TrayMenuLabels,
    connected: bool,
    resources: &[TrayResource],
    more: bool,
    api_blocked: bool,
) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let (lease_id, lease_label) = if connected {
        ("disconnect", labels.disconnect.as_str())
    } else {
        ("connect", labels.connect.as_str())
    };
    let open =
        MenuItemBuilder::with_id("open", &labels.open).accelerator("CmdOrCtrl+M").build(app)?;
    let request = MenuItemBuilder::with_id("request", &labels.request)
        .accelerator("CmdOrCtrl+Shift+A")
        .build(app)?;
    let lease = MenuItemBuilder::with_id(lease_id, lease_label)
        .accelerator("CmdOrCtrl+Enter")
        .enabled(connected || !api_blocked)
        .build(app)?;
    let settings = MenuItemBuilder::with_id("settings", &labels.settings)
        .accelerator("CmdOrCtrl+S")
        .build(app)?;
    let quit =
        MenuItemBuilder::with_id("quit", &labels.quit).accelerator("CmdOrCtrl+Q").build(app)?;

    let mut resource_items = Vec::with_capacity(resources.len());
    for (index, resource) in resources.iter().enumerate() {
        resource_items
            .push(MenuItemBuilder::with_id(format!("res:{index}"), &resource.label).build(app)?);
    }

    let has_resources = !resources.is_empty() || more;
    let resources_menu = if has_resources {
        let more_item = MenuItemBuilder::with_id("resources-more", &labels.more).build(app)?;
        let mut builder = SubmenuBuilder::with_id(app, "resources", &labels.resources);
        for item in &resource_items {
            builder = builder.item(item);
        }
        if more {
            builder = builder.item(&more_item);
        }
        Some(builder.build()?)
    } else {
        None
    };

    let mut builder = MenuBuilder::new(app).item(&open).item(&request);
    if let Some(submenu) = &resources_menu {
        builder = builder.separator().item(submenu);
    }
    builder.separator().item(&lease).separator().item(&settings).item(&quit).build()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn set_tray_menu(
    app: AppHandle,
    labels: TrayMenuLabels,
    connected: bool,
    resources: Vec<TrayResource>,
    more: bool,
    api_blocked: bool,
) -> CmdResult<()> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };
    let menu = tray_menu(&app, &labels, connected, &resources, more, api_blocked)
        .or_err(Reason::Internal, "build tray menu")?;
    tray.set_menu(Some(menu)).or_err(Reason::Internal, "set tray menu")?;
    #[cfg(target_os = "macos")]
    macos_tray::apply(&tray, &resources);
    Ok(())
}

#[derive(Serialize)]
#[allow(clippy::struct_field_names)]
struct ServiceConfigDto {
    stored_api_url: String,
    effective_api_url: String,
}

fn stored_service_config(app: &AppHandle) -> service_config::ServiceConfig {
    app.path().app_config_dir().ok().as_deref().map(service_config::load).unwrap_or_default()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn get_service_config(app: AppHandle, state: tauri::State<'_, DesktopState>) -> ServiceConfigDto {
    let stored = stored_service_config(&app);
    ServiceConfigDto {
        stored_api_url: stored.api_url,
        effective_api_url: state.api_base_url.clone(),
    }
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn save_service_config(app: AppHandle, api_url: String) -> CmdResult<()> {
    if let Err(message) = service_config::validate_endpoint(&api_url) {
        return Err(explain(Reason::InvalidArgument, message).into());
    }
    let dir = app.path().app_config_dir().map_err(|error| {
        log::warn!("save_service_config: cannot resolve config dir: {error}");
        CommandError::from(explain(Reason::External, "resolve app config dir"))
    })?;
    service_config::save_api_url(&dir, &api_url).map_err(|error| {
        log::warn!("save_service_config: cannot write config: {error}");
        CommandError::from(explain(Reason::External, "write app config"))
    })?;
    log::info!("save_service_config: api url stored");
    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn restart_app(app: AppHandle) {
    let handle = app.clone();
    log::info!("restart_app: restarting application");
    tauri::async_runtime::spawn(async move {
        handle.restart();
    });
}

const DEFAULT_PORTAL_URL: &str = "https://ztna-portal.corp.dy-sec.com";
const DEFAULT_API_URL: &str = "https://ztna-api.corp.dy-sec.com";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[allow(clippy::too_many_lines)]
/// Starts the desktop application event loop.
///
/// # Panics
///
/// Panics if the native Tauri runtime cannot be initialized.
pub fn run() {
    let log_builder = logging_plugin_builder();

    tauri::Builder::default()
        .plugin(log_builder.build())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let stored = app
                .path()
                .app_config_dir()
                .ok()
                .as_deref()
                .map(service_config::load)
                .unwrap_or_default();
            let api_base_url = service_config::endpoint(
                std::env::var("WARP_API_URL").ok().as_deref(),
                Some(stored.api_url.as_str()),
                DEFAULT_API_URL,
            );
            let portal_url = service_config::endpoint(
                None,
                Some(stored.portal_url.as_str()),
                DEFAULT_PORTAL_URL,
            );
            app.manage(DesktopState {
                daemon_addr: AgentConfig::from_env().netbird_daemon_addr,
                api_base_url,
                api_token: std::env::var("WARP_API_TOKEN").ok().filter(|s| !s.is_empty()),
                platform: Arc::new(RwLock::new(PlatformService {
                    portal_url,
                    api_auth: ApiOidc::new(OidcConfig::from_env()),
                })),
                user_email: Arc::new(Mutex::new(None)),
                token_email: Arc::new(Mutex::new(None)),
                hub: Arc::new(StatusHub::default()),
                task: Arc::new(std::sync::Mutex::new(None)),
            });
            log::info!("Warp ZTNA desktop application started");
            if let Some(window) = app.get_webview_window("main") {
                #[cfg(target_os = "macos")]
                macos_chrome::apply(&window);
                #[cfg(windows)]
                if let Ok(hwnd) = window.hwnd() {
                    apply_win11_corners(hwnd.0 as isize);
                }
                let _ = window.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
            }

            let fallback_labels = TrayMenuLabels {
                open: "显示主窗口".into(),
                request: "申请访问".into(),
                resources: "授权资源".into(),
                connect: "连接".into(),
                disconnect: "断开".into(),
                settings: "设置".into(),
                quit: "退出".into(),
                more: "更多…".into(),
            };
            let tray_menu = tray_menu(app.handle(), &fallback_labels, false, &[], false, false)?;

            let tray_icon = Image::from_bytes(tray_icon_bytes("dark")).ok();
            let mut tray_builder = TrayIconBuilder::with_id(TRAY_ID)
                .menu(&tray_menu)
                .tooltip("零通")
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| handle_tray_menu_event(app, &event));
            if let Some(icon) = tray_icon {
                tray_builder = tray_builder.icon(icon);
            }
            tray_builder.build(app)?;
            #[cfg(target_os = "macos")]
            if let Some(tray) = app.tray_by_id(TRAY_ID) {
                macos_tray::apply(&tray, &[]);
            }

            let handle = app.handle().clone();
            let state = app.state::<DesktopState>();
            let task = tauri::async_runtime::spawn(status_hub::run_status_stream(
                handle,
                state.daemon_addr.clone(),
                Arc::clone(&state.hub),
            ));
            if let Ok(mut slot) = state.task.lock() {
                *slot = Some(task);
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            agent_status,
            up,
            disconnect,
            start_sso,
            check_login,
            extend_sso,
            wait_extend_sso,
            dismiss_session_warning,
            wait_sso,
            list_profiles,
            add_profile,
            update_profile,
            remove_profile,
            switch_profile,
            api_probe,
            my_access_resources,
            my_mesh_status,
            current_user,
            acquire_api_session,
            clear_api_session,
            set_tray_icon_theme,
            set_tray_menu,
            open_url,
            open_portal,
            open_help,
            ssh_launch,
            ssh_prepare,
            start_dragging,
            minimize_window,
            toggle_maximize_window,
            quit_app,
            get_service_config,
            save_service_config,
            restart_app,
        ])
        .on_window_event(|window, event| {
            #[cfg(target_os = "macos")]
            if matches!(
                event,
                tauri::WindowEvent::Focused(_)
                    | tauri::WindowEvent::ThemeChanged(_)
                    | tauri::WindowEvent::Resized(_)
            ) && let Ok(ns_window) = window.ns_window()
            {
                macos_chrome::hide_min_zoom(ns_window);
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                emit_window_visibility(window, false);
            }
            if let tauri::WindowEvent::Focused(focused) = event {
                let visible = window.is_visible().unwrap_or(true);
                let minimized = window.is_minimized().unwrap_or(false);
                if *focused {
                    emit_window_visibility(window, true);
                } else if !visible || minimized {
                    emit_window_visibility(window, false);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("running Warp ZTNA desktop application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_response_2xx_is_ok_without_detail() {
        for code in [200u16, 204] {
            let dto = ApiProbeDto::from_http_status(code);
            assert_eq!(dto.status, "ok");
            assert!(dto.detail.is_none());
        }
    }

    #[test]
    fn probe_response_non_2xx_is_unreachable_with_http_detail() {
        for code in [404u16, 500, 503] {
            let dto = ApiProbeDto::from_http_status(code);
            assert_eq!(dto.status, "unreachable");
            assert_eq!(dto.detail, Some(format!("api HTTP {code}")));
        }
    }

    #[test]
    fn probe_dto_serializes_status_and_detail() {
        let ok = serde_json::to_value(ApiProbeDto::from_http_status(204)).unwrap();
        assert_eq!(ok["status"], "ok");
        assert_eq!(ok["detail"], serde_json::Value::Null);

        let down = serde_json::to_value(ApiProbeDto::unreachable(
            "Connection refused (os error 61)".into(),
        ))
        .unwrap();
        assert_eq!(down["status"], "unreachable");
        assert_eq!(down["detail"], "Connection refused (os error 61)");

        let unconfigured = serde_json::to_value(ApiProbeDto::unconfigured()).unwrap();
        assert_eq!(unconfigured["status"], "unconfigured");
        assert_eq!(unconfigured["detail"], serde_json::Value::Null);
    }

    fn test_platform() -> PlatformService {
        PlatformService {
            portal_url: "https://fallback.example".to_owned(),
            api_auth: ApiOidc::new(OidcConfig {
                issuer: "https://idp.example/realms/x".to_owned(),
                client_id: "warp-ztna-app".to_owned(),
                redirect_uri: "http://127.0.0.1:37123/oauth/callback".to_owned(),
                scope: "openid".to_owned(),
            }),
        }
    }

    #[test]
    fn client_config_status_separates_missing_and_failures() {
        assert_eq!(classify_client_config_status(200), ClientConfigStatus::Present);
        assert_eq!(classify_client_config_status(204), ClientConfigStatus::Present);
        assert_eq!(classify_client_config_status(404), ClientConfigStatus::Missing);
        assert_eq!(classify_client_config_status(405), ClientConfigStatus::Missing);
        assert_eq!(classify_client_config_status(500), ClientConfigStatus::Failed);
    }

    #[test]
    fn client_config_decodes_explicit_and_missing_portal() {
        let config: ClientConfigResponse = serde_json::from_str(
            r#"{"portal_url":null,"oidc_issuer":"https://idp.example/realms/x"}"#,
        )
        .expect("explicit null");
        assert!(config.portal_url.is_none());
        assert_eq!(config.oidc_issuer, "https://idp.example/realms/x");

        let config: ClientConfigResponse =
            serde_json::from_str(r#"{"oidc_issuer":"https://idp.example/realms/x"}"#)
                .expect("missing portal");
        assert!(config.portal_url.is_none());

        assert!(
            serde_json::from_str::<ClientConfigResponse>(r#"{"portal_url":"https://p"}"#).is_err()
        );
    }

    #[test]
    fn client_config_applies_portal_and_issuer() {
        let mut platform = test_platform();
        let changed = apply_client_config(
            &mut platform,
            &ClientConfigResponse {
                portal_url: Some(" https://portal.example/ ".to_owned()),
                oidc_issuer: " https://idp.example/realms/y/ ".to_owned(),
            },
        );
        assert!(changed);
        assert_eq!(platform.portal_url, "https://portal.example/");
        assert_eq!(platform.api_auth.issuer(), "https://idp.example/realms/y");
    }

    #[test]
    fn client_config_ignores_invalid_portal_and_same_issuer() {
        let mut platform = test_platform();
        let before = platform.api_auth.issuer().to_owned();
        let changed = apply_client_config(
            &mut platform,
            &ClientConfigResponse {
                portal_url: Some("portal.example".to_owned()),
                oidc_issuer: format!("{before}/"),
            },
        );
        assert!(!changed);
        assert_eq!(platform.portal_url, "https://fallback.example");
        assert_eq!(platform.api_auth.issuer(), before);
    }

    #[test]
    fn netbird_cloud_management_urls_are_recognized() {
        for url in [
            "https://api.netbird.io:443",
            "https://api.netbird.io",
            " https://api.netbird.io:443/ ",
        ] {
            assert!(is_netbird_cloud_management_url(url));
        }
        assert!(!is_netbird_cloud_management_url(""));
        assert!(!is_netbird_cloud_management_url("https://ztna.corp.dy-sec.com:443"));
    }

    #[test]
    fn cloud_default_profile_url_falls_through_to_env_then_default() {
        assert_eq!(
            effective_management_url(Some("https://api.netbird.io:443"), "https://corp.example"),
            "https://corp.example"
        );
        assert_eq!(
            effective_management_url(Some("https://api.netbird.io"), "  "),
            DEFAULT_MANAGEMENT_URL
        );
    }

    #[test]
    fn portal_help_url_appends_help_path_without_double_slashes() {
        assert_eq!(
            portal_help_url("https://ztna-portal.corp.dy-sec.com"),
            "https://ztna-portal.corp.dy-sec.com/help/"
        );
        assert_eq!(
            portal_help_url("https://ztna-portal.corp.dy-sec.com/"),
            "https://ztna-portal.corp.dy-sec.com/help/"
        );
    }

    #[test]
    fn configured_profile_url_wins_over_env() {
        assert_eq!(
            effective_management_url(
                Some("https://ztna.corp.dy-sec.com:443"),
                "https://other.example"
            ),
            "https://ztna.corp.dy-sec.com:443"
        );
    }

    #[test]
    fn empty_profile_url_falls_through_to_env_then_default() {
        assert_eq!(
            effective_management_url(Some("   "), "https://corp.example"),
            "https://corp.example"
        );
        assert_eq!(effective_management_url(None, ""), DEFAULT_MANAGEMENT_URL);
    }

    #[test]
    fn placeholder_replacement_rewrites_cloud_default() {
        assert_eq!(
            placeholder_replacement("https://api.netbird.io:443", ""),
            Some(DEFAULT_MANAGEMENT_URL.to_owned())
        );
        assert_eq!(
            placeholder_replacement(" https://api.netbird.io:443/ ", "https://corp.example"),
            Some("https://corp.example".to_owned())
        );
    }

    #[test]
    fn placeholder_replacement_leaves_other_values_alone() {
        assert_eq!(placeholder_replacement("https://ztna.corp.dy-sec.com:443", ""), None);
        assert_eq!(placeholder_replacement("", "https://corp.example"), None);
        assert_eq!(
            placeholder_replacement("https://api.netbird.io:443", "https://api.netbird.io"),
            None
        );
    }
}
