use serde::Serialize;
use warp_ztna_err::prelude::*;
use warp_ztna_nbrpc::prelude::MeshAgentRpcError;

use crate::api_oidc::ApiSessionError;

/// Tauri invoke error payload. Only `code` + `message` cross the IPC boundary.
#[derive(Debug, Serialize)]
pub(crate) struct CommandError {
    pub code: String,
    pub message: String,
}

pub(crate) const PENDING_APPROVAL_CODE: &str = "biz.pending_approval";

pub(crate) const PEER_CONFLICT_CODE: &str = "biz.peer_ownership_conflict";

pub(crate) const SSH_CERT_NOT_READY_CODE: &str = "biz.ssh_cert_not_ready";

pub(crate) const SSH_NO_LOCAL_KEY_CODE: &str = "biz.ssh_no_local_key";

pub(crate) const SSH_PRINCIPAL_MISMATCH_CODE: &str = "biz.ssh_principal_mismatch";

pub(crate) const SSH_NO_REGISTERED_HOST_CODE: &str = "biz.ssh_no_registered_host";

pub(crate) const SSH_UNSUPPORTED_ACCOUNT_CODE: &str = "biz.ssh_login_unsupported_account";

pub(crate) const SSH_CLIENT_MISSING_CODE: &str = "biz.ssh_client_missing";

pub(crate) const SSH_LOCAL_CONFIG_CODE: &str = "biz.ssh_local_config_failed";

pub(crate) const API_LOGIN_REQUIRED_CODE: &str = "biz.api_login_required";

pub(crate) const API_LOGIN_IN_PROGRESS_CODE: &str = "biz.api_login_in_progress";

pub(crate) const API_LOGIN_TIMEOUT_CODE: &str = "biz.api_login_timeout";

pub(crate) const API_LOGIN_CANCELLED_CODE: &str = "biz.api_login_cancelled";

pub(crate) const API_LOGIN_BROWSER_FAILED_CODE: &str = "biz.api_login_browser_failed";

impl CommandError {
    pub(crate) fn coded(code: &str, message: impl Into<String>) -> Self {
        let message = message.into();
        log::warn!("desktop command failed: {code}: {message}");
        Self { code: code.to_owned(), message }
    }

    pub(crate) fn pending_approval(error: &MeshAgentRpcError, context: &str) -> Self {
        log::error!("desktop command failed: {context}\n -> Caused by:\n    {error}");
        Self {
            code: PENDING_APPROVAL_CODE.to_owned(),
            message: "account is waiting for administrator approval".to_owned(),
        }
    }

    pub(crate) fn peer_conflict(error: &MeshAgentRpcError, context: &str) -> Self {
        log::error!("desktop command failed: {context}\n -> Caused by:\n    {error}");
        Self {
            code: PEER_CONFLICT_CODE.to_owned(),
            message: "this device is already registered by a different account".to_owned(),
        }
    }

    pub(crate) fn api_session(error: ApiSessionError) -> Self {
        match error {
            ApiSessionError::Cancelled => {
                Self::coded(API_LOGIN_CANCELLED_CODE, "API sign-in cancelled")
            }
            ApiSessionError::TimedOut => {
                Self::coded(API_LOGIN_TIMEOUT_CODE, "waiting for the browser sign-in timed out")
            }
            ApiSessionError::Busy => Self::coded(
                API_LOGIN_IN_PROGRESS_CODE,
                "another API sign-in is already in progress",
            ),
            ApiSessionError::BrowserFailed(error) => {
                Self::coded(API_LOGIN_BROWSER_FAILED_CODE, root_message(&error))
            }
            ApiSessionError::Failed(error) => Self::from(*error),
        }
    }
}

pub(crate) fn classify(error: &MeshAgentRpcError, context: &str) -> Option<CommandError> {
    if error.is_pending_approval() {
        return Some(CommandError::pending_approval(error, context));
    }
    if error.is_peer_ownership_conflict() {
        return Some(CommandError::peer_conflict(error, context));
    }
    None
}

pub(crate) type CmdResult<T> = std::result::Result<T, CommandError>;

pub(crate) fn root_message(err: &Error) -> String {
    err.root_cause()
        .map(ToString::to_string)
        .filter(|message| !message.is_empty())
        .or_else(|| err.detail().clone().filter(|detail| !detail.is_empty()))
        .unwrap_or_else(|| err.to_string())
}

impl From<Error> for CommandError {
    fn from(err: Error) -> Self {
        log::error!("desktop command failed: {}", display_chain(&err));
        Self { code: err.reason().code().to_owned(), message: root_message(&err) }
    }
}

