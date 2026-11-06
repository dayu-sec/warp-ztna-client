use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use serde::Deserialize;
use ssh_key::{Certificate, HashAlg, PublicKey};
use tauri::{AppHandle, Manager};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::sync::{Mutex, Notify};

use crate::{
    DesktopState, api_context, api_get,
    ssh_config::{self, CertificateMeta, ManagedHost},
    ssh_login,
};

const SYNC_CONTEXT: &str = "ssh sync";
const CERTIFICATES_CONTEXT: &str = "fetch ssh certificates";
const CLIENT_CONFIG_CONTEXT: &str = "fetch ssh client config";
const DOWNLOAD_CONTEXT: &str = "download ssh certificate";
const DEBOUNCE: Duration = Duration::from_secs(1);

pub(crate) struct SshSync {
    trigger: Notify,
    gate: Mutex<()>,
}

impl SshSync {
    pub(crate) fn new() -> Self {
        Self { trigger: Notify::new(), gate: Mutex::new(()) }
    }

    pub(crate) fn trigger(&self) {
        self.trigger.notify_one();
    }
}

pub(crate) async fn run(app: AppHandle) {
    let Some(sync) = app.try_state::<DesktopState>().map(|state| Arc::clone(&state.ssh_sync))
    else {
        return;
    };
    loop {
        sync.trigger.notified().await;
        tokio::time::sleep(DEBOUNCE).await;
        let _guard = sync.gate.lock().await;
        if let Err(error) = sync_once(&app).await {
            log::warn!("ssh_sync: {error}");
        }
    }
}

async fn sync_once(app: &AppHandle) -> Result<(), String> {
    let Some(state) = app.try_state::<DesktopState>() else {
        return Ok(());
    };
    let Some((base, token)) =
        api_context(state.inner(), SYNC_CONTEXT).await.map_err(|error| error.message)?
    else {
        return Ok(());
    };
    let certificates =
        api_get(&state.http, &base, "api/v1/me/ssh-certificates", &token, CERTIFICATES_CONTEXT)
            .await
            .map_err(|error| error.message)?;
    let config =
        api_get(&state.http, &base, "api/v1/me/ssh-client-config", &token, CLIENT_CONFIG_CONTEXT)
            .await
            .map_err(|error| error.message)?;
    let certificates: Vec<CertificateDto> =
        serde_json::from_value(certificates).map_err(|error| error.to_string())?;
    let config: ClientConfigDto =
        serde_json::from_value(config).map_err(|error| error.to_string())?;
    let home = app.path().home_dir().map_err(|error| error.to_string())?;
    let ssh_dir = home.join(".ssh");
    let metas = certificate_metas(&certificates);
    let now = OffsetDateTime::now_utc();

    if config.hosts.is_empty() {
        log_block(ssh_config::apply_managed_block(&ssh_dir, &[])?);
        log_prune(ssh_config::prune_certificates(&ssh_dir, &metas));
        return Ok(());
    }

    let keys = local_keys(&ssh_dir);
    let mut managed = Vec::new();
    let mut failed = false;
    let mut downloaded: HashMap<String, Option<String>> = HashMap::new();
    for host in &config.hosts {
        if !ssh_login::valid_host(&host.address) || !ssh_login::valid_account(&host.login_account) {
            log::debug!("ssh_sync: skipping invalid host or account");
            continue;
        }
        let principal = host.host_principal.as_deref().unwrap_or(&host.login_account);
        let Some((certificate, key_path)) =
            select_certificate(&certificates, principal, &keys, now)
        else {
            continue;
        };
        if !ssh_login::valid_request_id(&certificate.id) {
            continue;
        }
        let Some(request_id) = certificate
            .access_request_id
            .as_deref()
            .filter(|value| ssh_login::valid_request_id(value))
        else {
            continue;
        };
        let Some(certificate_path) = ssh_login::ticket_certificate_path(&home, request_id) else {
            continue;
        };
        let text = if let Some(value) = downloaded.get(&certificate.id) {
            value.clone()
        } else {
            let value =
                certificate_text(&state, &base, &token, certificate, &certificate_path, principal)
                    .await;
            downloaded.insert(certificate.id.clone(), value.clone());
            value
        };
        if text.is_none() {
            failed = true;
            continue;
        }
        managed.push(ManagedHost {
            address: host.address.clone(),
            account: host.login_account.clone(),
            key_path,
            certificate_path,
        });
    }
    if failed {
        log::warn!("ssh_sync: a certificate could not be downloaded; keeping the existing config");
        return Ok(());
    }
    if managed.is_empty() {
        log_block(ssh_config::apply_managed_block(&ssh_dir, &[])?);
        log_prune(ssh_config::prune_certificates(&ssh_dir, &metas));
        return Ok(());
    }
    ssh_login::ensure_known_hosts(&ssh_dir.join("known_hosts"), &config.known_hosts)?;
    log_block(ssh_config::apply_managed_block(&ssh_dir, &managed)?);
    log_prune(ssh_config::prune_certificates(&ssh_dir, &metas));
    Ok(())
}

