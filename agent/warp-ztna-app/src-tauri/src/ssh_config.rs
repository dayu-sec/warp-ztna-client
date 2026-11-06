use std::{collections::HashMap, fs, path::Path, sync::Mutex, time::SystemTime};

use ssh_key::Certificate;

use crate::ssh_login;

const BEGIN_MARKER: &str = "# >>> warp-ztna managed block >>>";
const END_MARKER: &str = "# <<< warp-ztna managed block <<<";
const CONFIG_FILE: &str = "config";
const BACKUP_FILE: &str = "config.warp-ztna.bak";
const CERT_SUFFIX: &str = "-cert.pub";
const CERT_DIR: &str = "warp-ztna";

static CONFIG_WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedHost {
    pub(crate) address: String,
    pub(crate) account: String,
    pub(crate) key_path: std::path::PathBuf,
    pub(crate) certificate_path: std::path::PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockOutcome {
    Unchanged,
    Written { hosts: usize },
    Removed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PruneOutcome {
    pub(crate) removed: usize,
    pub(crate) failed: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct CertificateMeta {
    pub(crate) status: String,
    pub(crate) request_status: Option<String>,
}

struct ConfigText {
    text: String,
    bom: bool,
    newline: String,
    raw: Vec<u8>,
}

pub(crate) fn apply_managed_block(
    ssh_dir: &Path,
    hosts: &[ManagedHost],
) -> Result<BlockOutcome, String> {
    let _guard = CONFIG_WRITE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    ssh_login::ensure_ssh_dir(ssh_dir)?;
    let config_path = ssh_dir.join(CONFIG_FILE);
    let write_path = match fs::canonicalize(&config_path) {
        Ok(path) => path,
        Err(_)
            if fs::symlink_metadata(&config_path)
                .is_ok_and(|metadata| metadata.file_type().is_symlink()) =>
        {
            return Err("the ssh config is a symlink to a missing file".to_owned());
        }
        Err(_) => config_path.clone(),
    };
    let current = read_config(&config_path)?;
    let (text, bom, newline, raw) = match current {
        Some(value) => (value.text, value.bom, value.newline, Some(value.raw)),
        None => (String::new(), false, default_newline(), None),
    };
    let stripped = strip_blocks(&text)?;
    let (next, written) = if hosts.is_empty() {
        (stripped, 0)
    } else {
        let (block, written) = render_block(hosts, &newline);
        (insert_block(&stripped, &block, &newline), written)
    };
    if next == text && !bom {
        return Ok(BlockOutcome::Unchanged);
    }
    write_config(&config_path, &write_path, raw.as_deref(), &next)?;
    Ok(if hosts.is_empty() {
        BlockOutcome::Removed
    } else {
        BlockOutcome::Written { hosts: written }
    })
}

pub(crate) fn prune_certificates(
    ssh_dir: &Path,
    metas: &HashMap<String, CertificateMeta>,
) -> PruneOutcome {
    let cert_dir = ssh_dir.join(CERT_DIR);
    let Ok(entries) = fs::read_dir(&cert_dir) else {
        return PruneOutcome::default();
    };
    let now = SystemTime::now();
    let mut outcome = PruneOutcome::default();
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(ToOwned::to_owned) else {
            continue;
        };
        let Some(request_id) = name.strip_suffix(CERT_SUFFIX) else {
            continue;
        };
        if !ssh_login::valid_request_id(request_id) {
            continue;
        }
        let path = entry.path();
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let valid_before = Certificate::from_openssh(text.trim())
            .ok()
            .map(|certificate| certificate.valid_before_time());
        let meta = metas.get(&request_id.to_ascii_lowercase());
        if !should_prune(valid_before, meta, now) {
            continue;
        }
        match fs::remove_file(&path) {
            Ok(()) => outcome.removed += 1,
            Err(error) => {
                outcome.failed += 1;
                log::warn!("ssh_config: cannot remove {}: {error}", path.display());
            }
        }
    }
    outcome
}

fn should_prune(
    valid_before: Option<SystemTime>,
    meta: Option<&CertificateMeta>,
    now: SystemTime,
) -> bool {
    let Some(valid_before) = valid_before else {
        return true;
    };
    if valid_before <= now {
        return true;
    }
    let Some(meta) = meta else {
        return false;
    };
    meta.status != "issued" || meta.request_status.as_deref() != Some("authorized")
}

fn read_config(path: &Path) -> Result<Option<ConfigText>, String> {
    let raw = match fs::read(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let (body, bom) = split_bom(&raw)?;
    let text = std::str::from_utf8(body).map_err(|error| error.to_string())?.to_owned();
    let newline = detect_newline(&text);
    Ok(Some(ConfigText { text, bom, newline, raw }))
}

fn split_bom(raw: &[u8]) -> Result<(&[u8], bool), String> {
    if raw.starts_with(&[0xFF, 0xFE]) || raw.starts_with(&[0xFE, 0xFF]) {
        return Err("the ssh config is UTF-16 encoded".to_owned());
    }
    if let Some(rest) = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return Ok((rest, true));
    }
    Ok((raw, false))
}

fn detect_newline(text: &str) -> String {
    if text.contains("\r\n") {
        "\r\n".to_owned()
    } else if text.contains('\n') {
        "\n".to_owned()
    } else {
        default_newline()
    }
}

fn default_newline() -> String {
    if cfg!(windows) { "\r\n".to_owned() } else { "\n".to_owned() }
}

fn strip_blocks(text: &str) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut in_block = false;
    for line in text.split_inclusive('\n') {
        let marker = line.trim_end_matches(['\r', '\n']).trim_end();
        if marker == BEGIN_MARKER {
            if in_block {
                return Err("the ssh config has nested managed blocks".to_owned());
            }
            in_block = true;
            continue;
        }
        if marker == END_MARKER {
            if !in_block {
                return Err("the ssh config has a managed block end without begin".to_owned());
            }
            in_block = false;
            continue;
        }
        if !in_block {
            out.push_str(line);
        }
    }
    if in_block {
        return Err("the ssh config has an unclosed managed block".to_owned());
    }
    Ok(out)
}

fn insert_block(rest: &str, block: &str, newline: &str) -> String {
    let mut out = String::with_capacity(block.len() + rest.len() + newline.len());
    out.push_str(block);
    if !rest.is_empty() {
        if !block.ends_with(newline) {
            out.push_str(newline);
        }
        out.push_str(rest);
    }
    out
}

fn render_block(hosts: &[ManagedHost], newline: &str) -> (String, usize) {
    let mut valid = hosts
        .iter()
        .filter(|host| {
            ssh_login::valid_host(&host.address)
                && ssh_login::valid_account(&host.account)
                && render_path(&host.key_path).is_some()
                && render_path(&host.certificate_path).is_some()
        })
        .collect::<Vec<_>>();
    valid.sort_by(|left, right| {
        left.address
            .to_ascii_lowercase()
            .cmp(&right.address.to_ascii_lowercase())
            .then_with(|| left.address.cmp(&right.address))
    });
    valid.dedup_by(|left, right| left.address.eq_ignore_ascii_case(&right.address));
    if valid.len() != hosts.len() {
        log::debug!(
            "ssh_config: skipped {} managed hosts with invalid or duplicate values",
            hosts.len() - valid.len()
        );
    }
    let mut out = String::new();
    out.push_str(BEGIN_MARKER);
    out.push_str(newline);
    for host in &valid {
        let key_path = render_path(&host.key_path).unwrap_or_default();
        let certificate_path = render_path(&host.certificate_path).unwrap_or_default();
        out.push_str("Host ");
        out.push_str(&host.address);
        out.push_str(newline);
        out.push_str("    HostName ");
        out.push_str(&host.address);
        out.push_str(newline);
        out.push_str("    User ");
        out.push_str(&host.account);
        out.push_str(newline);
        out.push_str("    IdentityFile ");
        out.push_str(&key_path);
        out.push_str(newline);
        out.push_str("    CertificateFile ");
        out.push_str(&certificate_path);
        out.push_str(newline);
        out.push_str("    IdentitiesOnly yes");
        out.push_str(newline);
    }
    out.push_str("Host *");
    out.push_str(newline);
    out.push_str(END_MARKER);
    out.push_str(newline);
    (out, valid.len())
}

fn render_path(path: &Path) -> Option<String> {
    let text = path.to_str()?;
    let normalized = if cfg!(windows) { text.replace('\\', "/") } else { text.to_owned() };
    if normalized.is_empty()
        || normalized
            .chars()
            .any(|character| character.is_control() || character == '"' || character == '%')
    {
        return None;
    }
    if normalized.chars().any(char::is_whitespace) {
        Some(format!("\"{normalized}\""))
    } else {
        Some(normalized)
    }
}

fn write_config(
    config_path: &Path,
    write_path: &Path,
    expected: Option<&[u8]>,
    text: &str,
) -> Result<(), String> {
    let current = match fs::read(write_path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.to_string()),
    };
    let unchanged = match (expected, current.as_deref()) {
        (Some(expected), Some(current)) => expected == current,
        (None, None) => true,
        _ => false,
    };
    if !unchanged {
        return Err("the ssh config changed while it was being synchronized".to_owned());
    }
    if current.is_some() {
        let backup = config_path.with_file_name(BACKUP_FILE);
        if !backup.exists() {
            fs::copy(config_path, &backup).map_err(|error| error.to_string())?;
        }
    }
    write_bytes(write_path, text.as_bytes())
}

