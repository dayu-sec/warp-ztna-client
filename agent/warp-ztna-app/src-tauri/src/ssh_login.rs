use std::{
    ffi::OsStr,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

use serde::Serialize;
use ssh_key::{Certificate, HashAlg, PublicKey};

use crate::error::{
    CmdResult, CommandError, SSH_CERT_NOT_READY_CODE, SSH_LOCAL_CONFIG_CODE, SSH_NO_LOCAL_KEY_CODE,
    SSH_UNSUPPORTED_ACCOUNT_CODE,
};
use crate::process::{run_osascript, wait_within_status};

const ACCOUNT_MAX_CHARS: usize = 64;
const HOST_MAX_CHARS: usize = 253;
const CERTIFICATE_SUFFIX: &str = "-cert.pub";
const PUBLIC_KEY_SUFFIX: &str = ".pub";
const PROBE_BUDGET: Duration = Duration::from_secs(3);
const PROBE_BUDGET_WINDOWS: Duration = Duration::from_secs(5);
const OSASCRIPT_BUDGET: Duration = Duration::from_mins(1);
const ITERM_APP: &str = "/Applications/iTerm.app";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Platform {
    Macos,
    Windows,
    Linux,
    Other,
}

impl Platform {
    pub(crate) fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else {
            Self::Other
        }
    }

    fn probe_budget(self) -> Duration {
        if self == Self::Windows { PROBE_BUDGET_WINDOWS } else { PROBE_BUDGET }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LaunchStatus {
    Launched,
    NeedsConfig,
    CopyOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum KnownHostsAction {
    Appended,
    AlreadyTrusted,
    KeptOtherPattern,
}

#[derive(Debug)]
pub(crate) struct ResolvedKey {
    pub(crate) key_path: PathBuf,
    pub(crate) fingerprint: String,
    pub(crate) loadable: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct SshLaunchDto {
    pub(crate) status: LaunchStatus,
    pub(crate) platform: Platform,
    pub(crate) host: String,
    pub(crate) account: String,
    pub(crate) command: String,
    pub(crate) alternate_command: Option<String>,
    pub(crate) launch_error: Option<String>,
    pub(crate) key_path: String,
    pub(crate) certificate_path: String,
    pub(crate) certificate_serial: Option<String>,
    pub(crate) certificate_valid_before: Option<String>,
    pub(crate) known_hosts_path: String,
    pub(crate) known_hosts_action: KnownHostsAction,
    pub(crate) known_hosts_pattern: Option<String>,
    pub(crate) replaced_certificate_file: bool,
    pub(crate) terminal_available: bool,
}

pub(crate) struct LaunchRequest {
    pub(crate) home: PathBuf,
    pub(crate) account: String,
    pub(crate) host: String,
    pub(crate) key: ResolvedKey,
    pub(crate) certificate: String,
    pub(crate) certificate_serial: Option<String>,
    pub(crate) certificate_valid_before: Option<String>,
    pub(crate) known_hosts_line: String,
}

fn valid_account(account: &str) -> bool {
    !account.is_empty()
        && account.len() <= ACCOUNT_MAX_CHARS
        && !account.starts_with('-')
        && account.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
}

fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= HOST_MAX_CHARS
        && !host.starts_with('-')
        && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '_' | '-'))
}

fn ssh_tokens(explicit: Option<(&Path, &Path)>, account: &str, host: &str) -> Option<Vec<String>> {
    if !valid_account(account) || !valid_host(host) {
        return None;
    }
    let mut tokens = vec!["ssh".to_owned()];
    if let Some((key_path, certificate_path)) = explicit {
        tokens.push("-i".to_owned());
        tokens.push(key_path.to_string_lossy().into_owned());
        tokens.push("-o".to_owned());
        tokens.push(format!("CertificateFile={}", certificate_path.to_string_lossy()));
    }
    tokens.push(format!("{account}@{host}"));
    Some(tokens)
}

pub(crate) fn client_available() -> bool {
    ssh_binary().is_some()
}

fn shell_inert(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '.' | '_' | '+' | '-' | '/' | ':' | '@' | '=' | ',')
        })
}