async fn certificate_text(
    state: &DesktopState,
    base: &str,
    token: &str,
    certificate: &CertificateDto,
    path: &Path,
    principal: &str,
) -> Option<String> {
    if let Some(text) = ssh_login::existing_certificate(path, certificate.serial.as_deref())
        && local_certificate_matches(&text, certificate, principal)
    {
        return Some(text);
    }
    let value = api_get(
        &state.http,
        base,
        &format!("api/v1/me/ssh-certificates/{}/download", certificate.id),
        token,
        DOWNLOAD_CONTEXT,
    )
    .await
    .ok()?;
    let body: DownloadDto = serde_json::from_value(value).ok()?;
    let text = body.certificate?;
    if !local_certificate_matches(&text, certificate, principal) {
        log::warn!("ssh_sync: downloaded certificate failed validation for {}", certificate.id);
        return None;
    }
    if let Err(error) = ssh_login::write_certificate(path, &text) {
        log::warn!("ssh_sync: cannot write {}: {error}", path.display());
        return None;
    }
    Some(text)
}

fn local_certificate_matches(text: &str, api: &CertificateDto, principal: &str) -> bool {
    let Ok(parsed) = Certificate::from_openssh(text.trim()) else {
        return false;
    };
    let now = SystemTime::now();
    if parsed.valid_after_time() > now || parsed.valid_before_time() <= now {
        return false;
    }
    if let Some(serial) = api.serial.as_deref()
        && parsed.serial().to_string() != serial
    {
        return false;
    }
    if !parsed.valid_principals().iter().any(|value| value.as_str() == principal) {
        return false;
    }
    let Some(fingerprint) = api.public_key_fingerprint.as_deref() else {
        return false;
    };
    PublicKey::from(parsed.public_key().clone()).fingerprint(HashAlg::Sha256).to_string()
        == fingerprint
}

fn select_certificate<'a>(
    certificates: &'a [CertificateDto],
    principal: &str,
    keys: &HashMap<String, PathBuf>,
    now: OffsetDateTime,
) -> Option<(&'a CertificateDto, PathBuf)> {
    let mut candidates = certificates
        .iter()
        .filter(|certificate| {
            certificate_is_usable(certificate, now)
                && certificate.principals.iter().any(|value| value.as_str() == principal)
        })
        .filter_map(|certificate| {
            let key_path = certificate
                .public_key_fingerprint
                .as_ref()
                .and_then(|fingerprint| keys.get(fingerprint))?;
            Some((certificate, key_path))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|(left, left_key), (right, right_key)| {
        certificate_rank(right).cmp(&certificate_rank(left)).then_with(|| left_key.cmp(right_key))
    });
    candidates.first().map(|(certificate, key)| (*certificate, (*key).clone()))
}

fn certificate_is_usable(certificate: &CertificateDto, now: OffsetDateTime) -> bool {
    certificate.status == "issued"
        && certificate.request_status.as_deref() == Some("authorized")
        && certificate.public_key_fingerprint.is_some()
        && parse_time(certificate.valid_after.as_deref()).is_some_and(|value| value <= now)
        && parse_time(certificate.valid_before.as_deref()).is_some_and(|value| value > now)
}

fn certificate_rank(certificate: &CertificateDto) -> (i128, i128, &str) {
    (
        parse_time(certificate.valid_before.as_deref())
            .map_or(0, OffsetDateTime::unix_timestamp_nanos),
        parse_time(certificate.issued_at.as_deref())
            .map_or(0, OffsetDateTime::unix_timestamp_nanos),
        certificate.id.as_str(),
    )
}

fn parse_time(value: Option<&str>) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(value?, &Rfc3339).ok()
}

fn local_keys(ssh_dir: &Path) -> HashMap<String, PathBuf> {
    let mut keys = HashMap::new();
    for key_path in ssh_login::scanned_public_key_paths(ssh_dir) {
        if let Some(fingerprint) = ssh_login::fingerprint_of(&key_path) {
            keys.entry(fingerprint).or_insert(key_path);
        }
    }
    keys
}

fn certificate_metas(certificates: &[CertificateDto]) -> HashMap<String, CertificateMeta> {
    let mut metas = HashMap::new();
    for certificate in certificates {
        if let Some(id) = certificate.access_request_id.as_deref() {
            metas.insert(
                id.to_ascii_lowercase(),
                CertificateMeta {
                    status: certificate.status.clone(),
                    request_status: certificate.request_status.clone(),
                },
            );
        }
    }
    metas
}

