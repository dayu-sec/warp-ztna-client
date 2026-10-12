use std::{env, net::SocketAddr, str::FromStr, time::Duration};

use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("required environment variable {0} is not set")]
    Missing(&'static str),
    #[error("invalid value for {name}: {value}")]
    Invalid { name: &'static str, value: String },
}

/// Process shutdown settings shared by the API and orchestration services.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShutdownConfig {
    pub grace_period: Duration,
}

/// Default application shutdown window, overridable with
/// `WARP_SHUTDOWN_GRACE_PERIOD_SECONDS`.
pub const DEFAULT_SHUTDOWN_GRACE_PERIOD_SECONDS: u64 = 25;

/// Default interval between scheduled Keycloak identity reconciliations.
pub const DEFAULT_IDENTITY_SYNC_INTERVAL_SECONDS: u64 = 14_400;

/// Default interval between `NetBird` audit event polls.
pub const DEFAULT_AUDIT_POLL_INTERVAL_SECONDS: u64 = 900;

/// Default interval for retrying approved access-request JIT applies.
pub const DEFAULT_AUTHZ_APPLY_INTERVAL_SECONDS: u64 = 120;
pub const DEFAULT_AUTHZ_AUTO_APPROVE_MAX_SECONDS: i64 = 604_800;
pub const DEFAULT_SSH_HOST_CERT_TTL_SECONDS: i64 = 15_552_000;
pub const DEFAULT_SSH_MANUAL_CERT_MAX_TTL_SECONDS: i64 = 86_400;

/// Default interval for expiring and revoking JIT policies.
pub const DEFAULT_AUTHZ_RECONCILE_INTERVAL_SECONDS: u64 = 900;

/// Default timeout for one API-to-orch authorization execution.
pub const DEFAULT_AUTHZ_REQUEST_TIMEOUT_SECONDS: u64 = 120;

pub const DEFAULT_ASSET_SYNC_INTERVAL_SECONDS: u64 = 43_200;

/// Default number of days audit events are kept in the local ledger.
pub const DEFAULT_AUDIT_RETENTION_DAYS: i64 = 90;

pub const DEFAULT_ASSET_CENTER_SNAPSHOT_PATH: &str = "/api/v1/ztna/asset-graph/snapshot";

/// Default bind address of the `warp-ztna-ssh-ca` adapter.
pub const DEFAULT_SSH_CA_BIND_ADDR: &str = "127.0.0.1:8091";

/// Default timeout for one call to the SSH certificate adapter.
pub const DEFAULT_SSH_CA_REQUEST_TIMEOUT_SECONDS: u64 = 30;

/// Default provisioner name registered in `step-ca` for the adapter.
pub const DEFAULT_STEP_CA_PROVISIONER_NAME: &str = "warp-ztna-ssh-ca";

/// Default base prefix for `NetBird` entities managed by Warp.
pub const DEFAULT_NETBIRD_MANAGED_PREFIX: &str = "warp-ztna";

pub const IDENTITY_SOURCE_PROVIDER_DINGTALK: &str = "dingtalk";

/// Derived names used to identify and create `NetBird` entities owned by this Warp instance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetBirdManagedNaming {
    pub base_prefix: String,
    pub keycloak_group_prefix: String,
    pub resource_group_prefix: String,
    pub follow_group_prefix: String,
    pub resource_prefix: String,
    pub policy_prefix: String,
    pub peer_group_prefix: String,
    pub process_posture_check_name: String,
}

impl NetBirdManagedNaming {
    pub fn new(value: &str) -> Result<Self, ConfigError> {
        let base_prefix = value;
        if base_prefix.is_empty()
            || base_prefix.len() > 64
            || base_prefix.starts_with('-')
            || base_prefix.ends_with('-')
            || !base_prefix.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return Err(ConfigError::Invalid {
                name: "WARP_NETBIRD_MANAGED_PREFIX",
                value: value.to_owned(),
            });
        }
        Ok(Self {
            base_prefix: base_prefix.to_owned(),
            keycloak_group_prefix: format!("{base_prefix}-kc-"),
            resource_group_prefix: format!("{base_prefix}-rg-"),
            follow_group_prefix: format!("{base_prefix}-fg-"),
            resource_prefix: format!("{base_prefix}-res-"),
            policy_prefix: format!("{base_prefix}-pol-"),
            peer_group_prefix: format!("{base_prefix}-pg-"),
            process_posture_check_name: format!("{base_prefix}-process-check"),
        })
    }
}

