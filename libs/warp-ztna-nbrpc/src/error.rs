use std::time::Duration;

use thiserror::Error;

/// Errors from connecting to or calling the `NetBird` daemon.
#[derive(Debug, Error)]
pub enum MeshAgentRpcError {
    #[error("invalid NetBird daemon address: {0}")]
    InvalidAddress(String),
    #[error("failed to connect to NetBird daemon at {addr}: {source}")]
    Connect {
        addr: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("NetBird daemon RPC failed: {0}")]
    Rpc(#[from] tonic::Status),
    #[error("NetBird daemon version is unsupported: {0}")]
    UnsupportedVersion(String),
    #[error("NetBird daemon is not reachable")]
    NotReachable,
    #[error("NetBird daemon RPC did not answer within {0:?}")]
    Timeout(Duration),
    #[error("{0}")]
    Message(String),
}

impl MeshAgentRpcError {
    /// Returns whether the daemon rejected peer enrollment because the user
    /// is waiting for management approval.
    #[must_use]
    pub fn is_pending_approval(&self) -> bool {
        match self {
            Self::Rpc(status) => {
                status.code() == tonic::Code::PermissionDenied
                    && status.message().to_ascii_lowercase().contains("pending approval")
            }
            _ => false,
        }
    }

    #[must_use]
    pub fn is_unimplemented(&self) -> bool {
        matches!(self, Self::Rpc(status) if status.code() == tonic::Code::Unimplemented)
    }

    #[must_use]
    pub fn is_peer_ownership_conflict(&self) -> bool {
        match self {
            Self::Rpc(status) => {
                status.code() == tonic::Code::PermissionDenied
                    && status
                        .message()
                        .to_ascii_lowercase()
                        .contains("already registered by a different")
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tonic::{Code, Status};

    use super::MeshAgentRpcError;

    #[test]
    fn identifies_unimplemented_rpc() {
        let missing = MeshAgentRpcError::Rpc(Status::new(
            Code::Unimplemented,
            "unknown method SubscribeStatus",
        ));
        let other = MeshAgentRpcError::Rpc(Status::new(Code::Unavailable, "subscribe status"));

        assert!(missing.is_unimplemented());
        assert!(!other.is_unimplemented());
        assert!(!MeshAgentRpcError::Timeout(Duration::from_secs(1)).is_unimplemented());
    }

    #[test]
    fn identifies_pending_approval_permission_denied() {
        let error = MeshAgentRpcError::Rpc(Status::new(
            Code::PermissionDenied,
            "user pending approval cannot add peers",
        ));

        assert!(error.is_pending_approval());
    }

    #[test]
    fn does_not_identify_other_rpc_errors_as_pending_approval() {
        let wrong_code = MeshAgentRpcError::Rpc(Status::new(
            Code::Unauthenticated,
            "user pending approval cannot add peers",
        ));
        let wrong_message = MeshAgentRpcError::Rpc(Status::new(
            Code::PermissionDenied,
            "user is not allowed to add peers",
        ));
        let non_rpc = MeshAgentRpcError::Message("user pending approval".to_owned());

        assert!(!wrong_code.is_pending_approval());
        assert!(!wrong_message.is_pending_approval());
        assert!(!non_rpc.is_pending_approval());
    }

    #[test]
    fn identifies_peer_ownership_conflict_permission_denied() {
        let error = MeshAgentRpcError::Rpc(Status::new(
            Code::PermissionDenied,
            "peer is already registered by a different User or a Setup Key",
        ));

        assert!(error.is_peer_ownership_conflict());
        assert!(!error.is_pending_approval());
    }

    #[test]
    fn does_not_identify_other_rpc_errors_as_peer_ownership_conflict() {
        let wrong_code = MeshAgentRpcError::Rpc(Status::new(
            Code::Unauthenticated,
            "peer is already registered by a different User or a Setup Key",
        ));
        let wrong_message = MeshAgentRpcError::Rpc(Status::new(
            Code::PermissionDenied,
            "user pending approval cannot add peers",
        ));
        let non_rpc =
            MeshAgentRpcError::Message("peer is already registered by a different User".to_owned());

        assert!(!wrong_code.is_peer_ownership_conflict());
        assert!(!wrong_message.is_peer_ownership_conflict());
        assert!(!non_rpc.is_peer_ownership_conflict());
    }
}
