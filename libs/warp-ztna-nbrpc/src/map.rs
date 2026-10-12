use serde::{Deserialize, Serialize};

use crate::pb::{LoginResponse, StatusResponse};

/// High-level connection state for Desktop UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Unknown,
    Disconnected,
    Connecting,
    NeedsLogin,
    Connected,
    Error,
}

/// UI-facing status snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusView {
    pub state: ConnectionState,
    pub peer_ip: Option<String>,
    pub hostname: Option<String>,
    pub detail: Option<String>,
    pub daemon_version: Option<String>,
    /// Unix seconds UTC; none when login expiration is unset.
    pub session_expires_at: Option<i64>,
    /// Routed prefixes / domains currently known on the local peer.
    #[serde(default)]
    pub routes: Vec<String>,
    /// Control plane reachable and, when peers are known, at least one peer connected.
    #[serde(default)]
    pub link_ok: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusFrame {
    pub view: StatusView,
    pub networks_revision: Option<u64>,
}

#[must_use]
pub fn status_frame_from_response(response: &StatusResponse) -> StatusFrame {
    StatusFrame {
        networks_revision: response.full_status.as_ref().map(|full| full.networks_revision),
        view: status_view_from_response(response),
    }
}

/// Diagnostics payload for the Desktop diagnostics view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsView {
    pub daemon_reachable: bool,
    pub daemon_addr: String,
    pub daemon_version: String,
    pub platform: String,
    pub status_detail: Option<String>,
}

/// Result of a `Login` RPC before `Up`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoginOutcome {
    /// Credentials accepted; caller should `Up`.
    Ready,
    /// Browser SSO required.
    SsoRequired { user_code: String, verification_uri: String, verification_uri_complete: String },
}

impl LoginOutcome {
    #[must_use]
    pub fn from_response(response: LoginResponse) -> Self {
        if response.needs_sso_login {
            Self::SsoRequired {
                user_code: response.user_code,
                verification_uri: response.verification_uri,
                verification_uri_complete: response.verification_uri_complete,
            }
        } else {
            Self::Ready
        }
    }
}