fn netbird_managed_naming() -> Result<NetBirdManagedNaming, ConfigError> {
    let raw = match env::var("WARP_NETBIRD_MANAGED_PREFIX") {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => DEFAULT_NETBIRD_MANAGED_PREFIX.to_owned(),
        Err(env::VarError::NotUnicode(value)) => {
            return Err(ConfigError::Invalid {
                name: "WARP_NETBIRD_MANAGED_PREFIX",
                value: value.to_string_lossy().into_owned(),
            });
        }
    };
    NetBirdManagedNaming::new(&raw)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyPortRangeConfig {
    pub start: u16,
    pub end: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessConfig {
    #[serde(default)]
    pub linux_path: String,
    #[serde(default)]
    pub mac_path: String,
    #[serde(default)]
    pub windows_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PostureCheckConfig {
    pub processes: Vec<ProcessConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyTemplateConfig {
    pub id: String,
    pub name: String,
    pub protocol: String,
    #[serde(default)]
    pub ports: Vec<String>,
    #[serde(default)]
    pub port_ranges: Vec<PolicyPortRangeConfig>,
    #[serde(default)]
    pub bidirectional: bool,
    pub action: String,
}

#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub netbird_naming: NetBirdManagedNaming,
    pub bind_addr: SocketAddr,
    pub database_url: SecretString,
    pub oidc_issuer: String,
    pub oidc_audience: String,
    pub allowed_origins: Vec<String>,
    pub local_session_secure: bool,
    pub local_session_ttl_hours: i64,
    pub shutdown: ShutdownConfig,
    pub orch_base_url: String,
    pub authz_request_timeout: Duration,
    pub policy_templates: Vec<PolicyTemplateConfig>,
    pub ssh_ca_base_url: String,
    pub ssh_ca_request_timeout: Duration,
    pub ssh_manual_cert_max_ttl_seconds: i64,
    pub client_pkg_base_url: String,
}

#[derive(Debug, Clone)]
pub struct SshCaConfig {
    pub bind_addr: SocketAddr,
    pub shutdown: ShutdownConfig,
    pub step_ca_url: String,
    pub step_ca_roots_file: String,
    pub step_ca_provisioner_name: String,
    pub step_ca_provisioner_jwk_file: String,
    pub step_ca_provisioner_jwk_password_file: Option<String>,
    pub step_ca_ssh_user_ca_file: Option<String>,
    pub step_ca_ssh_host_ca_file: Option<String>,
    pub step_ca_request_timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct MigrationConfig {
    pub database_url: SecretString,
}

#[derive(Debug, Clone)]
pub struct OrchConfig {
    pub netbird_naming: NetBirdManagedNaming,
    pub bind_addr: SocketAddr,
    pub shutdown: ShutdownConfig,
    pub database_url: SecretString,
    pub netbird_base_url: String,
    pub netbird_api_token: SecretString,
    pub keycloak_admin: Option<KeycloakAdminConfig>,
    pub identity_source_enabled: bool,
    pub identity_source: Option<IdentitySourceConfig>,
    pub auto_approve_initial_enabled: bool,
    pub identity_sync_interval: Duration,
    pub audit_poll_interval: Duration,
    pub authz_apply_interval: Duration,
    pub authz_auto_approve_enabled: bool,
    pub authz_auto_approve_max_seconds: i64,
    pub ssh_host_cert_ttl_seconds: i64,
    pub ssh_manual_cert_max_ttl_seconds: i64,
    pub authz_reconcile_interval: Duration,
    pub asset_sync_interval: Duration,
    pub audit_retention_days: i64,
    pub asset_center_base_url: Option<String>,
    pub asset_center_token: Option<SecretString>,
    pub asset_center_snapshot_path: String,
    pub authz_request_timeout: Duration,
    pub ssh_ca_base_url: String,
    pub ssh_ca_request_timeout: Duration,
    pub posture_check: Option<PostureCheckConfig>,
}

#[derive(Debug, Clone)]
pub struct KeycloakAdminConfig {
    pub base_url: String,
    pub realm: String,
    pub client_id: String,
    pub client_secret: SecretString,
}

#[derive(Debug, Clone)]
pub struct IdentitySourceConfig {
    pub provider: IdentitySourceProvider,
    pub group_root: String,
    pub email_domain: Option<String>,
}

impl IdentitySourceConfig {
    pub fn provider_name(&self) -> &'static str {
        self.provider.name()
    }
}

#[derive(Debug, Clone)]
pub enum IdentitySourceProvider {
    Dingtalk(DingTalkIdentitySourceConfig),
}

impl IdentitySourceProvider {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Dingtalk(_) => IDENTITY_SOURCE_PROVIDER_DINGTALK,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DingTalkIdentitySourceConfig {
    pub app_key: String,
    pub app_secret: SecretString,
    pub corp_id: Option<String>,
    pub department_ids: Vec<String>,
    pub fetch_user_detail: bool,
}

impl OrchConfig {
    /// Loads orch configuration from environment variables.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when the bind address or shutdown settings are invalid.
    pub fn from_env() -> Result<Self, ConfigError> {
        let identity_source_enabled: bool = parse("WARP_IDENTITY_SOURCE_ENABLED", "false")?;
        let identity_source = identity_source_config(identity_source_enabled)?;
        Ok(Self {
            netbird_naming: netbird_managed_naming()?,
            bind_addr: parse("WARP_ORCH_BIND_ADDR", "127.0.0.1:8090")?,
            shutdown: shutdown_config_from_env()?,
            database_url: SecretString::from(required("WARP_DATABASE_URL")?),
            netbird_base_url: required("WARP_NETBIRD_BASE_URL")?,
            netbird_api_token: SecretString::from(required("WARP_NETBIRD_API_TOKEN")?),
            keycloak_admin: keycloak_config()?,
            identity_source_enabled,
            identity_source,
            auto_approve_initial_enabled: parse("WARP_AUTO_APPROVE_INITIAL_ENABLED", "false")?,
            identity_sync_interval: parse_duration(
                "WARP_IDENTITY_SYNC_INTERVAL_SECONDS",
                DEFAULT_IDENTITY_SYNC_INTERVAL_SECONDS,
            )?,
            audit_poll_interval: parse_duration(
                "WARP_AUDIT_POLL_INTERVAL_SECONDS",
                DEFAULT_AUDIT_POLL_INTERVAL_SECONDS,
            )?,
            authz_apply_interval: parse_duration(
                "WARP_AUTHZ_APPLY_INTERVAL_SECONDS",
                DEFAULT_AUTHZ_APPLY_INTERVAL_SECONDS,
            )?,
            authz_auto_approve_enabled: parse_bool("WARP_AUTHZ_AUTO_APPROVE_ENABLED", true)?,
            authz_auto_approve_max_seconds: parse_positive_seconds(
                "WARP_AUTHZ_AUTO_APPROVE_MAX_SECONDS",
                DEFAULT_AUTHZ_AUTO_APPROVE_MAX_SECONDS,
            )?,
            ssh_host_cert_ttl_seconds: parse_positive_seconds(
                "WARP_SSH_HOST_CERT_TTL_SECONDS",
                DEFAULT_SSH_HOST_CERT_TTL_SECONDS,
            )?,
            ssh_manual_cert_max_ttl_seconds: parse_positive_seconds(
                "WARP_SSH_MANUAL_CERT_MAX_TTL_SECONDS",
                DEFAULT_SSH_MANUAL_CERT_MAX_TTL_SECONDS,
            )?,
            authz_reconcile_interval: parse_duration(
                "WARP_AUTHZ_RECONCILE_INTERVAL_SECONDS",
                DEFAULT_AUTHZ_RECONCILE_INTERVAL_SECONDS,
            )?,
            asset_sync_interval: parse_duration(
                "WARP_ASSET_SYNC_INTERVAL_SECONDS",
                DEFAULT_ASSET_SYNC_INTERVAL_SECONDS,
            )?,
            audit_retention_days: parse_positive_seconds(
                "WARP_AUDIT_RETENTION_DAYS",
                DEFAULT_AUDIT_RETENTION_DAYS,
            )?,
            asset_center_base_url: optional("WARP_ASSET_CENTER_BASE_URL"),
            asset_center_token: optional("WARP_ASSET_CENTER_TOKEN").map(SecretString::from),
            asset_center_snapshot_path: value(
                "WARP_ASSET_CENTER_SNAPSHOT_PATH",
                DEFAULT_ASSET_CENTER_SNAPSHOT_PATH,
            ),
            authz_request_timeout: parse_duration(
                "WARP_AUTHZ_REQUEST_TIMEOUT_SECONDS",
                DEFAULT_AUTHZ_REQUEST_TIMEOUT_SECONDS,
            )?,
            ssh_ca_base_url: value("WARP_SSH_CA_BASE_URL", "http://127.0.0.1:8091"),
            ssh_ca_request_timeout: parse_duration(
                "WARP_SSH_CA_REQUEST_TIMEOUT_SECONDS",
                DEFAULT_SSH_CA_REQUEST_TIMEOUT_SECONDS,
            )?,
            posture_check: parse_posture_check(optional("WARP_POLICY_POSTURE_CHECK_JSON"))?,
        })
    }
}

impl ApiConfig {
    /// Loads and validates API configuration from the process environment.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when a required value is absent or malformed.
    pub fn from_env() -> Result<Self, ConfigError> {
        let (oidc_issuer, oidc_audience) =
            oidc_config(optional("WARP_OIDC_ISSUER"), optional("WARP_OIDC_AUDIENCE"))?;

        Ok(Self {
            netbird_naming: netbird_managed_naming()?,
            bind_addr: parse("WARP_API_BIND_ADDR", "0.0.0.0:8080")?,
            database_url: SecretString::from(required("WARP_DATABASE_URL")?),
            oidc_issuer,
            oidc_audience,
            allowed_origins: parse_allowed_origins(
                "WARP_ALLOWED_ORIGINS",
                "http://localhost:5173,http://localhost:5174",
            )?,
            local_session_secure: value("WARP_LOCAL_SESSION_SECURE", "false").parse().map_err(
                |_| ConfigError::Invalid {
                    name: "WARP_LOCAL_SESSION_SECURE",
                    value: value("WARP_LOCAL_SESSION_SECURE", "false"),
                },
            )?,
            local_session_ttl_hours: parse("WARP_LOCAL_SESSION_TTL_HOURS", "12")?,
            shutdown: shutdown_config_from_env()?,
            orch_base_url: value("WARP_ORCH_BASE_URL", "http://127.0.0.1:8090"),
            authz_request_timeout: parse_duration(
                "WARP_AUTHZ_REQUEST_TIMEOUT_SECONDS",
                DEFAULT_AUTHZ_REQUEST_TIMEOUT_SECONDS,
            )?,
            policy_templates: parse_policy_templates(optional("WARP_POLICY_TEMPLATES_JSON"))?,
            ssh_ca_base_url: value("WARP_SSH_CA_BASE_URL", "http://127.0.0.1:8091"),
            ssh_ca_request_timeout: parse_duration(
                "WARP_SSH_CA_REQUEST_TIMEOUT_SECONDS",
                DEFAULT_SSH_CA_REQUEST_TIMEOUT_SECONDS,
            )?,
            ssh_manual_cert_max_ttl_seconds: parse_positive_seconds(
                "WARP_SSH_MANUAL_CERT_MAX_TTL_SECONDS",
                DEFAULT_SSH_MANUAL_CERT_MAX_TTL_SECONDS,
            )?,
            client_pkg_base_url: value(
                "WARP_CLIENT_PKG_BASE_URL",
                "https://github.com/dayu-sec/warp-ztna-client/releases/download",
            ),
        })
    }
}

impl SshCaConfig {
    /// Loads the adapter configuration from environment variables.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when the bind address, shutdown settings, `step-ca` URL or the
    /// provisioner JWK path are missing or invalid.
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            bind_addr: parse("WARP_SSH_CA_BIND_ADDR", DEFAULT_SSH_CA_BIND_ADDR)?,
            shutdown: shutdown_config_from_env()?,
            step_ca_url: required("STEP_CA_URL")?,
            step_ca_roots_file: required("STEP_CA_ROOTS_FILE")?,
            step_ca_provisioner_name: value(
                "STEP_CA_PROVISIONER_NAME",
                DEFAULT_STEP_CA_PROVISIONER_NAME,
            ),
            step_ca_provisioner_jwk_file: required("STEP_CA_PROVISIONER_JWK_FILE")?,
            step_ca_provisioner_jwk_password_file: optional(
                "STEP_CA_PROVISIONER_JWK_PASSWORD_FILE",
            ),
            step_ca_ssh_user_ca_file: optional("STEP_CA_SSH_USER_CA_FILE"),
            step_ca_ssh_host_ca_file: optional("STEP_CA_SSH_HOST_CA_FILE"),
            step_ca_request_timeout: parse_duration(
                "STEP_CA_REQUEST_TIMEOUT_SECONDS",
                DEFAULT_SSH_CA_REQUEST_TIMEOUT_SECONDS,
            )?,
        })
    }
}

impl MigrationConfig {
    /// Loads the database-only configuration used by the migration command.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when `WARP_DATABASE_URL` is absent.
    pub fn from_env() -> Result<Self, ConfigError> {
        migration_config(optional("WARP_DATABASE_URL"))
    }
}

fn oidc_config(
    issuer: Option<String>,
    audience: Option<String>,
) -> Result<(String, String), ConfigError> {
    Ok((
        issuer.ok_or(ConfigError::Missing("WARP_OIDC_ISSUER"))?,
        audience.ok_or(ConfigError::Missing("WARP_OIDC_AUDIENCE"))?,
    ))
}

fn migration_config(database_url: Option<String>) -> Result<MigrationConfig, ConfigError> {
    Ok(MigrationConfig {
        database_url: SecretString::from(
            database_url.ok_or(ConfigError::Missing("WARP_DATABASE_URL"))?,
        ),
    })
}

fn parse_posture_check(raw: Option<String>) -> Result<Option<PostureCheckConfig>, ConfigError> {
    let Some(raw) = raw else { return Ok(None) };
    parse_posture_check_value(&raw)
}

fn parse_posture_check_value(raw: &str) -> Result<Option<PostureCheckConfig>, ConfigError> {
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let mut config: Option<PostureCheckConfig> = serde_json::from_str(raw).map_err(|_| {
        ConfigError::Invalid { name: "WARP_POLICY_POSTURE_CHECK_JSON", value: raw.to_owned() }
    })?;
    if let Some(config) = config.as_mut() {
        if config.processes.is_empty() {
            return Err(ConfigError::Invalid {
                name: "WARP_POLICY_POSTURE_CHECK_JSON",
                value: raw.to_owned(),
            });
        }
        for process in &mut config.processes {
            process.linux_path = process.linux_path.trim().to_owned();
            process.mac_path = process.mac_path.trim().to_owned();
            process.windows_path = process.windows_path.trim().to_owned();
            if process.linux_path.is_empty()
                && process.mac_path.is_empty()
                && process.windows_path.is_empty()
            {
                return Err(ConfigError::Invalid {
                    name: "WARP_POLICY_POSTURE_CHECK_JSON",
                    value: raw.to_owned(),
                });
            }
        }
    }
    Ok(config)
}

fn parse_policy_templates(raw: Option<String>) -> Result<Vec<PolicyTemplateConfig>, ConfigError> {
    let Some(raw) = raw else { return Ok(Vec::new()) };
    parse_policy_templates_value(&raw)
}

fn parse_policy_templates_value(raw: &str) -> Result<Vec<PolicyTemplateConfig>, ConfigError> {
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    let templates: Vec<PolicyTemplateConfig> = serde_json::from_str(raw).map_err(|_| {
        ConfigError::Invalid { name: "WARP_POLICY_TEMPLATES_JSON", value: raw.to_owned() }
    })?;
    let mut ids = std::collections::HashSet::with_capacity(templates.len());
    for template in &templates {
        if template.id.trim().is_empty()
            || template.name.trim().is_empty()
            || template.action != "accept"
        {
            return Err(ConfigError::Invalid {
                name: "WARP_POLICY_TEMPLATES_JSON",
                value: raw.to_owned(),
            });
        }
        if !ids.insert(template.id.as_str()) {
            return Err(ConfigError::Invalid {
                name: "WARP_POLICY_TEMPLATES_JSON",
                value: raw.to_owned(),
            });
        }
        if !matches!(template.protocol.as_str(), "tcp" | "udp" | "icmp" | "all") {
            return Err(ConfigError::Invalid {
                name: "WARP_POLICY_TEMPLATES_JSON",
                value: raw.to_owned(),
            });
        }
        if matches!(template.protocol.as_str(), "icmp" | "all")
            && (!template.ports.is_empty() || !template.port_ranges.is_empty())
        {
            return Err(ConfigError::Invalid {
                name: "WARP_POLICY_TEMPLATES_JSON",
                value: raw.to_owned(),
            });
        }
        for port in &template.ports {
            let value = port.parse::<u16>().ok();
            if value.is_none_or(|value| value == 0) {
                return Err(ConfigError::Invalid {
                    name: "WARP_POLICY_TEMPLATES_JSON",
                    value: raw.to_owned(),
                });
            }
        }
        for range in &template.port_ranges {
            if range.start == 0 || range.end == 0 || range.start > range.end {
                return Err(ConfigError::Invalid {
                    name: "WARP_POLICY_TEMPLATES_JSON",
                    value: raw.to_owned(),
                });
            }
        }
    }
    Ok(templates)
}

#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// `NetBird` local daemon gRPC address (`unix://…` / `npipe://…`).
    pub netbird_daemon_addr: String,
}

