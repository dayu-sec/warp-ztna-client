use std::collections::HashMap;

use tauri::{AppHandle, Manager};

use crate::coverage::{is_covered, normalize_routes};
use crate::i18n;
use crate::notify;
use crate::resources::{LivenessRow, RowSource};
use crate::{DesktopState, native_lang};

#[derive(Default)]
pub(crate) struct LivenessState {
    rows: Vec<LivenessRow>,
    previous: Option<HashMap<String, bool>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Flip {
    address: String,
    live: bool,
}

fn flips(
    previous: &HashMap<String, bool>,
    rows: &[LivenessRow],
    live: &HashMap<String, bool>,
) -> Vec<Flip> {
    let mut flips = Vec::new();
    for row in rows {
        let now = live.get(&row.key).copied().unwrap_or(false);
        match previous.get(&row.key) {
            Some(was) if *was != now => {
                flips.push(Flip { address: row.address.clone(), live: now });
            }
            None if now && row.source == RowSource::Ticket => {
                flips.push(Flip { address: row.address.clone(), live: true });
            }
            _ => {}
        }
    }
    flips
}

fn describe(flips: &[Flip], lang: &str) -> Option<String> {
    if flips.is_empty() {
        return None;
    }
    let up = flips.iter().filter(|flip| flip.live).count();
    let down = flips.len() - up;
    if up > 0 && down > 0 {
        let up_text = up.to_string();
        let down_text = down.to_string();
        return Some(i18n::text_with(
            lang,
            "resources.livenessMixed",
            &[("up", up_text.as_str()), ("down", down_text.as_str())],
        ));
    }
    let group: Vec<&Flip> = flips.iter().filter(|flip| flip.live == (up > 0)).collect();
    if group.len() == 1 {
        return Some(i18n::text_with(
            lang,
            if up > 0 { "resources.livenessUp" } else { "resources.livenessDown" },
            &[("name", group[0].address.as_str())],
        ));
    }
    let count = group.len().to_string();
    Some(i18n::text_with(
        lang,
        if up > 0 { "resources.livenessUpMany" } else { "resources.livenessDownMany" },
        &[("count", count.as_str())],
    ))
}

pub(crate) async fn update_rows(app: &AppHandle, rows: Vec<LivenessRow>) {
    if let Some(state) = app.try_state::<DesktopState>() {
        let mut liveness = match state.liveness.lock() {
            Ok(liveness) => liveness,
            Err(poisoned) => poisoned.into_inner(),
        };
        liveness.rows = rows;
        drop(liveness);
    }
    recompute(app).await;
}

pub(crate) async fn recompute(app: &AppHandle) {
    let Some(state) = app.try_state::<DesktopState>() else {
        return;
    };
    let Some(status) = state.hub.latest().await else {
        return;
    };
    let connected = status.state == "connected";
    if !connected {
        let mut liveness = match state.liveness.lock() {
            Ok(liveness) => liveness,
            Err(poisoned) => poisoned.into_inner(),
        };
        liveness.previous = None;
        return;
    }
    let rows = {
        let liveness = match state.liveness.lock() {
            Ok(liveness) => liveness,
            Err(poisoned) => poisoned.into_inner(),
        };
        liveness.rows.clone()
    };
    if rows.is_empty() {
        let mut liveness = match state.liveness.lock() {
            Ok(liveness) => liveness,
            Err(poisoned) => poisoned.into_inner(),
        };
        liveness.previous = None;
        return;
    }
    let coverage = normalize_routes(&status.routes);
    let live: HashMap<String, bool> = rows
        .iter()
        .map(|row| {
            let covered = is_covered(&row.address, &coverage);
            (row.key.clone(), row.effective && connected && covered)
        })
        .collect();
    let flips = {
        let mut liveness = match state.liveness.lock() {
            Ok(liveness) => liveness,
            Err(poisoned) => poisoned.into_inner(),
        };
        let flips = match &liveness.previous {
            Some(previous) => flips(previous, &rows, &live),
            None => Vec::new(),
        };
        liveness.previous = Some(live);
        flips
    };
    if flips.is_empty() {
        return;
    }
    let hidden =
        app.get_webview_window("main").is_none_or(|window| !window.is_visible().unwrap_or(false));
    if !hidden {
        return;
    }
    let lang = native_lang(app);
    if let Some(message) = describe(&flips, &lang) {
        notify::notify(app, &i18n::text(&lang, "app.name"), &message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(key: &str, address: &str, source: RowSource, effective: bool) -> LivenessRow {
        LivenessRow { key: key.to_owned(), address: address.to_owned(), source, effective }
    }

    #[test]
    fn baseline_and_flips_follow_frontend_rules() {
        let rows = vec![
            row("a|tcp|", "a.example", RowSource::Ticket, true),
            row("b|tcp|", "b.example", RowSource::Group, true),
        ];
        let mut previous =
            HashMap::from([("a|tcp|".to_owned(), false), ("b|tcp|".to_owned(), false)]);
        let live = HashMap::from([("a|tcp|".to_owned(), true), ("b|tcp|".to_owned(), true)]);
        let changed = flips(&previous, &rows, &live);
        assert_eq!(changed.len(), 2);
        assert!(changed.iter().all(|flip| flip.live));
        previous.clear();
        let new_rows = vec![
            row("c|tcp|", "c.example", RowSource::Ticket, true),
            row("d|tcp|", "d.example", RowSource::Group, true),
        ];
        let live = HashMap::from([("c|tcp|".to_owned(), true), ("d|tcp|".to_owned(), true)]);
        let added = flips(&HashMap::new(), &new_rows, &live);
        assert_eq!(added, vec![Flip { address: "c.example".to_owned(), live: true }]);
    }

    #[test]
    fn messages_aggregate_counts() {
        let lang = "zh";
        let one = vec![Flip { address: "a.example".to_owned(), live: true }];
        assert_eq!(describe(&one, lang).as_deref(), Some("a.example 已开通"));
        let many = vec![
            Flip { address: "a.example".to_owned(), live: true },
            Flip { address: "b.example".to_owned(), live: true },
        ];
        assert_eq!(describe(&many, lang).as_deref(), Some("2 个资源已开通"));
        let mixed = vec![
            Flip { address: "a.example".to_owned(), live: true },
            Flip { address: "b.example".to_owned(), live: false },
        ];
        assert_eq!(describe(&mixed, lang).as_deref(), Some("1 个资源已开通，1 个已失效"));
        assert_eq!(describe(&[], lang), None);
    }
}