/// Lift a daemon RPC failure into an operation error.
pub(crate) fn lift_mesh<T>(
    result: std::result::Result<T, MeshAgentRpcError>,
    context: impl Into<ImmutStr>,
) -> Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(MeshAgentRpcError::InvalidAddress(message)) => {
            e_explain(Reason::InvalidArgument, message)
        }
        Err(error @ (MeshAgentRpcError::Connect { .. } | MeshAgentRpcError::NotReachable)) => {
            Err(because(Reason::Unavailable, "connect netbird daemon", error))
        }
        Err(error @ MeshAgentRpcError::UnsupportedVersion(_)) => {
            Err(because(Reason::External, "unsupported netbird daemon version", error))
        }
        Err(error) => Err(because(Reason::External, context, error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_error_uses_reason_code_and_detail() {
        let err = CommandError::from(explain(Reason::Unavailable, "connect netbird daemon"));
        assert_eq!(err.code, "sys.unavailable");
        assert_eq!(err.message, "connect netbird daemon");
    }

    #[test]
    fn pending_approval_uses_stable_ipc_code() {
        let source = MeshAgentRpcError::Message("pending approval".to_owned());

        let err = CommandError::pending_approval(&source, "wait sso");

        assert_eq!(err.code, PENDING_APPROVAL_CODE);
        assert_eq!(err.message, "account is waiting for administrator approval");
    }

    #[test]
    fn peer_conflict_uses_stable_ipc_code() {
        let source = MeshAgentRpcError::Message("peer is already registered".to_owned());

        let err = CommandError::peer_conflict(&source, "wait sso");

        assert_eq!(err.code, PEER_CONFLICT_CODE);
        assert_eq!(err.message, "this device is already registered by a different account");
    }

    #[test]
    fn command_error_prefers_root_cause_over_context_label() {
        let cause = MeshAgentRpcError::Message(
            "peer is already registered by a different User or a Setup Key".to_owned(),
        );

        let err = CommandError::from(because(Reason::External, "wait sso", cause));

        assert_eq!(err.code, "sys.external");
        assert_eq!(err.message, "peer is already registered by a different User or a Setup Key");
    }

    #[test]
    fn lift_mesh_maps_not_reachable_to_unavailable() {
        let err =
            lift_mesh(Err::<(), _>(MeshAgentRpcError::NotReachable), "agent status").unwrap_err();
        assert_eq!(err.reason(), &Reason::Unavailable);
        assert_eq!(err.detail().as_deref(), Some("connect netbird daemon"));
    }

    #[test]
    fn lift_mesh_maps_invalid_address() {
        let err =
            lift_mesh(Err::<(), _>(MeshAgentRpcError::InvalidAddress("bad".into())), "connect")
                .unwrap_err();
        assert_eq!(err.reason(), &Reason::InvalidArgument);
        assert_eq!(err.detail().as_deref(), Some("bad"));
    }

    #[test]
    fn lift_mesh_maps_rpc_message_to_external() {
        let err =
            lift_mesh(Err::<(), _>(MeshAgentRpcError::Message("boom".into())), "agent status")
                .unwrap_err();
        assert_eq!(err.reason(), &Reason::External);
        assert_eq!(err.detail().as_deref(), Some("agent status"));
    }

    #[test]
    fn lift_mesh_maps_unsupported_version() {
        let err = lift_mesh(
            Err::<(), _>(MeshAgentRpcError::UnsupportedVersion("0.75.0".into())),
            "agent status",
        )
        .unwrap_err();
        assert_eq!(err.reason(), &Reason::External);
        assert_eq!(err.detail().as_deref(), Some("unsupported netbird daemon version"));
    }

    #[test]
    fn api_session_errors_use_stable_ipc_codes() {
        let cancelled = CommandError::api_session(ApiSessionError::Cancelled);
        assert_eq!(cancelled.code, API_LOGIN_CANCELLED_CODE);

        let busy = CommandError::api_session(ApiSessionError::Busy);
        assert_eq!(busy.code, API_LOGIN_IN_PROGRESS_CODE);

        let failed = CommandError::api_session(ApiSessionError::Failed(Box::new(explain(
            Reason::Unavailable,
            "reach the OIDC issuer",
        ))));
        assert_eq!(failed.code, "sys.unavailable");
        assert_eq!(failed.message, "reach the OIDC issuer");
    }

    #[test]
    fn api_session_timeout_has_a_readable_message() {
        let err = CommandError::api_session(ApiSessionError::TimedOut);
        assert_eq!(err.code, API_LOGIN_TIMEOUT_CODE);
        assert_eq!(err.message, "waiting for the browser sign-in timed out");
        assert!(!err.message.contains("deadline"));
    }

    #[test]
    fn api_session_browser_failure_keeps_the_root_cause() {
        let cause = MeshAgentRpcError::Message("no browser available".to_owned());
        let err = CommandError::api_session(ApiSessionError::BrowserFailed(because(
            Reason::External,
            "open OIDC authorization",
            cause,
        )));
        assert_eq!(err.code, API_LOGIN_BROWSER_FAILED_CODE);
        assert_eq!(err.message, "no browser available");
    }
}
