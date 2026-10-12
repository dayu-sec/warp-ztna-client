#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

//! Local mesh **agent RPC** client (`warp-ztna-nbrpc`).
//!
//! Talks to the on-device mesh agent (`NetBird` daemon) over gRPC (`DaemonService`).
//! Here **agent** means the privileged local mesh process, not a Warp-owned
//! daemon. Integration style matches
//! [netbird-tui](https://github.com/n0pashkov/netbird-tui).
//!
//! ```rust
//! use warp_ztna_nbrpc::prelude::*;
//! ```

mod addr;
mod backoff;
mod error;
mod map;
mod rpc;

/// Generated protobuf / gRPC types for `daemon.DaemonService` (not in [`prelude`]).
pub mod pb {
    #![allow(clippy::all, clippy::pedantic)]

    tonic::include_proto!("daemon");
}

mod version {
    /// Supported `NetBird` daemon minor line for this crate (gRPC proto pinned from v0.76.2).
    pub const SUPPORTED_NETBIRD_MINOR: &str = "0.80";

    /// Returns true when `version` looks like a supported 0.80.x release string.
    #[must_use]
    pub fn is_supported_daemon_version(version: &str) -> bool {
        let trimmed = version.trim().trim_start_matches('v');
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("unknown") {
            return false;
        }
        let mut parts = trimmed.split(|c: char| !c.is_ascii_digit());
        let major = parts.next().and_then(|p| p.parse::<u32>().ok());
        let minor = parts.next().and_then(|p| p.parse::<u32>().ok());
        matches!((major, minor), (Some(0), Some(80)))
    }
}

/// Public import path for consumers.
pub mod prelude {
    pub use crate::addr::{DaemonAddr, default_daemon_addr, resolve_daemon_addr};
    pub use crate::backoff::ReconnectBackoff;
    pub use crate::error::MeshAgentRpcError;
    pub use crate::map::{ConnectionState, DiagnosticsView, LoginOutcome, StatusFrame, StatusView};
    pub use crate::rpc::MeshAgentRpc;
    pub use crate::rpc::{ActiveProfile, ExtendSessionStart, ProfileInfo, StatusStream};
    pub use crate::version::{SUPPORTED_NETBIRD_MINOR, is_supported_daemon_version};
}

#[cfg(test)]
mod tests {
    use crate::prelude::is_supported_daemon_version;

    #[test]
    fn supports_080_line() {
        assert!(is_supported_daemon_version("0.80.2"));
        assert!(is_supported_daemon_version("v0.80.0"));
        assert!(!is_supported_daemon_version("0.79.0"));
        assert!(!is_supported_daemon_version("0.81.0"));
        assert!(!is_supported_daemon_version("unknown"));
    }
}