impl AgentConfig {
    pub fn from_env() -> Self {
        Self {
            netbird_daemon_addr: value("WARP_NETBIRD_DAEMON_ADDR", default_netbird_daemon_addr()),
        }
    }
}

fn required(name: &'static str) -> Result<String, ConfigError> {
    optional(name).ok_or(ConfigError::Missing(name))
}

fn parse_duration(name: &'static str, default_seconds: u64) -> Result<Duration, ConfigError> {
    let value = value(name, &default_seconds.to_string());
    parse_duration_value(name, &value)
}

fn parse_duration_value(name: &'static str, value: &str) -> Result<Duration, ConfigError> {
    let seconds: u64 =
        value.parse().map_err(|_| ConfigError::Invalid { name, value: value.to_owned() })?;
    if seconds == 0 {
        return Err(ConfigError::Invalid { name, value: value.to_owned() });
    }
    Ok(Duration::from_secs(seconds))
}

fn parse_bool(name: &'static str, default: bool) -> Result<bool, ConfigError> {
    let value = value(name, if default { "true" } else { "false" });
    parse_bool_value(name, &value)
}

fn parse_bool_value(name: &'static str, value: &str) -> Result<bool, ConfigError> {
    value.parse().map_err(|_| ConfigError::Invalid { name, value: value.to_owned() })
}

