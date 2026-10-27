use std::time::Duration;

use tonic::Request;
use tonic::transport::{Channel, Endpoint};

use crate::addr::{DaemonAddr, resolve_daemon_addr};
use crate::error::MeshAgentRpcError;
use crate::map::{
    DiagnosticsView, LoginOutcome, StatusFrame, StatusView, status_frame_from_response,
    status_view_from_response,
};
use crate::pb::daemon_service_client::DaemonServiceClient;
use crate::pb::{
    AddProfileRequest, DismissSessionWarningRequest, DownRequest, GetActiveProfileRequest,
    GetConfigRequest, GetFeaturesRequest, ListNetworksRequest, ListProfilesRequest, LoginRequest,
    LogoutRequest, RemoveProfileRequest, RenameProfileRequest, RequestExtendAuthSessionRequest,
    SetConfigRequest, StatusRequest, StatusResponse, SwitchProfileRequest, UpRequest,
    WaitExtendAuthSessionRequest, WaitSsoLoginRequest,
};
use crate::version::{SUPPORTED_NETBIRD_MINOR, is_supported_daemon_version};

const UNARY_TIMEOUT: Duration = Duration::from_secs(10);
const OPERATION_TIMEOUT: Duration = Duration::from_mins(1);
const WAIT_TIMEOUT: Duration = Duration::from_mins(10);

async fn with_budget<T>(
    budget: Duration,
    future: impl std::future::Future<Output = Result<T, tonic::Status>>,
) -> Result<T, MeshAgentRpcError> {
    match tokio::time::timeout(budget, future).await {
        Ok(result) => Ok(result?),
        Err(_) => Err(MeshAgentRpcError::Timeout(budget)),
    }
}

async fn with_timeout<T>(
    future: impl std::future::Future<Output = Result<T, tonic::Status>>,
) -> Result<T, MeshAgentRpcError> {
    with_budget(UNARY_TIMEOUT, future).await
}

/// Async client for `NetBird` `DaemonService`.
#[derive(Debug, Clone)]
pub struct MeshAgentRpc {
    inner: DaemonServiceClient<Channel>,
    addr: DaemonAddr,
}

impl MeshAgentRpc {
    /// Dials the daemon at `addr` (after unix socket discovery when applicable).
    ///
    /// # Errors
    ///
    /// Returns transport or address errors when the daemon cannot be reached.
    pub async fn connect(addr: DaemonAddr) -> Result<Self, MeshAgentRpcError> {
        let resolved = resolve_daemon_addr(&addr);
        let channel = connect_channel(&resolved).await?;
        Ok(Self { inner: DaemonServiceClient::new(channel), addr: resolved })
    }

    /// Parses `addr` then [`connect`](Self::connect).
    pub async fn connect_str(addr: &str) -> Result<Self, MeshAgentRpcError> {
        Self::connect(DaemonAddr::parse(addr)?).await
    }

    #[must_use]
    pub fn addr(&self) -> &DaemonAddr {
        &self.addr
    }

    /// Fetches full status and maps it for the UI.
    pub async fn status(&mut self) -> Result<StatusView, MeshAgentRpcError> {
        let response = with_timeout(self.inner.status(Request::new(StatusRequest {
            get_full_peer_status: true,
            should_run_probes: false,
            wait_for_ready: Some(false),
        })))
        .await?
        .into_inner();
        Ok(status_view_from_response(&response))
    }

    pub async fn subscribe_status(&mut self) -> Result<StatusStream, MeshAgentRpcError> {
        let response = with_timeout(self.inner.subscribe_status(Request::new(StatusRequest {
            get_full_peer_status: true,
            should_run_probes: false,
            wait_for_ready: Some(false),
        })))
        .await?;
        Ok(StatusStream { inner: response.into_inner() })
    }

    /// Login with a setup key and optional management URL override.
    pub async fn login_setup_key(
        &mut self,
        setup_key: &str,
        management_url: Option<&str>,
    ) -> Result<LoginOutcome, MeshAgentRpcError> {
        let response = with_budget(
            OPERATION_TIMEOUT,
            self.inner.login(Request::new(LoginRequest {
                setup_key: setup_key.to_owned(),
                management_url: management_url.unwrap_or("").to_owned(),
                is_unix_desktop_client: cfg!(unix),
                ..Default::default()
            })),
        )
        .await?
        .into_inner();
        Ok(LoginOutcome::from_response(response))
    }