#[cfg(unix)]
fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let mode = fs::metadata(path).map_or(0o600, |metadata| metadata.permissions().mode());
    let temporary = path.with_file_name(format!(".{}.warp-ztna.tmp", std::process::id()));
    fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(mode))
        .map_err(|error| error.to_string())?;
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        error.to_string()
    })
}

#[cfg(not(unix))]
fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(bytes).map_err(|error| error.to_string())?;
    file.flush().map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("warp-ztna-ssh-config-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn host(address: &str) -> ManagedHost {
        ManagedHost {
            address: address.to_owned(),
            account: "root".to_owned(),
            key_path: PathBuf::from("/home/user/.ssh/id_ed25519"),
            certificate_path: PathBuf::from("/home/user/.ssh/warp-ztna/ticket-cert.pub"),
        }
    }

    fn read(dir: &Path) -> String {
        fs::read_to_string(dir.join("config")).expect("read config")
    }

    #[test]
    fn inserts_block_at_top_and_keeps_existing_content() {
        let dir = temp_dir("insert");
        fs::write(dir.join("config"), "ServerAliveInterval 60\n").expect("write config");
        let outcome = apply_managed_block(&dir, &[host("10.0.0.1")]).expect("apply");
        assert_eq!(outcome, BlockOutcome::Written { hosts: 1 });
        let text = read(&dir);
        assert!(text.starts_with(BEGIN_MARKER));
        assert!(text.contains("Host 10.0.0.1\n"));
        assert!(text.contains("Host *\n"));
        assert!(text.ends_with("ServerAliveInterval 60\n"));
        assert!(dir.join(BACKUP_FILE).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn identical_block_is_unchanged() {
        let dir = temp_dir("unchanged");
        apply_managed_block(&dir, &[host("10.0.0.1")]).expect("apply");
        let before = read(&dir);
        let outcome = apply_managed_block(&dir, &[host("10.0.0.1")]).expect("apply");
        assert_eq!(outcome, BlockOutcome::Unchanged);
        assert_eq!(read(&dir), before);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn replaces_previous_block_without_duplicating() {
        let dir = temp_dir("replace");
        apply_managed_block(&dir, &[host("10.0.0.1")]).expect("apply");
        apply_managed_block(&dir, &[host("10.0.0.2")]).expect("apply");
        let text = read(&dir);
        assert_eq!(text.matches(BEGIN_MARKER).count(), 1);
        assert!(!text.contains("Host 10.0.0.1"));
        assert!(text.contains("Host 10.0.0.2"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn removes_block_but_keeps_user_content() {
        let dir = temp_dir("remove");
        fs::write(dir.join("config"), "Host example\n    User me\n").expect("write config");
        apply_managed_block(&dir, &[host("10.0.0.1")]).expect("apply");
        let outcome = apply_managed_block(&dir, &[]).expect("remove");
        assert_eq!(outcome, BlockOutcome::Removed);
        let text = read(&dir);
        assert!(!text.contains(BEGIN_MARKER));
        assert!(text.contains("Host example"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn preserves_crlf() {
        let dir = temp_dir("crlf");
        fs::write(dir.join("config"), "ServerAliveInterval 60\r\n").expect("write config");
        apply_managed_block(&dir, &[host("10.0.0.1")]).expect("apply");
        let text = read(&dir);
        assert!(text.contains("\r\nHost 10.0.0.1\r\n"));
        assert!(!text.replace("\r\n", "").contains('\n'));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn strips_utf8_bom() {
        let dir = temp_dir("bom");
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"ServerAliveInterval 60\n");
        fs::write(dir.join("config"), bytes).expect("write config");
        apply_managed_block(&dir, &[host("10.0.0.1")]).expect("apply");
        let bytes = fs::read(dir.join("config")).expect("read config");
        assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
        assert!(String::from_utf8(bytes).expect("utf8").starts_with(BEGIN_MARKER));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_utf16() {
        let dir = temp_dir("utf16");
        fs::write(dir.join("config"), [0xFF, 0xFE, b'a', 0x00]).expect("write config");
        assert!(apply_managed_block(&dir, &[host("10.0.0.1")]).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_unclosed_block() {
        let dir = temp_dir("unclosed");
        fs::write(dir.join("config"), format!("{BEGIN_MARKER}\nHost 10.0.0.1\n")).expect("write");
        assert!(apply_managed_block(&dir, &[host("10.0.0.1")]).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn merges_duplicate_blocks() {
        let dir = temp_dir("duplicate");
        fs::write(
            dir.join("config"),
            format!("{BEGIN_MARKER}\nHost 10.0.0.9\n{END_MARKER}\n{BEGIN_MARKER}\nHost 10.0.0.8\n{END_MARKER}\n"),
        )
        .expect("write");
        apply_managed_block(&dir, &[host("10.0.0.1")]).expect("apply");
        let text = read(&dir);
        assert_eq!(text.matches(BEGIN_MARKER).count(), 1);
        assert!(!text.contains("10.0.0.9"));
        assert!(text.contains("10.0.0.1"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn quotes_paths_with_spaces_and_skips_invalid_tokens() {
        let dir = temp_dir("paths");
        let mut spaced = host("10.0.0.1");
        spaced.key_path = PathBuf::from("/home/user/with space/id_ed25519");
        let mut injected = host("10.0.0.2");
        injected.account = "root\nHost evil".to_owned();
        apply_managed_block(&dir, &[spaced, injected]).expect("apply");
        let text = read(&dir);
        assert!(text.contains("IdentityFile \"/home/user/with space/id_ed25519\""));
        assert!(!text.contains("Host evil"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_decision_covers_expiry_and_metadata() {
        let now = SystemTime::now();
        let issued = CertificateMeta {
            status: "issued".to_owned(),
            request_status: Some("authorized".to_owned()),
        };
        assert!(should_prune(None, Some(&issued), now));
        assert!(should_prune(Some(now - std::time::Duration::from_secs(1)), Some(&issued), now));
        assert!(!should_prune(Some(now + std::time::Duration::from_mins(1)), Some(&issued), now));
        assert!(!should_prune(Some(now + std::time::Duration::from_mins(1)), None, now));
        let revoked = CertificateMeta {
            status: "revoked".to_owned(),
            request_status: Some("authorized".to_owned()),
        };
        assert!(should_prune(Some(now + std::time::Duration::from_mins(1)), Some(&revoked), now));
        let inactive = CertificateMeta {
            status: "issued".to_owned(),
            request_status: Some("expired".to_owned()),
        };
        assert!(should_prune(Some(now + std::time::Duration::from_mins(1)), Some(&inactive), now));
    }

    #[test]
    fn prune_removes_corrupt_files_and_ignores_other_names() {
        let dir = temp_dir("prune");
        let cert_dir = dir.join(CERT_DIR);
        fs::create_dir_all(&cert_dir).expect("create cert dir");
        let corrupt = cert_dir.join("00000000-0000-0000-0000-000000000001-cert.pub");
        fs::write(&corrupt, "not a certificate").expect("write corrupt");
        let other = cert_dir.join("notes.txt");
        fs::write(&other, "keep me").expect("write other");
        let outcome = prune_certificates(&dir, &HashMap::new());
        assert_eq!(outcome.removed, 1);
        assert!(!corrupt.exists());
        assert!(other.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn writes_through_symlink_without_replacing_it() {
        use std::os::unix::fs::symlink;

        let dir = temp_dir("symlink");
        let target = dir.join("dotfiles-config");
        fs::write(&target, "ServerAliveInterval 60\n").expect("write target");
        let link = dir.join("config");
        symlink(&target, &link).expect("create symlink");
        apply_managed_block(&dir, &[host("10.0.0.1")]).expect("apply");
        assert!(fs::symlink_metadata(&link).expect("metadata").file_type().is_symlink());
        assert!(fs::read_to_string(&target).expect("read target").contains(BEGIN_MARKER));
        let _ = fs::remove_dir_all(&dir);
    }
}