fn keycloak_config() -> Result<Option<KeycloakAdminConfig>, ConfigError> {
    build_keycloak_config(
        optional("WARP_KEYCLOAK_BASE_URL"),
        optional("WARP_KEYCLOAK_REALM"),
        optional("WARP_KEYCLOAK_CLIENT_ID"),
        optional("WARP_KEYCLOAK_CLIENT_SECRET"),
    )
}

fn build_keycloak_config(
    base_url: Option<String>,
    realm: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
) -> Result<Option<KeycloakAdminConfig>, ConfigError> {
    // The realm has a deployment default, but does not enable the adapter by
    // itself. Any credential-bearing setting makes the full trio required.
    if base_url.is_none() && client_id.is_none() && client_secret.is_none() {
        return Ok(None);
    }
    Ok(Some(KeycloakAdminConfig {
        base_url: base_url.ok_or(ConfigError::Missing("WARP_KEYCLOAK_BASE_URL"))?,
        realm: realm.unwrap_or_else(|| "warp-ztna".to_owned()),
        client_id: client_id.ok_or(ConfigError::Missing("WARP_KEYCLOAK_CLIENT_ID"))?,
        client_secret: SecretString::from(
            client_secret.ok_or(ConfigError::Missing("WARP_KEYCLOAK_CLIENT_SECRET"))?,
        ),
    }))
}

fn identity_source_config(enabled: bool) -> Result<Option<IdentitySourceConfig>, ConfigError> {
    build_identity_source_config(
        enabled,
        IdentitySourceEnv {
            provider: optional("WARP_IDENTITY_SOURCE_PROVIDER"),
            group_root: optional("WARP_IDENTITY_SOURCE_GROUP_ROOT"),
            email_domain: optional("WARP_IDENTITY_SOURCE_EMAIL_DOMAIN"),
            dingtalk_app_key: optional("WARP_IDENTITY_SOURCE_DINGTALK_APP_KEY"),
            dingtalk_app_secret: optional("WARP_IDENTITY_SOURCE_DINGTALK_APP_SECRET"),
            dingtalk_corp_id: optional("WARP_IDENTITY_SOURCE_DINGTALK_CORP_ID"),
            dingtalk_department_ids: optional("WARP_IDENTITY_SOURCE_DINGTALK_DEPT_IDS"),
            dingtalk_fetch_user_detail: optional("WARP_IDENTITY_SOURCE_DINGTALK_FETCH_USER_DETAIL"),
        },
    )
}