/// Maps a `NetBird` `StatusResponse` into a compact UI view.
#[must_use]
pub fn status_view_from_response(response: &StatusResponse) -> StatusView {
    let daemon_version = non_empty(&response.daemon_version);
    let status_text = response.status.trim();
    let full = response.full_status.as_ref();

    let local = full.and_then(|f| f.local_peer_state.as_ref());
    let peer_ip =
        local.map(|local| local.ip.trim()).filter(|ip| !ip.is_empty()).map(ToOwned::to_owned);
    let hostname =
        local.map(|local| local.fqdn.trim()).filter(|name| !name.is_empty()).map(ToOwned::to_owned);
    let mut routes: Vec<String> = local
        .map(|local| local.networks.iter().filter(|n| !n.trim().is_empty()).cloned().collect())
        .unwrap_or_default();
    routes.sort();
    routes.dedup();
    let session_expires_at = response.session_expires_at.as_ref().map(|ts| ts.seconds);

    let management_connected =
        full.and_then(|f| f.management_state.as_ref()).is_some_and(|m| m.connected);
    let signal_connected = full.and_then(|f| f.signal_state.as_ref()).is_some_and(|s| s.connected);
    let peers_connected = full.is_some_and(|full| {
        full.peers.is_empty()
            || full.peers.iter().any(|peer| peer.conn_status.eq_ignore_ascii_case("connected"))
    });
    let link_ok = management_connected && signal_connected && peers_connected;

    let lowered = status_text.to_ascii_lowercase();
    let compact: String = lowered.chars().filter(char::is_ascii_alphanumeric).collect();
    let state = if compact.contains("needslogin") {
        ConnectionState::NeedsLogin
    } else if lowered.contains("error") || lowered.contains("failed") {
        ConnectionState::Error
    } else if management_connected || signal_connected || lowered.contains("connected") {
        ConnectionState::Connected
    } else if lowered.contains("connecting") || lowered.contains("login") {
        ConnectionState::Connecting
    } else if lowered.is_empty() || lowered.contains("idle") || lowered.contains("disconnect") {
        ConnectionState::Disconnected
    } else {
        ConnectionState::Unknown
    };

    let detail = if status_text.is_empty() { None } else { Some(status_text.to_owned()) };

    StatusView {
        state,
        peer_ip,
        hostname,
        detail,
        daemon_version,
        session_expires_at,
        routes,
        link_ok,
    }
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_owned()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pb::{
        FullStatus, LocalPeerState, ManagementState, PeerState, SignalState, StatusResponse,
    };

    #[test]
    fn maps_connected() {
        let response = StatusResponse {
            status: "Connected".to_owned(),
            full_status: Some(FullStatus {
                management_state: Some(ManagementState { connected: true, ..Default::default() }),
                signal_state: Some(SignalState { connected: true, ..Default::default() }),
                local_peer_state: Some(LocalPeerState {
                    ip: "100.64.0.2".to_owned(),
                    networks: vec!["192.0.2.0/24".to_owned()],
                    ..Default::default()
                }),
                peers: vec![
                    PeerState {
                        networks: vec![
                            "192.0.2.122/32".to_owned(),
                            "*.example.com".to_owned(),
                            "192.0.2.122/32".to_owned(),
                            " 192.0.2.105/32 ".to_owned(),
                        ],
                        conn_status: "Connected".to_owned(),
                        ..Default::default()
                    },
                    PeerState { conn_status: "Connected".to_owned(), ..Default::default() },
                ],
                ..Default::default()
            }),
            daemon_version: "0.76.2".to_owned(),
            ..Default::default()
        };
        let view = status_view_from_response(&response);
        assert_eq!(view.state, ConnectionState::Connected);
        assert_eq!(view.peer_ip.as_deref(), Some("100.64.0.2"));
        assert_eq!(view.routes, vec!["192.0.2.0/24".to_owned()]);
        assert!(view.link_ok);
    }

    #[test]
    fn maps_needs_login_before_connecting() {
        for status in ["NeedsLogin", "needs_login", "needs login"] {
            let response = StatusResponse { status: status.to_owned(), ..Default::default() };
            let view = status_view_from_response(&response);
            assert_eq!(view.state, ConnectionState::NeedsLogin, "{status}");
            assert!(!view.link_ok);
        }
    }

    #[test]
    fn maps_missing_full_status_to_empty_view() {
        let view = status_view_from_response(&StatusResponse::default());
        assert!(view.routes.is_empty());
        assert!(!view.link_ok);
    }

    #[test]
    fn status_frames_carry_the_networks_revision() {
        let with_revision = StatusResponse {
            status: "Connected".to_owned(),
            full_status: Some(FullStatus { networks_revision: 22, ..Default::default() }),
            ..Default::default()
        };
        assert_eq!(status_frame_from_response(&with_revision).networks_revision, Some(22));
        assert_eq!(
            status_frame_from_response(&with_revision).view.state,
            ConnectionState::Connected
        );

        assert_eq!(status_frame_from_response(&StatusResponse::default()).networks_revision, None);
    }

    #[test]
    fn link_is_degraded_without_the_control_plane_or_a_connected_peer() {
        let base = FullStatus {
            management_state: Some(ManagementState { connected: true, ..Default::default() }),
            signal_state: Some(SignalState { connected: true, ..Default::default() }),
            ..Default::default()
        };
        let without_peers = StatusResponse {
            status: "Connected".to_owned(),
            full_status: Some(base.clone()),
            ..Default::default()
        };
        assert!(status_view_from_response(&without_peers).link_ok);

        let stalled_peer = StatusResponse {
            status: "Connected".to_owned(),
            full_status: Some(FullStatus {
                peers: vec![PeerState {
                    conn_status: "Connecting".to_owned(),
                    ..Default::default()
                }],
                ..base.clone()
            }),
            ..Default::default()
        };
        assert!(!status_view_from_response(&stalled_peer).link_ok);

        let signal_down = StatusResponse {
            status: "Connected".to_owned(),
            full_status: Some(FullStatus {
                signal_state: Some(SignalState { connected: false, ..Default::default() }),
                ..base
            }),
            ..Default::default()
        };
        assert!(!status_view_from_response(&signal_down).link_ok);
    }

    #[test]
    fn login_sso_outcome() {
        let outcome = LoginOutcome::from_response(LoginResponse {
            needs_sso_login: true,
            user_code: "ABCD".to_owned(),
            verification_uri: "https://example.com".to_owned(),
            verification_uri_complete: "https://example.com/c".to_owned(),
        });
        assert!(matches!(outcome, LoginOutcome::SsoRequired { .. }));
    }
}
