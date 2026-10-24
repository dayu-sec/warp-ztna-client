use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::error::{CmdResult, CommandError};
use warp_ztna_err::prelude::*;

const PREFS_FILE: &str = "prefs.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct NativePrefs {
    pub lang: String,
    pub notify: bool,
    pub session_lead_min: u32,
    pub tray_theme: String,
}

impl Default for NativePrefs {
    fn default() -> Self {
        Self {
            lang: "zh".to_owned(),
            notify: true,
            session_lead_min: 15,
            tray_theme: "dark".to_owned(),
        }
    }
}

fn prefs_path(dir: &Path) -> PathBuf {
    dir.join(PREFS_FILE)
}

pub(crate) fn load(dir: &Path) -> Option<NativePrefs> {
    let path = prefs_path(dir);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            log::warn!("native prefs: cannot read {}: {error}", path.display());
            return None;
        }
    };
    match serde_json::from_str(&raw) {
        Ok(prefs) => Some(prefs),
        Err(error) => {
            log::warn!("native prefs: ignoring malformed {}: {error}", path.display());
            None
        }
    }
}

pub(crate) fn save(dir: &Path, prefs: &NativePrefs) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = prefs_path(dir);
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_vec_pretty(prefs).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &path)
}

fn config_dir(app: &AppHandle) -> CmdResult<PathBuf> {
    app.path().app_config_dir().map_err(|error| {
        log::warn!("native prefs: cannot resolve config dir: {error}");
        CommandError::from(explain(Reason::External, "resolve app config dir"))
    })
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn get_native_prefs(app: AppHandle) -> CmdResult<Option<NativePrefs>> {
    Ok(load(&config_dir(&app)?))
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn set_native_prefs(app: AppHandle, prefs: NativePrefs) -> CmdResult<()> {
    save(&config_dir(&app)?, &prefs).map_err(|error| {
        log::warn!("native prefs: cannot write prefs: {error}");
        CommandError::from(explain(Reason::External, "write native prefs"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("warp-ztna-prefs-{}-{name}", std::process::id()))
    }

    #[test]
    fn missing_file_reads_as_none() {
        let dir = temp_dir("missing");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load(&dir), None);
    }

    #[test]
    fn save_and_load_round_trips() {
        let dir = temp_dir("roundtrip");
        let _ = std::fs::remove_dir_all(&dir);
        let prefs = NativePrefs {
            lang: "en".to_owned(),
            notify: false,
            session_lead_min: 30,
            tray_theme: "light".to_owned(),
        };
        save(&dir, &prefs).expect("save prefs");
        assert_eq!(load(&dir), Some(prefs));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
