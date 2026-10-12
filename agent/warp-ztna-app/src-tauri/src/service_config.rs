use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const CONFIG_FILE: &str = "config.json";

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServiceConfig {
    pub api_url: String,
    pub portal_url: String,
}

fn config_path(dir: &Path) -> PathBuf {
    dir.join(CONFIG_FILE)
}

pub fn load(dir: &Path) -> ServiceConfig {
    let path = config_path(dir);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return ServiceConfig::default();
        }
        Err(error) => {
            log::warn!("service config: cannot read {}: {error}", path.display());
            return ServiceConfig::default();
        }
    };
    match serde_json::from_str(&raw) {
        Ok(config) => config,
        Err(error) => {
            log::warn!("service config: ignoring malformed {}: {error}", path.display());
            ServiceConfig::default()
        }
    }
}

pub fn save(dir: &Path, config: &ServiceConfig) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = config_path(dir);
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_vec_pretty(config).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &path)
}

pub fn endpoint(env: Option<&str>, stored: Option<&str>, default: &str) -> String {
    for value in [env, stored].into_iter().flatten() {
        let value = value.trim();
        if !value.is_empty() {
            return value.to_owned();
        }
    }
    default.to_owned()
}

pub fn validate_endpoint(value: &str) -> Result<(), &'static str> {
    let value = value.trim();
    if value.is_empty() || value.starts_with("https://") || value.starts_with("http://") {
        Ok(())
    } else {
        Err("address must be http(s)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_prefers_env_then_stored_then_default() {
        assert_eq!(
            endpoint(Some("https://env"), Some("https://stored"), "https://def"),
            "https://env"
        );
        assert_eq!(endpoint(None, Some("https://stored"), "https://def"), "https://stored");
        assert_eq!(endpoint(None, None, "https://def"), "https://def");
        assert_eq!(endpoint(Some("  "), Some("  "), "https://def"), "https://def");
        assert_eq!(endpoint(Some("  https://env  "), None, "https://def"), "https://env");
    }

    #[test]
    fn validate_accepts_http_and_empty_only() {
        assert!(validate_endpoint("").is_ok());
        assert!(validate_endpoint("   ").is_ok());
        assert!(validate_endpoint("https://api.example").is_ok());
        assert!(validate_endpoint("http://localhost:8080").is_ok());
        assert!(validate_endpoint("ftp://api.example").is_err());
        assert!(validate_endpoint("api.example").is_err());
    }

    #[test]
    fn save_then_load_roundtrips_on_disk() {
        let dir =
            std::env::temp_dir().join(format!("warp-ztna-service-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = ServiceConfig {
            api_url: "https://api.example".into(),
            portal_url: "https://portal.example".into(),
        };
        save(&dir, &config).expect("save config");
        let loaded = load(&dir);
        assert_eq!(loaded.api_url, "https://api.example");
        assert_eq!(loaded.portal_url, "https://portal.example");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_tolerates_missing_and_malformed_files() {
        let dir = std::env::temp_dir()
            .join(format!("warp-ztna-service-config-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(load(&dir).api_url.is_empty());
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(config_path(&dir), "not json").expect("write");
        assert!(load(&dir).api_url.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