    /// Start SSO login (empty setup key). Returns verification URI when required.
    pub async fn login_sso(
        &mut self,
        management_url: Option<&str>,
        hint: Option<&str>,
        profile_name: Option<&str>,
        username: Option<&str>,
    ) -> Result<LoginOutcome, MeshAgentRpcError> {
        let response = with_budget(
            OPERATION_TIMEOUT,
            self.inner.login(Request::new(LoginRequest {
                setup_key: String::new(),
                management_url: management_url.unwrap_or("").to_owned(),
                is_unix_desktop_client: cfg!(unix),
                hint: hint.map(ToOwned::to_owned),
                profile_name: profile_name
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned),
                username: username
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned),
                ..Default::default()
            })),
        )
        .await?
        .into_inner();
        Ok(LoginOutcome::from_response(response))
    }

    /// Blocks until browser SSO completes for `user_code`.
    pub async fn wait_sso_login(
        &mut self,
        user_code: &str,
        hostname: Option<&str>,
    ) -> Result<String, MeshAgentRpcError> {
        let response = with_budget(
            WAIT_TIMEOUT,
            self.inner.wait_sso_login(Request::new(WaitSsoLoginRequest {
                user_code: user_code.to_owned(),
                hostname: hostname.unwrap_or("").to_owned(),
            })),
        )
        .await?
        .into_inner();
        Ok(response.email)
    }

    /// Starts the engine (`async_ = true` so UI can follow status streams).
    pub async fn up(&mut self) -> Result<(), MeshAgentRpcError> {
        with_budget(
            OPERATION_TIMEOUT,
            self.inner.up(Request::new(UpRequest { r#async: true, ..Default::default() })),
        )
        .await?;
        Ok(())
    }

    /// Stops the engine without removing the peer from management.
    pub async fn down(&mut self) -> Result<(), MeshAgentRpcError> {
        with_budget(OPERATION_TIMEOUT, self.inner.down(Request::new(DownRequest {}))).await?;
        Ok(())
    }

    /// Starts session extension SSO. Caller must open the returned URI and then wait.
    pub async fn request_extend_session(
        &mut self,
        hint: Option<&str>,
    ) -> Result<ExtendSessionStart, MeshAgentRpcError> {
        let response = with_budget(
            OPERATION_TIMEOUT,
            self.inner.request_extend_auth_session(Request::new(RequestExtendAuthSessionRequest {
                hint: hint.map(str::trim).filter(|value| !value.is_empty()).map(ToOwned::to_owned),
            })),
        )
        .await?
        .into_inner();
        Ok(ExtendSessionStart {
            user_code: response.user_code,
            device_code: response.device_code,
            verification_uri: response.verification_uri,
            verification_uri_complete: response.verification_uri_complete,
            expires_in: response.expires_in,
        })
    }

    /// Blocks until the user finishes the extend-session SSO in the browser.
    pub async fn wait_extend_session(
        &mut self,
        device_code: &str,
        user_code: &str,
    ) -> Result<Option<i64>, MeshAgentRpcError> {
        let response = with_budget(
            WAIT_TIMEOUT,
            self.inner.wait_extend_auth_session(Request::new(WaitExtendAuthSessionRequest {
                device_code: device_code.to_owned(),
                user_code: user_code.to_owned(),
            })),
        )
        .await?
        .into_inner();
        Ok(response.session_expires_at.map(|ts| ts.seconds))
    }

    /// Best-effort: suppress the daemon's final warning dialog after the user dismissed ours.
    pub async fn dismiss_session_warning(&mut self) -> Result<(), MeshAgentRpcError> {
        with_timeout(
            self.inner.dismiss_session_warning(Request::new(DismissSessionWarningRequest {})),
        )
        .await?;
        Ok(())
    }

    /// Signs out the current profile (clears local login). Next `Login` can require SSO.
    pub async fn logout(
        &mut self,
        profile_name: Option<&str>,
        username: Option<&str>,
    ) -> Result<(), MeshAgentRpcError> {
        with_timeout(
            self.inner.logout(Request::new(LogoutRequest {
                profile_name: profile_name
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned),
                username: username
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned),
            })),
        )
        .await?;
        Ok(())
    }

    /// Setup-key enroll: Login then Up when SSO is not required.
    pub async fn enroll_setup_key(
        &mut self,
        setup_key: &str,
        management_url: Option<&str>,
    ) -> Result<StatusView, MeshAgentRpcError> {
        match self.login_setup_key(setup_key, management_url).await? {
            LoginOutcome::Ready => {
                self.up().await?;
                self.status().await
            }
            LoginOutcome::SsoRequired { .. } => Err(MeshAgentRpcError::Message(
                "daemon requested SSO login; use the SSO enroll path".to_owned(),
            )),
        }
    }

    /// Completes SSO after the user finished browser auth, then brings the engine up.
    pub async fn enroll_finish_sso(
        &mut self,
        user_code: &str,
        hostname: Option<&str>,
    ) -> Result<StatusView, MeshAgentRpcError> {
        self.wait_sso_login(user_code, hostname).await?;
        self.up().await?;
        self.status().await
    }

    /// Aggregated diagnostics for the Desktop diagnostics panel.
    pub async fn diagnostics(&mut self) -> DiagnosticsView {
        let daemon_addr = self.addr.display();
        match self.status().await {
            Ok(status) => {
                let version = status.daemon_version.clone().unwrap_or_else(|| "unknown".to_owned());
                DiagnosticsView {
                    daemon_reachable: true,
                    daemon_addr,
                    daemon_version: version,
                    platform: std::env::consts::OS.to_owned(),
                    status_detail: status.detail,
                }
            }
            Err(error) => DiagnosticsView {
                daemon_reachable: false,
                daemon_addr,
                daemon_version: "unavailable".to_owned(),
                platform: std::env::consts::OS.to_owned(),
                status_detail: Some(error.to_string()),
            },
        }
    }

    /// Ensures daemon version is on the supported minor line when reported.
    pub async fn ensure_supported_version(&mut self) -> Result<String, MeshAgentRpcError> {
        let status = self.status().await?;
        let version = status.daemon_version.unwrap_or_else(|| "unknown".to_owned());
        if !is_supported_daemon_version(&version) {
            return Err(MeshAgentRpcError::UnsupportedVersion(format!(
                "{version} (supported: {SUPPORTED_NETBIRD_MINOR}.x)"
            )));
        }
        Ok(version)
    }

    /// Optional probe that the features RPC is implemented (daemon not ancient).
    pub async fn get_features_ok(&mut self) -> bool {
        with_timeout(self.inner.get_features(Request::new(GetFeaturesRequest {}))).await.is_ok()
    }

    /// Lists daemon profiles for the current OS user.
    pub async fn list_profiles(
        &mut self,
        username: &str,
    ) -> Result<Vec<ProfileInfo>, MeshAgentRpcError> {
        let response =
            with_timeout(self.inner.list_profiles(Request::new(ListProfilesRequest {
                username: username.to_owned(),
            })))
            .await?
            .into_inner();
        Ok(response
            .profiles
            .into_iter()
            .map(|p| ProfileInfo { id: p.id, name: p.name, is_active: p.is_active })
            .collect())
    }

    /// Active profile display name / id.
    pub async fn get_active_profile(&mut self) -> Result<ActiveProfile, MeshAgentRpcError> {
        let response =
            with_timeout(self.inner.get_active_profile(Request::new(GetActiveProfileRequest {})))
                .await?
                .into_inner();
        Ok(ActiveProfile {
            id: response.id,
            name: response.profile_name,
            username: response.username,
        })
    }

    /// Management URL stored on a profile.
    pub async fn get_profile_config(
        &mut self,
        username: &str,
        profile_name: &str,
    ) -> Result<String, MeshAgentRpcError> {
        let response = with_timeout(self.inner.get_config(Request::new(GetConfigRequest {
            username: username.to_owned(),
            profile_name: profile_name.to_owned(),
        })))
        .await?
        .into_inner();
        Ok(response.management_url)
    }

    /// Creates a profile (display name) then writes self-hosted management URL.
    pub async fn add_profile(
        &mut self,
        username: &str,
        name: &str,
        management_url: &str,
    ) -> Result<String, MeshAgentRpcError> {
        let added = with_timeout(self.inner.add_profile(Request::new(AddProfileRequest {
            username: username.to_owned(),
            profile_name: name.to_owned(),
        })))
        .await?
        .into_inner();
        self.set_profile_url(username, name, management_url).await?;
        Ok(added.id)
    }

    /// Updates display name and/or management URL. `handle` is id or unique name.
    pub async fn update_profile(
        &mut self,
        username: &str,
        handle: &str,
        new_name: Option<&str>,
        management_url: Option<&str>,
    ) -> Result<(), MeshAgentRpcError> {
        if let Some(name) = new_name.filter(|n| !n.trim().is_empty() && *n != handle) {
            with_timeout(self.inner.rename_profile(Request::new(RenameProfileRequest {
                username: username.to_owned(),
                handle: handle.to_owned(),
                new_profile_name: name.to_owned(),
            })))
            .await?;
        }
        if let Some(url) = management_url.filter(|u| !u.trim().is_empty()) {
            let name = new_name.filter(|n| !n.trim().is_empty()).unwrap_or(handle);
            self.set_profile_url(username, name, url).await?;
        }
        Ok(())
    }

    pub async fn remove_profile(
        &mut self,
        username: &str,
        handle: &str,
    ) -> Result<(), MeshAgentRpcError> {
        with_timeout(self.inner.remove_profile(Request::new(RemoveProfileRequest {
            username: username.to_owned(),
            profile_name: handle.to_owned(),
        })))
        .await?;
        Ok(())
    }

    /// Switches active profile. Caller should Down first when connected.
    pub async fn switch_profile(
        &mut self,
        username: &str,
        handle: &str,
    ) -> Result<String, MeshAgentRpcError> {
        let response =
            with_timeout(self.inner.switch_profile(Request::new(SwitchProfileRequest {
                profile_name: Some(handle.to_owned()),
                username: Some(username.to_owned()),
            })))
            .await?
            .into_inner();
        Ok(response.id)
    }

    /// Routed networks as address strings (CIDR + domains).
    pub async fn list_network_addresses(&mut self) -> Result<Vec<String>, MeshAgentRpcError> {
        let response = with_timeout(self.inner.list_networks(Request::new(ListNetworksRequest {})))
            .await?
            .into_inner();
        let mut addrs = Vec::new();
        for net in response.routes {
            if !net.range.trim().is_empty() && !net.range.contains(char::is_whitespace) {
                addrs.push(net.range);
            }
            for d in net.domains {
                if !d.trim().is_empty() {
                    addrs.push(d);
                }
            }
        }
        addrs.sort();
        addrs.dedup();
        Ok(addrs)
    }

    pub async fn set_profile_url(
        &mut self,
        username: &str,
        profile_name: &str,
        management_url: &str,
    ) -> Result<(), MeshAgentRpcError> {
        with_timeout(self.inner.set_config(Request::new(SetConfigRequest {
            username: username.to_owned(),
            profile_name: profile_name.to_owned(),
            management_url: management_url.to_owned(),
            ..Default::default()
        })))
        .await?;
        Ok(())
    }
}

