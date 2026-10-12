use serde::Serialize;
use warp_ztna_err::prelude::*;
use warp_ztna_nbrpc::prelude::MeshAgentRpcError;

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
}
