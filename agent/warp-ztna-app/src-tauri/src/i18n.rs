use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

const RAW: &str = include_str!("../native-strings.json");

#[derive(Debug, Deserialize)]
struct Bundles {
    zh: HashMap<String, String>,
    en: HashMap<String, String>,
}

static BUNDLES: LazyLock<Bundles> =
    LazyLock::new(|| serde_json::from_str(RAW).expect("native strings must be valid JSON"));

#[derive(Debug, Clone)]
pub(crate) struct TrayLabels {
    pub open: String,
    pub request: String,
    pub resources: String,
    pub connect: String,
    pub disconnect: String,
    pub settings: String,
    pub quit: String,
    pub more: String,
}

pub(crate) fn text(lang: &str, key: &str) -> String {
    let bundle = if lang == "en" { &BUNDLES.en } else { &BUNDLES.zh };
    bundle.get(key).or_else(|| BUNDLES.zh.get(key)).cloned().unwrap_or_else(|| key.to_owned())
}

pub(crate) fn text_with(lang: &str, key: &str, vars: &[(&str, &str)]) -> String {
    let mut value = text(lang, key);
    for (name, replacement) in vars {
        value = value.replace(&format!("{{{name}}}"), replacement);
    }
    value
}

pub(crate) fn tray_labels(lang: &str) -> TrayLabels {
    TrayLabels {
        open: text(lang, "tray.open"),
        request: text(lang, "action.requestAccess"),
        resources: text(lang, "tray.resources"),
        connect: text(lang, "action.connect"),
        disconnect: text(lang, "action.disconnect"),
        settings: text(lang, "nav.settings"),
        quit: text(lang, "tray.quit"),
        more: text(lang, "tray.resourcesMore"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn both_locales_expose_the_same_keys() {
        let zh: BTreeSet<_> = BUNDLES.zh.keys().cloned().collect();
        let en: BTreeSet<_> = BUNDLES.en.keys().cloned().collect();
        assert_eq!(zh, en);
    }

    #[test]
    fn placeholders_are_replaced() {
        assert_eq!(text_with("zh", "resources.livenessUp", &[("name", "x")]), "x 已开通");
        assert_eq!(text_with("en", "resources.livenessUp", &[("name", "x")]), "x is now active");
    }

    #[test]
    fn unknown_language_falls_back_to_chinese() {
        assert_eq!(text("fr", "app.name"), "零通");
    }
}