fn shell_quote(value: &str) -> String {
    if shell_inert(value) {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn shell_join(tokens: &[String]) -> String {
    tokens.iter().map(|token| shell_quote(token)).collect::<Vec<_>>().join(" ")
}

fn escape_applescript(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            control if control.is_control() => {}
            other => escaped.push(other),
        }
    }
    escaped
}

fn terminal_app_script(command: &str) -> String {
    format!(
        "tell application \"Terminal\"\n  activate\n  do script \"{}\"\nend tell",
        escape_applescript(command)
    )
}

fn iterm_script(command: &str) -> String {
    format!(
        "tell application \"iTerm\"\n  activate\n  set w to (create window with default profile)\n  tell current session of w\n    write text \"{}\"\n  end tell\nend tell",
        escape_applescript(command)
    )
}

fn parse_identity_files(output: &str, home: &Path) -> Vec<PathBuf> {
    let mut identities = Vec::new();
    for line in output.lines() {
        let Some(value) = line.trim().strip_prefix("identityfile ") else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() || value.eq_ignore_ascii_case("none") {
            continue;
        }
        let path = expand_home(value, home);
        if !identities.contains(&path) {
            identities.push(path);
        }
    }
    identities
}

fn expand_home(value: &str, home: &Path) -> PathBuf {
    if value == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = value.strip_prefix("~/").or_else(|| value.strip_prefix("~\\")) {
        return home.join(rest);
    }
    PathBuf::from(value)
}

fn sibling_public_key(key_path: &Path) -> Option<PathBuf> {
    let name = key_path.file_name()?.to_str()?;
    Some(key_path.with_file_name(format!("{name}{PUBLIC_KEY_SUFFIX}")))
}

fn certificate_path_for(key_path: &Path) -> Option<PathBuf> {
    let name = key_path.file_name()?.to_str()?;
    Some(key_path.with_file_name(format!("{name}{CERTIFICATE_SUFFIX}")))
}

fn usably_paired(key_path: &Path) -> bool {
    key_path.is_file() && sibling_public_key(key_path).is_some_and(|path| path.is_file())
}

fn scanned_public_key_paths(ssh_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(ssh_dir) else {
        return Vec::new();
    };
    let mut keys = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        let Some(key_name) = name.strip_suffix(PUBLIC_KEY_SUFFIX) else {
            continue;
        };
        if key_name.is_empty() || key_name.ends_with("-cert") {
            continue;
        }
        let key_path = ssh_dir.join(key_name);
        if usably_paired(&key_path) {
            keys.push(key_path);
        }
    }
    keys.sort();
    keys
}

fn fingerprint_of(key_path: &Path) -> Option<String> {
    let path = sibling_public_key(key_path)?;
    let text = fs::read_to_string(path).ok()?;
    let key = PublicKey::from_openssh(text.trim()).ok()?;
    Some(key.fingerprint(HashAlg::Sha256).to_string())
}

fn matching_key(keys: &[PathBuf], fingerprint: &str) -> Option<PathBuf> {
    keys.iter().find(|key_path| fingerprint_of(key_path).as_deref() == Some(fingerprint)).cloned()
}

fn path_lookup(program: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|directory| directory.join(program))
        .find(|candidate| candidate.is_file())
}