#[derive(Debug, Default)]
struct IdentitySourceEnv {
    provider: Option<String>,
    group_root: Option<String>,
    email_domain: Option<String>,
    dingtalk_app_key: Option<String>,
    dingtalk_app_secret: Option<String>,
    dingtalk_corp_id: Option<String>,
    dingtalk_department_ids: Option<String>,
    dingtalk_fetch_user_detail: Option<String>,
}

fn build_identity_source_config(
    enabled: bool,
    env: IdentitySourceEnv,
) -> Result<Option<IdentitySourceConfig>, ConfigError> {
    match assemble_identity_source(env) {
        Ok(config) => Ok(Some(config)),
        Err(_) if !enabled => Ok(None),
        Err(error) => Err(error),
    }
}

fn assemble_identity_source(env: IdentitySourceEnv) -> Result<IdentitySourceConfig, ConfigError> {
    let provider_name =
        env.provider.ok_or(ConfigError::Missing("WARP_IDENTITY_SOURCE_PROVIDER"))?;
    let email_domain = env.email_domain.map(|domain| domain.to_lowercase());
    match provider_name.as_str() {
        IDENTITY_SOURCE_PROVIDER_DINGTALK => {
            let app_key = env
                .dingtalk_app_key
                .ok_or(ConfigError::Missing("WARP_IDENTITY_SOURCE_DINGTALK_APP_KEY"))?;
            let app_secret = env
                .dingtalk_app_secret
                .ok_or(ConfigError::Missing("WARP_IDENTITY_SOURCE_DINGTALK_APP_SECRET"))?;
            let department_ids = env
                .dingtalk_department_ids
                .ok_or(ConfigError::Missing("WARP_IDENTITY_SOURCE_DINGTALK_DEPT_IDS"))?;
            let fetch_user_detail = match env.dingtalk_fetch_user_detail {
                Some(raw) => {
                    parse_bool_value("WARP_IDENTITY_SOURCE_DINGTALK_FETCH_USER_DETAIL", &raw)?
                }
                None => false,
            };
            let group_root =
                env.group_root.unwrap_or_else(|| format!("/{IDENTITY_SOURCE_PROVIDER_DINGTALK}"));
            Ok(IdentitySourceConfig {
                provider: IdentitySourceProvider::Dingtalk(DingTalkIdentitySourceConfig {
                    app_key,
                    app_secret: SecretString::from(app_secret),
                    corp_id: env.dingtalk_corp_id,
                    department_ids: parse_csv_list(
                        "WARP_IDENTITY_SOURCE_DINGTALK_DEPT_IDS",
                        &department_ids,
                    )?,
                    fetch_user_detail,
                }),
                group_root: normalize_group_root(&group_root)?,
                email_domain,
            })
        }
        other => Err(ConfigError::Invalid {
            name: "WARP_IDENTITY_SOURCE_PROVIDER",
            value: other.to_owned(),
        }),
    }
}

fn normalize_group_root(raw: &str) -> Result<String, ConfigError> {
    let trimmed = raw.trim();
    let valid = trimmed.starts_with('/')
        && trimmed.len() > 1
        && !trimmed.ends_with('/')
        && !trimmed[1..].contains("//");
    if valid {
        Ok(trimmed.to_owned())
    } else {
        Err(ConfigError::Invalid { name: "WARP_IDENTITY_SOURCE_GROUP_ROOT", value: raw.to_owned() })
    }
}

fn shutdown_config_from_env() -> Result<ShutdownConfig, ConfigError> {
    let raw = optional("WARP_SHUTDOWN_GRACE_PERIOD_SECONDS");
    shutdown_config(raw.as_deref())
}

fn shutdown_config(raw: Option<&str>) -> Result<ShutdownConfig, ConfigError> {
    let default = DEFAULT_SHUTDOWN_GRACE_PERIOD_SECONDS.to_string();
    parse_shutdown_config_value(raw.unwrap_or(&default))
}

fn parse_shutdown_config_value(raw: &str) -> Result<ShutdownConfig, ConfigError> {
    let seconds = raw.parse::<u64>().map_err(|_| ConfigError::Invalid {
        name: "WARP_SHUTDOWN_GRACE_PERIOD_SECONDS",
        value: raw.to_owned(),
    })?;
    if seconds == 0 {
        return Err(ConfigError::Invalid {
            name: "WARP_SHUTDOWN_GRACE_PERIOD_SECONDS",
            value: raw.to_owned(),
        });
    }
    Ok(ShutdownConfig { grace_period: Duration::from_secs(seconds) })
}

