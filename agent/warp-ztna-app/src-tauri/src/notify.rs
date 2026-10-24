use std::time::Duration;

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::i18n;
use crate::prefs;
use crate::{DesktopState, native_lang};

fn native_prefs(app: &AppHandle) -> prefs::NativePrefs {
    app.path().app_config_dir().ok().as_deref().and_then(prefs::load).unwrap_or_default()
}

pub(crate) fn notify(app: &AppHandle, title: &str, body: &str) {
    let prefs = native_prefs(app);
    if !prefs.notify {
        return;
    }
    if let Err(error) = app.notification().builder().title(title).body(body).show() {
        log::warn!("notification: {error}");
    }
}

pub(crate) fn notify_connection(app: &AppHandle) {
    let lang = native_lang(app);
    notify(app, &i18n::text(&lang, "app.name"), &i18n::text(&lang, "toast.connected"));
}

pub(crate) async fn is_connected(app: &AppHandle) -> bool {
    match app.try_state::<DesktopState>() {
        Some(state) => state.hub.latest().await.is_some_and(|status| status.state == "connected"),
        None => false,
    }
}

fn session_warning_due(
    now: i64,
    expires: i64,
    lead_min: u32,
    dismissed: Option<i64>,
    notified: Option<i64>,
) -> bool {
    if expires <= now {
        return false;
    }
    let lead = i64::from(lead_min.clamp(1, 59)) * 60;
    if expires - now > lead {
        return false;
    }
    dismissed != Some(expires) && notified != Some(expires)
}

pub(crate) async fn run_session_watch(app: AppHandle) {
    let mut notified: Option<i64> = None;
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;
        crate::liveness::recompute(&app).await;
        let Some(state) = app.try_state::<DesktopState>() else {
            return;
        };
        let Some(status) = state.hub.latest().await else {
            continue;
        };
        if status.state != "connected" {
            continue;
        }
        let Some(expires) = status.session_expires_at else {
            continue;
        };
        let prefs = native_prefs(&app);
        if !prefs.notify {
            continue;
        }
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let dismissed = match state.dismissed_expiry.lock() {
            Ok(slot) => *slot,
            Err(poisoned) => *poisoned.into_inner(),
        };
        if !session_warning_due(now, expires, prefs.session_lead_min, dismissed, notified) {
            continue;
        }
        notify(
            &app,
            &i18n::text(&prefs.lang, "session.title"),
            &i18n::text(&prefs.lang, "session.body"),
        );
        notified = Some(expires);
    }
}

#[cfg(test)]
mod tests {
    use super::session_warning_due;

    #[test]
    fn warns_inside_lead_once_and_respects_dismissal() {
        let now = 1_000;
        let expires = now + 15 * 60;
        assert!(session_warning_due(now, expires, 15, None, None));
        assert!(!session_warning_due(now, expires + 3_600, 15, None, None));
        assert!(!session_warning_due(now, expires, 15, Some(expires), None));
        assert!(!session_warning_due(now, expires, 15, None, Some(expires)));
        assert!(!session_warning_due(now, now, 15, None, None));
    }
}