fn log_block(outcome: ssh_config::BlockOutcome) {
    match outcome {
        ssh_config::BlockOutcome::Written { hosts } => {
            log::info!("ssh_sync: wrote a managed ssh block for {hosts} hosts");
        }
        ssh_config::BlockOutcome::Removed => {
            log::info!("ssh_sync: removed the managed ssh block");
        }
        ssh_config::BlockOutcome::Unchanged => {}
    }
}

fn log_prune(outcome: ssh_config::PruneOutcome) {
    if outcome.removed > 0 || outcome.failed > 0 {
        log::info!(
            "ssh_sync: removed {} certificate files ({} failed)",
            outcome.removed,
            outcome.failed
        );
    }
}

#[derive(Debug, Clone, Deserialize)]
struct CertificateDto {
    id: String,
    #[serde(default)]
    access_request_id: Option<String>,
    #[serde(default)]
    request_status: Option<String>,
    #[serde(default)]
    principals: Vec<String>,
    #[serde(default)]
    serial: Option<String>,
    #[serde(default)]
    status: String,
    #[serde(default)]
    valid_after: Option<String>,
    #[serde(default)]
    valid_before: Option<String>,
    #[serde(default)]
    issued_at: Option<String>,
    #[serde(default)]
    public_key_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ClientConfigDto {
    #[serde(default)]
    known_hosts: String,
    #[serde(default)]
    hosts: Vec<ClientHostDto>,
}

#[derive(Debug, Clone, Deserialize)]
struct ClientHostDto {
    address: String,
    login_account: String,
    #[serde(default)]
    host_principal: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DownloadDto {
    #[serde(default)]
    certificate: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn certificate(id: &str, valid_before: &str, fingerprint: &str) -> CertificateDto {
        CertificateDto {
            id: id.to_owned(),
            access_request_id: Some("00000000-0000-0000-0000-000000000001".to_owned()),
            request_status: Some("authorized".to_owned()),
            principals: vec!["warp:host:1".to_owned()],
            serial: Some("1".to_owned()),
            status: "issued".to_owned(),
            valid_after: Some("2026-01-01T00:00:00Z".to_owned()),
            valid_before: Some(valid_before.to_owned()),
            issued_at: Some("2026-01-01T00:00:00Z".to_owned()),
            public_key_fingerprint: Some(fingerprint.to_owned()),
        }
    }

    fn keys() -> HashMap<String, PathBuf> {
        HashMap::from([("SHA256:key".to_owned(), PathBuf::from("/home/user/.ssh/id_ed25519"))])
    }

    #[test]
    fn selects_the_longest_lived_certificate() {
        let now = OffsetDateTime::parse("2026-06-01T00:00:00Z", &Rfc3339).expect("now");
        let certificates = vec![
            certificate("short", "2026-07-01T00:00:00Z", "SHA256:key"),
            certificate("long", "2026-12-01T00:00:00Z", "SHA256:key"),
        ];
        let (selected, key) =
            select_certificate(&certificates, "warp:host:1", &keys(), now).expect("selected");
        assert_eq!(selected.id, "long");
        assert_eq!(key, PathBuf::from("/home/user/.ssh/id_ed25519"));
    }

    #[test]
    fn ignores_principal_mismatch_and_unknown_keys() {
        let now = OffsetDateTime::parse("2026-06-01T00:00:00Z", &Rfc3339).expect("now");
        let mut wrong_principal = certificate("wrong", "2026-12-01T00:00:00Z", "SHA256:key");
        wrong_principal.principals = vec!["warp:host:2".to_owned()];
        let unknown_key = certificate("unknown", "2026-12-01T00:00:00Z", "SHA256:other");
        assert!(
            select_certificate(&[wrong_principal, unknown_key], "warp:host:1", &keys(), now)
                .is_none()
        );
    }

    #[test]
    fn ignores_revoked_and_expired_certificates() {
        let now = OffsetDateTime::parse("2026-06-01T00:00:00Z", &Rfc3339).expect("now");
        let mut revoked = certificate("revoked", "2026-12-01T00:00:00Z", "SHA256:key");
        revoked.status = "revoked".to_owned();
        let expired = certificate("expired", "2026-05-01T00:00:00Z", "SHA256:key");
        assert!(select_certificate(&[revoked, expired], "warp:host:1", &keys(), now).is_none());
    }

    #[test]
    fn metadata_maps_ticket_ids() {
        let metas = certificate_metas(&[certificate("one", "2026-12-01T00:00:00Z", "SHA256:key")]);
        let meta = metas.get("00000000-0000-0000-0000-000000000001").expect("meta");
        assert_eq!(meta.status, "issued");
        assert_eq!(meta.request_status.as_deref(), Some("authorized"));
    }

    #[test]
    fn corrupt_local_certificate_never_matches() {
        let api = certificate("one", "2026-12-01T00:00:00Z", "SHA256:key");
        assert!(!local_certificate_matches("not a certificate", &api, "warp:host:1"));
    }
}