fn optional(name: &'static str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

fn value(name: &'static str, default: &str) -> String {
    optional(name).unwrap_or_else(|| default.to_owned())
}

fn parse_allowed_origins(name: &'static str, default: &str) -> Result<Vec<String>, ConfigError> {
    let raw = match env::var(name) {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => default.to_owned(),
        Err(env::VarError::NotUnicode(value)) => {
            return Err(ConfigError::Invalid { name, value: value.to_string_lossy().into_owned() });
        }
    };
    parse_csv_list(name, &raw)
}

fn parse_csv_list(name: &'static str, raw: &str) -> Result<Vec<String>, ConfigError> {
    let origins = raw.split(',').map(str::trim).map(str::to_owned).collect::<Vec<_>>();
    if origins.iter().any(String::is_empty) {
        return Err(ConfigError::Invalid { name, value: raw.to_owned() });
    }
    Ok(origins)
}

fn parse<T>(name: &'static str, default: &str) -> Result<T, ConfigError>
where
    T: FromStr,
{
    let value = value(name, default);
    value.parse().map_err(|_| ConfigError::Invalid { name, value })
}

fn parse_positive_seconds(name: &'static str, default: i64) -> Result<i64, ConfigError> {
    let value = value(name, &default.to_string());
    parse_positive_seconds_value(name, &value)
}

fn parse_positive_seconds_value(name: &'static str, value: &str) -> Result<i64, ConfigError> {
    let seconds: i64 =
        value.parse().map_err(|_| ConfigError::Invalid { name, value: value.to_owned() })?;
    if seconds <= 0 {
        return Err(ConfigError::Invalid { name, value: value.to_owned() });
    }
    Ok(seconds)
}

#[cfg(windows)]
fn default_netbird_daemon_addr() -> &'static str {
    r"npipe://netbird"
}

#[cfg(not(windows))]
fn default_netbird_daemon_addr() -> &'static str {
    "unix:///var/run/netbird.sock"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_netbird_daemon_addr_is_local() {
        let config = AgentConfig::from_env();
        assert!(
            config.netbird_daemon_addr.starts_with("unix://")
                || config.netbird_daemon_addr.starts_with("npipe://")
        );
    }

    #[test]
    fn netbird_managed_naming_derives_all_entity_names() {
        let naming = NetBirdManagedNaming::new("corp-warp").expect("valid prefix");
        assert_eq!(naming.base_prefix, "corp-warp");
        assert_eq!(naming.keycloak_group_prefix, "corp-warp-kc-");
        assert_eq!(naming.resource_group_prefix, "corp-warp-rg-");
        assert_eq!(naming.follow_group_prefix, "corp-warp-fg-");
        assert_eq!(naming.resource_prefix, "corp-warp-res-");
        assert_eq!(naming.policy_prefix, "corp-warp-pol-");
        assert_eq!(naming.peer_group_prefix, "corp-warp-pg-");
        assert_eq!(naming.process_posture_check_name, "corp-warp-process-check");
    }

    #[test]
    fn netbird_managed_naming_rejects_invalid_prefixes() {
        for value in
            ["", "-warp", "warp-", "warp_name", "warp name", " warp", "warp ", &"a".repeat(65)]
        {
            assert!(NetBirdManagedNaming::new(value).is_err(), "invalid prefix: {value:?}");
        }
        assert!(NetBirdManagedNaming::new("warp-ztna").is_ok());
        assert!(NetBirdManagedNaming::new(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn client_pkg_base_url_falls_back_to_the_package_host() {
        assert_eq!(
            value(
                "WARP_CLIENT_PKG_BASE_URL",
                "https://github.com/dayu-sec/warp-ztna-client/releases/download"
            ),
            "https://github.com/dayu-sec/warp-ztna-client/releases/download"
        );
    }

    #[test]
    fn allowed_origins_are_split_and_trimmed() {
        let origins = parse_csv_list(
            "WARP_ALLOWED_ORIGINS",
            " https://admin.example.com,https://portal.example.com ",
        )
        .expect("valid origins");
        assert_eq!(origins, ["https://admin.example.com", "https://portal.example.com"]);
    }

    #[test]
    fn allowed_origins_reject_empty_entries() {
        let error = parse_csv_list("WARP_ALLOWED_ORIGINS", "https://admin.example.com,")
            .expect_err("empty origin entry must fail");
        assert!(matches!(error, ConfigError::Invalid { name: "WARP_ALLOWED_ORIGINS", .. }));

        let error =
            parse_csv_list("WARP_ALLOWED_ORIGINS", "").expect_err("empty origin list must fail");
        assert!(matches!(error, ConfigError::Invalid { name: "WARP_ALLOWED_ORIGINS", .. }));
    }

    #[test]
    fn api_oidc_config_requires_issuer_and_audience() {
        assert!(matches!(
            oidc_config(None, Some("warp-ztna-api".to_owned())),
            Err(ConfigError::Missing("WARP_OIDC_ISSUER"))
        ));
        assert!(matches!(
            oidc_config(Some("https://idp.example/realms/warp-ztna".to_owned()), None),
            Err(ConfigError::Missing("WARP_OIDC_AUDIENCE"))
        ));
        assert!(
            oidc_config(
                Some("https://idp.example/realms/warp-ztna".to_owned()),
                Some("warp-ztna-api".to_owned())
            )
            .is_ok()
        );
    }

    #[test]
    fn migration_config_only_requires_database_url() {
        assert!(matches!(migration_config(None), Err(ConfigError::Missing("WARP_DATABASE_URL"))));
        assert!(migration_config(Some("postgres://warp@localhost/warp_ztna".to_owned())).is_ok());
    }

    #[test]
    fn posture_check_config_defaults_to_disabled() {
        assert!(parse_posture_check(None).expect("missing posture check").is_none());
        assert!(parse_posture_check_value(" ").expect("empty posture check").is_none());
        assert!(parse_posture_check_value("null").expect("null posture check").is_none());
    }

    #[test]
    fn posture_check_config_requires_processes_with_a_path() {
        assert!(parse_posture_check_value(r#"{"processes":[]}"#).is_err());
        assert!(parse_posture_check_value(r#"{"processes":[{}]}"#).is_err());
        assert!(parse_posture_check_value("not-json").is_err());
    }

    #[test]
    fn posture_check_config_trims_process_paths() {
        let config = parse_posture_check_value(
            r#"{"processes":[{"linux_path":" /usr/local/bin/netbird ","mac_path":"","windows_path":"C:\\ProgramData\\NetBird\\netbird.exe"}]}"#,
        )
        .expect("valid posture check")
        .expect("posture check should be enabled");
        assert_eq!(config.processes[0].linux_path, "/usr/local/bin/netbird");
        assert_eq!(config.processes[0].mac_path, "");
        assert_eq!(config.processes[0].windows_path, r"C:\ProgramData\NetBird\netbird.exe");
    }

    #[test]
    fn policy_templates_default_to_empty_and_accept_empty_array() {
        assert!(parse_policy_templates(None).expect("missing templates").is_empty());
        assert!(parse_policy_templates_value(" ").expect("empty templates").is_empty());
        assert!(parse_policy_templates_value("[]").expect("empty templates").is_empty());
    }

    #[test]
    fn policy_templates_reject_invalid_json() {
        assert!(matches!(
            parse_policy_templates_value("not-json"),
            Err(ConfigError::Invalid { name: "WARP_POLICY_TEMPLATES_JSON", .. })
        ));
    }

    #[test]
    fn policy_templates_reject_duplicate_ids_and_invalid_protocols() {
        let duplicate = r#"[
            {"id":"k8s","name":"Kubernetes","protocol":"tcp","ports":["6443"],"port_ranges":[],"bidirectional":false,"action":"accept"},
            {"id":"k8s","name":"Other","protocol":"tcp","ports":["443"],"port_ranges":[],"bidirectional":false,"action":"accept"}
        ]"#;
        assert!(parse_policy_templates_value(duplicate).is_err());

        let invalid_protocol = r#"[{"id":"icmp","name":"ICMP","protocol":"sctp","ports":[],"port_ranges":[],"bidirectional":false,"action":"accept"}]"#;
        assert!(parse_policy_templates_value(invalid_protocol).is_err());
    }

    #[test]
    fn policy_templates_reject_invalid_ports_and_ports_for_icmp() {
        let invalid_port = r#"[{"id":"bad","name":"Bad","protocol":"tcp","ports":["0"],"port_ranges":[],"bidirectional":false,"action":"accept"}]"#;
        assert!(parse_policy_templates_value(invalid_port).is_err());

        let invalid_icmp = r#"[{"id":"icmp","name":"ICMP","protocol":"icmp","ports":["8"],"port_ranges":[],"bidirectional":false,"action":"accept"}]"#;
        assert!(parse_policy_templates_value(invalid_icmp).is_err());

        let invalid_range = r#"[{"id":"range","name":"Range","protocol":"tcp","ports":[],"port_ranges":[{"start":9000,"end":8999}],"bidirectional":false,"action":"accept"}]"#;
        assert!(parse_policy_templates_value(invalid_range).is_err());

        let invalid_action = r#"[{"id":"deny","name":"Deny","protocol":"tcp","ports":["443"],"port_ranges":[],"bidirectional":false,"action":"deny"}]"#;
        assert!(parse_policy_templates_value(invalid_action).is_err());
    }

    fn dingtalk_env() -> IdentitySourceEnv {
        IdentitySourceEnv {
            provider: Some(IDENTITY_SOURCE_PROVIDER_DINGTALK.to_owned()),
            dingtalk_app_key: Some("app-key".to_owned()),
            dingtalk_app_secret: Some("app-secret".to_owned()),
            dingtalk_department_ids: Some("1, 2".to_owned()),
            ..IdentitySourceEnv::default()
        }
    }

    fn dingtalk_config(enabled: bool, env: IdentitySourceEnv) -> DingTalkIdentitySourceConfig {
        let config = build_identity_source_config(enabled, env)
            .expect("valid identity source config")
            .expect("identity source is configured");
        match config.provider {
            IdentitySourceProvider::Dingtalk(provider) => provider,
        }
    }

    #[test]
    fn identity_source_is_disabled_when_nothing_is_declared() {
        assert!(
            build_identity_source_config(false, IdentitySourceEnv::default())
                .expect("absent identity source config is disabled")
                .is_none()
        );
    }

    #[test]
    fn a_half_filled_block_stays_inert_while_the_source_is_disabled() {
        let credentials_only = IdentitySourceEnv {
            dingtalk_app_key: Some("app-key".to_owned()),
            dingtalk_app_secret: Some("app-secret".to_owned()),
            ..IdentitySourceEnv::default()
        };
        assert!(
            build_identity_source_config(false, credentials_only)
                .expect("credentials without a scope are ignored while disabled")
                .is_none()
        );

        let scope_only = IdentitySourceEnv {
            dingtalk_department_ids: Some("10".to_owned()),
            ..IdentitySourceEnv::default()
        };
        assert!(
            build_identity_source_config(false, scope_only)
                .expect("a scope without credentials is ignored while disabled")
                .is_none()
        );
    }

    #[test]
    fn enabled_identity_source_requires_a_provider() {
        assert!(matches!(
            build_identity_source_config(true, IdentitySourceEnv::default()),
            Err(ConfigError::Missing("WARP_IDENTITY_SOURCE_PROVIDER"))
        ));
    }

    #[test]
    fn declared_provider_requires_its_whole_block() {
        let no_secret = IdentitySourceEnv { dingtalk_app_secret: None, ..dingtalk_env() };
        assert!(matches!(
            build_identity_source_config(true, no_secret),
            Err(ConfigError::Missing("WARP_IDENTITY_SOURCE_DINGTALK_APP_SECRET"))
        ));

        let no_scope = IdentitySourceEnv { dingtalk_department_ids: None, ..dingtalk_env() };
        assert!(matches!(
            build_identity_source_config(true, no_scope),
            Err(ConfigError::Missing("WARP_IDENTITY_SOURCE_DINGTALK_DEPT_IDS"))
        ));
    }

    #[test]
    fn provider_block_without_a_provider_name_is_rejected() {
        let unnamed = IdentitySourceEnv { provider: None, ..dingtalk_env() };
        assert!(matches!(
            build_identity_source_config(true, unnamed),
            Err(ConfigError::Missing("WARP_IDENTITY_SOURCE_PROVIDER"))
        ));
    }

    #[test]
    fn unknown_provider_names_are_rejected() {
        let unknown = IdentitySourceEnv { provider: Some("feishu".to_owned()), ..dingtalk_env() };
        assert!(matches!(
            build_identity_source_config(true, unknown),
            Err(ConfigError::Invalid { name: "WARP_IDENTITY_SOURCE_PROVIDER", .. })
        ));
    }

    #[test]
    fn dingtalk_block_defaults_group_root_department_scope_and_detail_fetch() {
        let provider = dingtalk_config(false, dingtalk_env());

        assert_eq!(provider.app_key, "app-key");
        assert_eq!(provider.department_ids, vec!["1", "2"]);
        assert!(!provider.fetch_user_detail);
        assert_eq!(provider.corp_id, None);

        let config = build_identity_source_config(false, dingtalk_env())
            .expect("valid identity source config")
            .expect("identity source is configured");
        assert_eq!(config.provider_name(), IDENTITY_SOURCE_PROVIDER_DINGTALK);
        assert_eq!(config.group_root, "/dingtalk");
    }

    #[test]
    fn dingtalk_block_accepts_overrides() {
        let env = IdentitySourceEnv {
            group_root: Some("/corp".to_owned()),
            email_domain: Some("Example.COM".to_owned()),
            dingtalk_corp_id: Some("corp-id".to_owned()),
            dingtalk_fetch_user_detail: Some("true".to_owned()),
            ..dingtalk_env()
        };
        let config = build_identity_source_config(false, env)
            .expect("valid identity source config")
            .expect("identity source is configured");

        assert_eq!(config.group_root, "/corp");
        assert_eq!(config.email_domain.as_deref(), Some("example.com"));
    }

    #[test]
    fn group_root_must_be_an_absolute_non_root_path() {
        for value in ["dingtalk", "/dingtalk/", "/", "/dingtalk//team"] {
            let env = IdentitySourceEnv { group_root: Some(value.to_owned()), ..dingtalk_env() };
            assert!(
                matches!(
                    build_identity_source_config(true, env),
                    Err(ConfigError::Invalid { name: "WARP_IDENTITY_SOURCE_GROUP_ROOT", .. })
                ),
                "value must be rejected: {value}"
            );
        }
    }

    #[test]
    fn keycloak_realm_alone_does_not_enable_the_adapter() {
        assert!(
            build_keycloak_config(None, Some("warp-ztna".to_owned()), None, None)
                .expect("realm-only config is disabled")
                .is_none()
        );
    }

    #[test]
    fn shutdown_config_defaults_to_twenty_five_seconds() {
        assert_eq!(
            shutdown_config(None).expect("default shutdown config").grace_period,
            Duration::from_secs(DEFAULT_SHUTDOWN_GRACE_PERIOD_SECONDS)
        );
    }

    #[test]
    fn shutdown_config_accepts_override() {
        assert_eq!(
            parse_shutdown_config_value("42").expect("valid shutdown config").grace_period,
            Duration::from_secs(42)
        );
    }

    #[test]
    fn shutdown_config_rejects_non_positive_and_invalid_values() {
        for value in ["0", "-1", "not-a-number", "18446744073709551616"] {
            assert!(parse_shutdown_config_value(value).is_err(), "value must be rejected: {value}");
        }
    }

    #[test]
    fn identity_sync_interval_defaults_to_four_hours() {
        assert_eq!(
            parse_duration(
                "WARP_IDENTITY_SYNC_INTERVAL_SECONDS",
                DEFAULT_IDENTITY_SYNC_INTERVAL_SECONDS,
            )
            .expect("default identity sync interval"),
            Duration::from_hours(4)
        );
    }

    #[test]
    fn audit_poll_interval_defaults_to_fifteen_minutes() {
        assert_eq!(
            parse_duration_value(
                "WARP_AUDIT_POLL_INTERVAL_SECONDS",
                &DEFAULT_AUDIT_POLL_INTERVAL_SECONDS.to_string(),
            )
            .expect("default audit poll interval"),
            Duration::from_mins(15)
        );
    }

    #[test]
    fn audit_poll_interval_accepts_override() {
        assert_eq!(
            parse_duration_value("WARP_AUDIT_POLL_INTERVAL_SECONDS", "60")
                .expect("audit poll interval override"),
            Duration::from_mins(1)
        );
    }

    #[test]
    fn audit_poll_interval_rejects_non_positive_and_invalid_values() {
        for value in ["0", "-1", "not-a-number", "18446744073709551616"] {
            assert!(
                parse_duration_value("WARP_AUDIT_POLL_INTERVAL_SECONDS", value).is_err(),
                "value must be rejected: {value}"
            );
        }
    }

    #[test]
    fn asset_center_snapshot_path_defaults_to_the_reference_endpoint() {
        assert_eq!(
            value("WARP_ASSET_CENTER_SNAPSHOT_PATH", DEFAULT_ASSET_CENTER_SNAPSHOT_PATH),
            "/api/v1/ztna/asset-graph/snapshot"
        );
    }

    #[test]
    fn asset_sync_interval_defaults_to_twelve_hours() {
        assert_eq!(
            parse_duration_value(
                "WARP_ASSET_SYNC_INTERVAL_SECONDS",
                &DEFAULT_ASSET_SYNC_INTERVAL_SECONDS.to_string(),
            )
            .expect("default asset sync interval"),
            Duration::from_hours(12)
        );
    }

    #[test]
    fn asset_sync_interval_accepts_override() {
        assert_eq!(
            parse_duration_value("WARP_ASSET_SYNC_INTERVAL_SECONDS", "60")
                .expect("asset sync interval override"),
            Duration::from_mins(1)
        );
    }

    #[test]
    fn asset_sync_interval_rejects_non_positive_and_invalid_values() {
        for value in ["0", "-1", "not-a-number", "18446744073709551616"] {
            assert!(
                parse_duration_value("WARP_ASSET_SYNC_INTERVAL_SECONDS", value).is_err(),
                "value must be rejected: {value}"
            );
        }
    }

    #[test]
    fn bool_values_accept_only_true_and_false() {
        let enabled = parse_bool_value("WARP_AUTHZ_AUTO_APPROVE_ENABLED", "true")
            .expect("true parses as a bool");
        let disabled = parse_bool_value("WARP_AUTHZ_AUTO_APPROVE_ENABLED", "false")
            .expect("false parses as a bool");
        assert!(enabled && !disabled);
    }

    #[test]
    fn bool_values_reject_everything_else() {
        for value in ["", " ", "1", "0", "yes", "no", "TRUE", "not-a-bool"] {
            assert!(
                parse_bool_value("WARP_AUTHZ_AUTO_APPROVE_ENABLED", value).is_err(),
                "value must be rejected: {value:?}"
            );
        }
    }

    #[test]
    fn an_unset_bool_uses_its_default() {
        let defaults_to_true =
            parse_bool("WARP_TEST_UNSET_BOOL", true).expect("unset bool falls back to true");
        let defaults_to_false =
            parse_bool("WARP_TEST_UNSET_BOOL", false).expect("unset bool falls back to false");
        assert!(defaults_to_true && !defaults_to_false);
    }

    #[test]
    fn audit_retention_days_defaults_to_ninety() {
        assert_eq!(
            parse_positive_seconds_value(
                "WARP_AUDIT_RETENTION_DAYS",
                &DEFAULT_AUDIT_RETENTION_DAYS.to_string(),
            )
            .expect("default audit retention days"),
            90
        );
    }

    #[test]
    fn audit_retention_days_accepts_override() {
        assert_eq!(
            parse_positive_seconds_value("WARP_AUDIT_RETENTION_DAYS", "7")
                .expect("audit retention override"),
            7
        );
    }

    #[test]
    fn audit_retention_days_rejects_non_positive_and_invalid_values() {
        for value in ["0", "-1", "not-a-number", "99999999999999999999"] {
            assert!(
                parse_positive_seconds_value("WARP_AUDIT_RETENTION_DAYS", value).is_err(),
                "value must be rejected: {value}"
            );
        }
    }
}