fn ssh_binary() -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("SystemRoot") {
        let candidate = Path::new(&root).join("System32").join("OpenSSH").join("ssh.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let fallback = PathBuf::from("/usr/bin/ssh");
    if fallback.is_file() {
        return Some(fallback);
    }
    path_lookup("ssh")
}

fn wait_within(child: &mut Child, budget: Duration) -> bool {
    wait_within_status(child, budget).is_some_and(|status| status.success())
}

fn probe_identities(ssh: &Path, account: &str, host: &str, budget: Duration) -> Option<String> {
    let mut child = Command::new(ssh)
        .arg("-G")
        .arg(format!("{account}@{host}"))
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    if !wait_within(&mut child, budget) {
        return None;
    }
    let output = child.wait_with_output().ok()?;
    String::from_utf8(output.stdout).ok()
}

fn loadable_identities(home: &Path, account: &str, host: &str, platform: Platform) -> Vec<PathBuf> {
    let Some(ssh) = ssh_binary() else {
        return Vec::new();
    };
    let Some(output) = probe_identities(&ssh, account, host, platform.probe_budget()) else {
        return Vec::new();
    };
    parse_identity_files(&output, home)
        .into_iter()
        .filter(|key_path| usably_paired(key_path))
        .collect()
}

pub(crate) fn resolve_signing_key(
    home: &Path,
    platform: Platform,
    account: &str,
    host: &str,
    fingerprints: &[String],
) -> Option<ResolvedKey> {
    if fingerprints.is_empty() || !valid_account(account) || !valid_host(host) {
        return None;
    }
    let loadable = loadable_identities(home, account, host, platform);
    let mut installed = None;
    for fingerprint in fingerprints {
        if let Some(key_path) = matching_key(&loadable, fingerprint) {
            return Some(ResolvedKey {
                key_path,
                fingerprint: fingerprint.clone(),
                loadable: true,
            });
        }
        if installed.is_none() {
            let scanned = scanned_public_key_paths(&home.join(".ssh"));
            installed = matching_key(&scanned, fingerprint).map(|key_path| ResolvedKey {
                key_path,
                fingerprint: fingerprint.clone(),
                loadable: false,
            });
        }
    }
    installed
}

pub(crate) fn certificate_path_for_key(key_path: &Path) -> Option<PathBuf> {
    certificate_path_for(key_path)
}

pub(crate) fn certificate_is_current(text: &str) -> bool {
    Certificate::from_openssh(text.trim())
        .is_ok_and(|certificate| std::time::SystemTime::now() < certificate.valid_before_time())
}

pub(crate) fn existing_certificate(path: &Path, serial: Option<&str>) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let certificate = Certificate::from_openssh(text.trim()).ok()?;
    let serial = serial?;
    let reusable = certificate.serial().to_string() == serial
        && std::time::SystemTime::now() < certificate.valid_before_time();
    reusable.then_some(text)
}

pub(crate) fn certificate_principals(text: &str) -> Option<Vec<String>> {
    Certificate::from_openssh(text.trim())
        .ok()
        .map(|certificate| certificate.valid_principals().to_vec())
}

fn parse_known_hosts_line(line: &str) -> Option<(String, String)> {
    if line.chars().any(char::is_control) {
        return None;
    }
    let fields = line.split_whitespace().collect::<Vec<_>>();
    if fields.len() != 4 || fields[0] != "@cert-authority" {
        return None;
    }
    let key_type = fields[2];
    let known_type = key_type.starts_with("ssh-")
        || key_type.starts_with("ecdsa-")
        || key_type.starts_with("sk-");
    if !known_type {
        return None;
    }
    let blob = fields[3];
    let base64 = !blob.is_empty()
        && blob.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='));
    if !base64 {
        return None;
    }
    Some((fields[1].to_owned(), format!("{key_type} {blob}")))
}

fn known_hosts_append(
    existing: &str,
    incoming: &str,
) -> Option<(KnownHostsAction, Option<String>, String)> {
    let (_, incoming_key) = parse_known_hosts_line(incoming)?;
    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((pattern, key)) = parse_known_hosts_line(trimmed)
            && key == incoming_key
        {
            let action = if pattern == "*" {
                KnownHostsAction::AlreadyTrusted
            } else {
                KnownHostsAction::KeptOtherPattern
            };
            return Some((action, Some(pattern), String::new()));
        }
    }
    let newline = if existing.ends_with("\r\n") { "\r\n" } else { "\n" };
    let mut appended = String::new();
    if !existing.is_empty() && !existing.ends_with('\n') {
        appended.push_str(newline);
    }
    appended.push_str(incoming);
    appended.push_str(newline);
    Some((KnownHostsAction::Appended, None, appended))
}

fn ssh_dir_builder() -> fs::DirBuilder {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
}

