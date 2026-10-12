use std::{thread, time::Duration};

use crate::process::run_osascript;
use crate::ssh_login::Platform;

const NETBIRD_REDIRECT_PORT: u16 = 53_000;
const CLOSE_BUDGET: Duration = Duration::from_secs(10);
const SCRIPTABLE_BROWSERS: [&str; 5] =
    ["Safari", "Google Chrome", "Microsoft Edge", "Brave Browser", "Chromium"];

pub(crate) fn close_netbird_redirect_tabs() {
    if Platform::current() != Platform::Macos {
        log::debug!(
            "sso_tab: closing the NetBird callback tab is not implemented on this platform yet"
        );
        return;
    }
    thread::spawn(|| {
        for browser in SCRIPTABLE_BROWSERS {
            let script = close_script(browser);
            match run_osascript(&script, CLOSE_BUDGET) {
                Ok(()) => log::debug!("sso_tab: asked {browser} to close the NetBird callback tab"),
                Err(error) => {
                    log::debug!("sso_tab: {browser} kept the NetBird callback tab: {error}");
                }
            }
        }
    });
}

fn close_script(browser: &str) -> String {
    let port = NETBIRD_REDIRECT_PORT;
    format!(
        "set hostPrefix to \"http://localhost:{port}/\"\nset loopbackPrefix to \"http://127.0.0.1:{port}/\"\ntry\n  if application \"{browser}\" is running then\n    tell application \"{browser}\"\n      repeat with w in windows\n        set n to count of tabs of w\n        if n > 0 then\n          repeat with i from n to 1 by -1\n            set u to URL of tab i of w\n            if (u starts with hostPrefix) or (u starts with loopbackPrefix) then close tab i of w\n          end repeat\n        end if\n      end repeat\n    end tell\n  end if\nend try"
    )
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    use std::{path::Path, process::Command};

    use super::*;

    #[test]
    fn script_targets_both_loopback_spellings() {
        let script = close_script("Safari");
        assert!(script.contains("http://localhost:53000/"));
        assert!(script.contains("http://127.0.0.1:53000/"));
    }

    #[test]
    fn script_names_the_browser_and_guards_on_running() {
        let script = close_script("Brave Browser");
        assert!(script.contains("application \"Brave Browser\" is running"));
        assert!(script.contains("tell application \"Brave Browser\""));
    }

    #[test]
    fn script_closes_by_url_and_never_by_index() {
        let script = close_script("Safari");
        assert!(script.contains(
            "if (u starts with hostPrefix) or (u starts with loopbackPrefix) then close tab i of w"
        ));
    }

    #[test]
    fn every_scriptable_browser_gets_a_script() {
        for browser in SCRIPTABLE_BROWSERS {
            let script = close_script(browser);
            assert!(script.contains(browser));
            assert!(script.ends_with("end try"));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn installed_browsers_accept_the_script() {
        for browser in SCRIPTABLE_BROWSERS {
            if !Path::new(&format!("/Applications/{browser}.app")).is_dir() {
                continue;
            }
            let script = close_script(browser);
            let output = Command::new("/usr/bin/osacompile")
                .args(["-o", "/dev/null", "-e", &script])
                .output()
                .expect("osacompile should be runnable");
            assert!(
                output.status.success(),
                "{browser} rejected the script: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
    }
}
