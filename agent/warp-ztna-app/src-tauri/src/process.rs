use std::{
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

pub(crate) const POLL_INTERVAL: Duration = Duration::from_millis(20);

pub(crate) fn wait_within_status(child: &mut Child, budget: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + budget;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => thread::sleep(POLL_INTERVAL),
            Err(_) => {
                let _ = child.kill();
                return None;
            }
        }
    }
}

pub(crate) fn run_osascript(script: &str, budget: Duration) -> Result<(), String> {
    let mut child = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    if wait_within_status(&mut child, budget).is_none() {
        return Err(format!(
            "osascript did not return within {}s — an automation consent dialog may be waiting",
            budget.as_secs()
        ));
    }
    let output = child.wait_with_output().map_err(|error| error.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(if stderr.is_empty() { format!("osascript exited with {}", output.status) } else { stderr })
}