/// Result of `RequestExtendAuthSession`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ExtendSessionStart {
    pub user_code: String,
    pub device_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_in: i64,
}

/// One daemon profile row.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ProfileInfo {
    pub id: String,
    pub name: String,
    pub is_active: bool,
}

/// Active profile as reported by the daemon.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ActiveProfile {
    pub id: String,
    pub name: String,
    pub username: String,
}

#[derive(Debug)]
pub struct StatusStream {
    inner: tonic::Streaming<StatusResponse>,
}

impl StatusStream {
    pub async fn next(&mut self) -> Option<Result<StatusFrame, MeshAgentRpcError>> {
        match self.inner.message().await {
            Ok(Some(response)) => Some(Ok(status_frame_from_response(&response))),
            Ok(None) => None,
            Err(status) => Some(Err(MeshAgentRpcError::Rpc(status))),
        }
    }
}

async fn connect_channel(addr: &DaemonAddr) -> Result<Channel, MeshAgentRpcError> {
    match addr {
        DaemonAddr::Unix(path) => {
            #[cfg(unix)]
            {
                let path = path.clone();
                let display = addr.display();
                Endpoint::try_from("http://[::]:50051")
                    .map_err(|error| MeshAgentRpcError::Connect {
                        addr: display.clone(),
                        source: Box::new(error),
                    })?
                    .connect_timeout(Duration::from_secs(5))
                    .connect_with_connector(tower::service_fn(move |_| {
                        let path = path.clone();
                        async move {
                            let stream = tokio::net::UnixStream::connect(path).await?;
                            Ok::<_, std::io::Error>(hyper_util::rt::TokioIo::new(stream))
                        }
                    }))
                    .await
                    .map_err(|error| MeshAgentRpcError::Connect {
                        addr: display,
                        source: Box::new(error),
                    })
            }
            #[cfg(not(unix))]
            {
                let _ = path;
                Err(MeshAgentRpcError::InvalidAddress(
                    "unix socket addresses are only supported on unix".to_owned(),
                ))
            }
        }
        DaemonAddr::Tcp(endpoint) => {
            let uri = format!("http://{endpoint}");
            Endpoint::from_shared(uri)
                .map_err(|error| MeshAgentRpcError::Connect {
                    addr: addr.display(),
                    source: Box::new(error),
                })?
                .connect_timeout(Duration::from_secs(5))
                .connect()
                .await
                .map_err(|error| MeshAgentRpcError::Connect {
                    addr: addr.display(),
                    source: Box::new(error),
                })
        }
        DaemonAddr::NamedPipe { .. } => {
            #[cfg(windows)]
            {
                connect_windows_pipe(addr).await
            }
            #[cfg(not(windows))]
            {
                Err(MeshAgentRpcError::InvalidAddress(
                    "npipe addresses are only supported on Windows".to_owned(),
                ))
            }
        }
    }
}

