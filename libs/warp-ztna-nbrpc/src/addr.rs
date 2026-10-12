use std::path::{Path, PathBuf};

use crate::error::MeshAgentRpcError;

/// Parsed `NetBird` daemon listen / dial address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonAddr {
    /// `unix:///path/to.sock`
    Unix(PathBuf),
    /// Windows named pipe path (with or without `\\.\pipe\` prefix).
    NamedPipe(String),
    /// Explicit TCP (discouraged in production on `NetBird` ≥ 0.76).
    Tcp(String),
}

impl DaemonAddr {
    /// Parses `unix://…`, `npipe://…`, `tcp://…`, or a bare filesystem path (treated as unix).
    ///
    /// # Errors
    ///
    /// Returns [`MeshAgentRpcError::InvalidAddress`] when the scheme is missing or unsupported.
    pub fn parse(input: &str) -> Result<Self, MeshAgentRpcError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(MeshAgentRpcError::InvalidAddress("empty address".to_owned()));
        }

        if let Some((scheme, rest)) = trimmed.split_once("://") {
            return match scheme {
                "unix" => Ok(Self::Unix(PathBuf::from(rest))),
                "npipe" => Ok(Self::NamedPipe(normalize_pipe_name(rest))),
                "tcp" => Ok(Self::Tcp(rest.to_owned())),
                other => Err(MeshAgentRpcError::InvalidAddress(format!(
                    "unsupported scheme {other:?} (use unix, npipe, or tcp)"
                ))),
            };
        }

        // Bare path → unix socket (common for local overrides).
        if trimmed.starts_with('/') || trimmed.starts_with('.') {
            return Ok(Self::Unix(PathBuf::from(trimmed)));
        }

        Err(MeshAgentRpcError::InvalidAddress(format!(
            "expected scheme://address, got {trimmed:?}"
        )))
    }

    #[must_use]
    pub fn display(&self) -> String {
        match self {
            Self::Unix(path) => format!("unix://{}", path.display()),
            Self::NamedPipe(name) => format!("npipe://{name}"),
            Self::Tcp(endpoint) => format!("tcp://{endpoint}"),
        }
    }
}

/// Default daemon address for the current OS (aligned with `NetBird` CLI defaults).
#[must_use]
pub fn default_daemon_addr() -> DaemonAddr {
    #[cfg(windows)]
    {
        // NetBird v0.76 uses a protected named pipe; bare name is resolved by the client.
        DaemonAddr::NamedPipe(r"\\.\pipe\netbird".to_owned())
    }
    #[cfg(not(windows))]
    {
        DaemonAddr::Unix(PathBuf::from("/var/run/netbird.sock"))
    }
}

/// Resolves a configured address, applying Unix socket discovery when the default path is missing.
///
/// Mirrors `NetBird` `ResolveUnixDaemonAddr`: if `unix:///var/run/netbird.sock` is absent and exactly
/// one `*.sock` exists under `/var/run/netbird/`, that socket is used.
#[must_use]
pub fn resolve_daemon_addr(configured: &DaemonAddr) -> DaemonAddr {
    match configured {
        DaemonAddr::Unix(path) if path.as_path() == Path::new("/var/run/netbird.sock") => {
            if path.exists() {
                return configured.clone();
            }
            discover_single_unix_socket(Path::new("/var/run/netbird"))
                .unwrap_or_else(|| configured.clone())
        }
        other => other.clone(),
    }
}

fn discover_single_unix_socket(dir: &Path) -> Option<DaemonAddr> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("sock") {
            found.push(path);
        }
    }
    if found.len() == 1 { Some(DaemonAddr::Unix(found.remove(0))) } else { None }
}

fn normalize_pipe_name(rest: &str) -> String {
    if rest.starts_with(r"\\.\pipe\") || rest.starts_with("//./pipe/") {
        rest.replace('/', r"\")
    } else if rest.starts_with(r".\pipe\") {
        format!(r"\\{rest}")
    } else {
        format!(r"\\.\pipe\{}", rest.trim_start_matches('/'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unix() {
        let addr = DaemonAddr::parse("unix:///var/run/netbird.sock").unwrap();
        assert_eq!(addr, DaemonAddr::Unix(PathBuf::from("/var/run/netbird.sock")));
    }

    #[test]
    fn parses_npipe() {
        let addr = DaemonAddr::parse("npipe://netbird").unwrap();
        assert_eq!(addr, DaemonAddr::NamedPipe(r"\\.\pipe\netbird".to_owned()));
    }

    #[test]
    fn rejects_empty() {
        assert!(DaemonAddr::parse("").is_err());
    }
}