fn append_options() -> fs::OpenOptions {
    let mut options = fs::OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

fn write_options() -> fs::OpenOptions {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

fn ensure_ssh_dir(ssh_dir: &Path) -> Result<(), String> {
    if ssh_dir.is_dir() {
        return Ok(());
    }
    ssh_dir_builder().create(ssh_dir).map_err(|error| error.to_string())
}

fn write_certificate(path: &Path, text: &str) -> Result<bool, String> {
    let existing = fs::read_to_string(path).ok();
    let replacing_foreign_file = existing
        .as_deref()
        .is_some_and(|existing| Certificate::from_openssh(existing.trim()).is_err());
    if replacing_foreign_file {
        log::warn!("ssh_login: replacing a file that is not an OpenSSH certificate");
    }
    if let Some(parent) = path.parent() {
        ensure_ssh_dir(parent)?;
    }
    let mut file = write_options().open(path).map_err(|error| error.to_string())?;
    file.write_all(text.trim().as_bytes()).map_err(|error| error.to_string())?;
    file.write_all(b"\n").map_err(|error| error.to_string())?;
    Ok(existing.is_some())
}

fn ensure_known_hosts(
    path: &Path,
    incoming: &str,
) -> Result<(KnownHostsAction, Option<String>), String> {
    let existing = fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    let Some((action, pattern, appended)) = known_hosts_append(&existing, incoming) else {
        return Err("the API returned invalid known_hosts material".to_owned());
    };
    if appended.is_empty() {
        return Ok((action, pattern));
    }
    if let Some(parent) = path.parent() {
        ensure_ssh_dir(parent)?;
    }
    let mut file = append_options().open(path).map_err(|error| error.to_string())?;
    file.write_all(appended.as_bytes()).map_err(|error| error.to_string())?;
    Ok((action, pattern))
}

fn run_detached(program: &Path, args: &[&str]) -> Result<(), String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn open_macos_terminal(tokens: &[String]) -> Result<(), String> {
    let command = shell_join(tokens);
    if Path::new(ITERM_APP).is_dir() {
        match run_osascript(&iterm_script(&command), OSASCRIPT_BUDGET) {
            Ok(()) => return Ok(()),
            Err(error) => log::warn!("ssh_login: iTerm2 refused the session: {error}"),
        }
    }
    run_osascript(&terminal_app_script(&command), OSASCRIPT_BUDGET)
}

fn open_windows_terminal(tokens: &[String]) -> Result<(), String> {
    let shell = std::env::var_os("ComSpec").map_or_else(|| PathBuf::from("cmd.exe"), PathBuf::from);
    let mut args = vec!["/c", "start", "", "cmd.exe", "/k"];
    args.extend(tokens.iter().map(String::as_str));
    run_detached(&shell, &args)
}

fn open_linux_terminal(command: &str) -> Result<(), String> {
    let candidates: [(&str, &[&str]); 4] = [
        ("x-terminal-emulator", &["-e", "sh", "-c"]),
        ("gnome-terminal", &["--", "sh", "-c"]),
        ("konsole", &["-e", "sh", "-c"]),
        ("xterm", &["-e", "sh", "-c"]),
    ];
    let mut last_error = "no terminal emulator was found".to_owned();
    for (program, args) in candidates {
        let Some(path) = path_lookup(program) else {
            continue;
        };
        let mut full = args.to_vec();
        full.push(command);
        match run_detached(&path, &full) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

fn open_terminal(platform: Platform, tokens: &[String]) -> Result<(), String> {
    match platform {
        Platform::Macos => open_macos_terminal(tokens),
        Platform::Windows => open_windows_terminal(tokens),
        Platform::Linux => open_linux_terminal(&shell_join(tokens)),
        Platform::Other => Err("no terminal launcher is available on this platform".to_owned()),
    }
}

pub(crate) fn launch(request: &LaunchRequest) -> CmdResult<SshLaunchDto> {
    let platform = Platform::current();
    let Some(tokens) = ssh_tokens(None, &request.account, &request.host) else {
        return Err(CommandError::coded(
            SSH_UNSUPPORTED_ACCOUNT_CODE,
            "the login account or host cannot be passed to a shell safely",
        ));
    };
    if !certificate_is_current(&request.certificate) {
        return Err(CommandError::coded(
            SSH_CERT_NOT_READY_CODE,
            "the certificate is expired or not yet valid",
        ));
    }
    let Some(certificate_path) = certificate_path_for(&request.key.key_path) else {
        return Err(CommandError::coded(
            SSH_NO_LOCAL_KEY_CODE,
            "the private key has no usable file name",
        ));
    };
    let replaced_certificate_file = write_certificate(&certificate_path, &request.certificate)
        .map_err(|error| CommandError::coded(SSH_LOCAL_CONFIG_CODE, error))?;
    let ssh_dir = request.home.join(".ssh");
    let known_hosts_path = ssh_dir.join("known_hosts");
    let (known_hosts_action, known_hosts_pattern) =
        ensure_known_hosts(&known_hosts_path, &request.known_hosts_line)
            .map_err(|error| CommandError::coded(SSH_LOCAL_CONFIG_CODE, error))?;
    let explicit = ssh_tokens(
        Some((&request.key.key_path, &certificate_path)),
        &request.account,
        &request.host,
    );
    let alternate_command = explicit.as_deref().map(shell_join);
    let mut status = LaunchStatus::CopyOnly;
    let mut launch_error = None;
    if request.key.loadable {
        match open_terminal(platform, &tokens) {
            Ok(()) => status = LaunchStatus::Launched,
            Err(error) => launch_error = Some(error),
        }
    }
    if status != LaunchStatus::Launched
        && let Some(explicit) = explicit.as_deref()
    {
        match open_terminal(platform, explicit) {
            Ok(()) => status = LaunchStatus::NeedsConfig,
            Err(error) => {
                launch_error = Some(match launch_error {
                    Some(first) => format!("{first}; {error}"),
                    None => error,
                });
            }
        }
    }
    Ok(SshLaunchDto {
        status,
        platform,
        host: request.host.clone(),
        account: request.account.clone(),
        command: tokens.join(" "),
        alternate_command,
        launch_error,
        key_path: request.key.key_path.to_string_lossy().into_owned(),
        certificate_path: certificate_path.to_string_lossy().into_owned(),
        certificate_serial: request.certificate_serial.clone(),
        certificate_valid_before: request.certificate_valid_before.clone(),
        known_hosts_path: known_hosts_path.to_string_lossy().into_owned(),
        known_hosts_action,
        known_hosts_pattern,
        replaced_certificate_file,
        terminal_available: platform != Platform::Other,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hostile() -> Vec<&'static str> {
        vec![
            "; rm -rf ~",
            "$(id)",
            "`id`",
            "a\"b",
            "a b",
            "a&b",
            "a|b",
            "a>b",
            "a<b",
            "a^b",
            "a%b",
            "a!b",
            "a(b)",
            "a'b",
            "a\\b",
            "a\nb",
            "a~b",
            "a*b",
            "a?b",
            "",
            "-F",
            "-",
            "--",
            "-oProxyCommand=x",
            "-host",
        ]
    }

    #[test]
    fn accounts_and_hosts_accept_only_shell_inert_characters() {
        assert!(valid_account("deploy_1.prod+x-y"));
        assert!(valid_host("10.20.1.5"));
        assert!(valid_host("db.corp.example"));
        assert!(valid_host("fd00::1"));
        for value in hostile() {
            assert!(!valid_account(value), "{value:?} must not pass as an account");
        }
        for value in hostile() {
            assert!(!valid_host(value), "{value:?} must not pass as a host");
        }
    }

    #[test]
    fn overlong_identifiers_are_rejected() {
        assert!(!valid_account(&"a".repeat(ACCOUNT_MAX_CHARS + 1)));
        assert!(!valid_host(&"a".repeat(HOST_MAX_CHARS + 1)));
        assert!(valid_account(&"a".repeat(ACCOUNT_MAX_CHARS)));
        assert!(valid_host(&"a".repeat(HOST_MAX_CHARS)));
    }

    #[test]
    fn no_hostile_input_reaches_a_command_string() {
        for value in hostile() {
            assert!(ssh_tokens(None, value, "10.20.1.5").is_none());
            assert!(ssh_tokens(None, "root", value).is_none());
        }
        assert_eq!(ssh_tokens(None, "root", "10.20.1.5").unwrap(), vec!["ssh", "root@10.20.1.5"]);
    }

    #[test]
    fn a_leading_dash_can_never_turn_the_target_into_an_ssh_option() {
        assert!(!valid_account("-F"));
        assert!(!valid_account("-"));
        assert!(!valid_account("-oProxyCommand=x"));
        assert!(!valid_host("-host"));
        assert!(ssh_tokens(None, "-F", "10.20.1.5").is_none());
        assert!(ssh_tokens(None, "-oProxyCommand=x", "10.20.1.5").is_none());
        assert!(valid_account("deploy-1"));
        assert!(valid_host("host-1"));
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_overruns_its_budget_is_killed_and_reaped() {
        let mut child = Command::new("/bin/sleep").arg("5").spawn().expect("spawn sleep");
        assert!(wait_within_status(&mut child, Duration::from_millis(100)).is_none());
        assert!(child.try_wait().expect("try_wait").is_some(), "the child must be reaped");
    }

    #[test]
    fn explicit_tokens_pin_the_identity_and_the_certificate() {
        let tokens = ssh_tokens(
            Some((
                Path::new("/home/dev/.ssh/id_work"),
                Path::new("/home/dev/.ssh/id_work-cert.pub"),
            )),
            "root",
            "10.20.1.5",
        )
        .unwrap();
        assert_eq!(
            tokens,
            vec![
                "ssh",
                "-i",
                "/home/dev/.ssh/id_work",
                "-o",
                "CertificateFile=/home/dev/.ssh/id_work-cert.pub",
                "root@10.20.1.5",
            ]
        );
    }

    #[test]
    fn shell_joining_survives_spaces_and_single_quotes() {
        let tokens = vec![
            "ssh".to_owned(),
            "-i".to_owned(),
            "/Users/a b/it's/id_rsa".to_owned(),
            "root@10.20.1.5".to_owned(),
        ];
        assert_eq!(shell_join(&tokens), r"ssh -i '/Users/a b/it'\''s/id_rsa' root@10.20.1.5");
    }

    #[test]
    fn shell_joining_leaves_readable_tokens_unquoted() {
        let tokens = ssh_tokens(
            Some((
                Path::new("/home/dev/.ssh/id_work"),
                Path::new("/home/dev/.ssh/id_work-cert.pub"),
            )),
            "root",
            "10.20.1.5",
        )
        .unwrap();
        assert_eq!(
            shell_join(&tokens),
            "ssh -i /home/dev/.ssh/id_work -o CertificateFile=/home/dev/.ssh/id_work-cert.pub root@10.20.1.5"
        );
    }

    #[test]
    fn applescript_escaping_closes_the_string_literal() {
        assert_eq!(
            escape_applescript("\" & (do shell script \"rm -rf ~\") & \""),
            "\\\" & (do shell script \\\"rm -rf ~\\\") & \\\""
        );
        assert_eq!(escape_applescript("a\\b"), "a\\\\b");
        assert_eq!(escape_applescript("a\nb"), "ab");
    }

    #[test]
    fn terminal_scripts_embed_the_escaped_command() {
        let script = terminal_app_script("ssh root@10.20.1.5");
        assert!(script.contains("do script \"ssh root@10.20.1.5\""));
        assert!(script.starts_with("tell application \"Terminal\""));
        let script = iterm_script("ssh root@10.20.1.5");
        assert!(script.contains("write text \"ssh root@10.20.1.5\""));
        assert!(script.starts_with("tell application \"iTerm\""));
    }

    #[test]
    fn identity_lines_are_home_expanded_and_deduplicated() {
        let output = "hostname db.corp.example\nidentityfile ~/.ssh/id_rsa\nidentityfile ~/.ssh/id_ed25519\nidentityfile ~/.ssh/id_rsa\nidentityfile none\nidentityfile /opt/keys/deploy\n";
        assert_eq!(
            parse_identity_files(output, Path::new("/home/dev")),
            vec![
                PathBuf::from("/home/dev/.ssh/id_rsa"),
                PathBuf::from("/home/dev/.ssh/id_ed25519"),
                PathBuf::from("/opt/keys/deploy"),
            ]
        );
    }

    #[test]
    fn certificate_paths_append_the_suffix_to_the_file_name() {
        assert_eq!(
            certificate_path_for(Path::new("/home/dev/.ssh/id_ed25519")),
            Some(PathBuf::from("/home/dev/.ssh/id_ed25519-cert.pub"))
        );
        assert_eq!(
            certificate_path_for(Path::new("/home/dev/.ssh/id_ed25519.work")),
            Some(PathBuf::from("/home/dev/.ssh/id_ed25519.work-cert.pub"))
        );
        assert_eq!(certificate_path_for(Path::new("/")), None);
    }

    #[test]
    fn anything_that_is_not_a_certificate_is_never_current() {
        assert!(!certificate_is_current(""));
        assert!(!certificate_is_current("not a certificate"));
        assert!(!certificate_is_current(
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAINiZIKD9IUyaK5lEh4lHIV8UqRYa7bjR9gwEGdNfiGF+ poc@warp"
        ));
    }

    #[test]
    fn certificate_authority_lines_are_strictly_parsed() {
        let line = "@cert-authority * ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTY=";
        assert_eq!(
            parse_known_hosts_line(line),
            Some((
                "*".to_owned(),
                "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTY=".to_owned(),
            ))
        );
        assert!(
            parse_known_hosts_line("@cert-authority * ssh-ed25519 AAAA\nssh-ed25519 AAAAB")
                .is_none()
        );
        assert!(parse_known_hosts_line("@cert-authority * ssh-ed25519").is_none());
        assert!(parse_known_hosts_line("@cert-authority * magic AAAA").is_none());
        assert!(parse_known_hosts_line("@cert-authority * ssh-ed25519 not base64!").is_none());
        assert!(parse_known_hosts_line("10.20.1.5 ssh-ed25519 AAAA").is_none());
    }

    #[test]
    fn known_hosts_is_idempotent_for_the_same_authority() {
        let line = "@cert-authority * ecdsa-sha2-nistp256 AAAAKEY";
        let (action, pattern, appended) = known_hosts_append(line, line).unwrap();
        assert_eq!(action, KnownHostsAction::AlreadyTrusted);
        assert_eq!(pattern.as_deref(), Some("*"));
        assert!(appended.is_empty());
    }

    #[test]
    fn known_hosts_keeps_a_narrower_existing_pattern() {
        let existing = "@cert-authority *.corp.example ecdsa-sha2-nistp256 AAAAKEY\n";
        let (action, pattern, appended) =
            known_hosts_append(existing, "@cert-authority * ecdsa-sha2-nistp256 AAAAKEY").unwrap();
        assert_eq!(action, KnownHostsAction::KeptOtherPattern);
        assert_eq!(pattern.as_deref(), Some("*.corp.example"));
        assert!(appended.is_empty());
    }

    #[test]
    fn known_hosts_appends_a_rotated_authority_and_keeps_the_old_line() {
        let existing = "@cert-authority * ecdsa-sha2-nistp256 AAAAOLD\n";
        let (action, pattern, appended) =
            known_hosts_append(existing, "@cert-authority * ecdsa-sha2-nistp256 AAAANEW").unwrap();
        assert_eq!(action, KnownHostsAction::Appended);
        assert!(pattern.is_none());
        assert_eq!(appended, "@cert-authority * ecdsa-sha2-nistp256 AAAANEW\n");
    }

    #[test]
    fn known_hosts_separates_records_that_lack_a_trailing_newline() {
        let (_, _, appended) = known_hosts_append(
            "10.20.1.5 ssh-ed25519 AAAA",
            "@cert-authority * ecdsa-sha2-nistp256 AAAAKEY",
        )
        .unwrap();
        assert_eq!(appended, "\n@cert-authority * ecdsa-sha2-nistp256 AAAAKEY\n");
    }

    #[test]
    fn known_hosts_into_an_empty_file_has_no_leading_blank_line() {
        let (_, _, appended) =
            known_hosts_append("", "@cert-authority * ecdsa-sha2-nistp256 AAAAKEY").unwrap();
        assert_eq!(appended, "@cert-authority * ecdsa-sha2-nistp256 AAAAKEY\n");
    }
}