#[cfg(windows)]
async fn connect_windows_pipe(addr: &DaemonAddr) -> Result<Channel, MeshAgentRpcError> {
    use std::sync::Arc;
    let Some(pipe_path) = addr.pipe_dial_path() else {
        return Err(MeshAgentRpcError::InvalidAddress("not a named pipe address".to_owned()));
    };
    let pipe_path = Arc::new(pipe_path);
    let display = addr.display();
    Endpoint::try_from("http://[::]:50051")
        .map_err(|error| MeshAgentRpcError::Connect {
            addr: display.clone(),
            source: Box::new(error),
        })?
        .connect_timeout(Duration::from_secs(5))
        .connect_with_connector(tower::service_fn(move |_| {
            let pipe_path = Arc::clone(&pipe_path);
            async move {
                open_windows_pipe(pipe_path.as_str())
                    .await
                    .map(hyper_util::rt::TokioIo::new)
            }
        }))
        .await
        .map_err(|error| MeshAgentRpcError::Connect { addr: display, source: Box::new(error) })
}

#[cfg(windows)]
async fn open_windows_pipe(
    path: &str,
) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    const ERROR_PIPE_BUSY: i32 = 231;
    const RETRY_INTERVAL: Duration = Duration::from_millis(50);
    const RETRY_BUDGET: Duration = Duration::from_secs(5);

    let deadline = tokio::time::Instant::now() + RETRY_BUDGET;
    loop {
        match tokio::net::windows::named_pipe::ClientOptions::new().open(path) {
            Ok(client) => return Ok(client),
            Err(error)
                if error.raw_os_error() == Some(ERROR_PIPE_BUSY)
                    && tokio::time::Instant::now() < deadline =>
            {
                tokio::time::sleep(RETRY_INTERVAL).await;
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MeshAgentRpc;

    #[tokio::test]
    #[ignore = "needs a running NetBird daemon listening on the default socket"]
    async fn the_local_daemon_answers_the_status_stream() {
        let mut client = MeshAgentRpc::connect_str("unix:///var/run/netbird.sock")
            .await
            .expect("connect to the local daemon");
        let mut stream = client.subscribe_status().await.expect("subscribe to status");
        let first = stream.next().await.expect("the stream ended before its first frame");
        let frame = first.expect("first frame failed");

        assert!(frame.view.daemon_version.is_some(), "first frame carries the daemon version");
        assert!(frame.networks_revision.is_some(), "first frame carries the networks revision");
    }
}
