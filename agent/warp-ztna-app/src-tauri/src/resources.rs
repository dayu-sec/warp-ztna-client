use std::collections::{BTreeMap, HashSet};

use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::TrayResource;
use crate::i18n;

pub(crate) const TRAY_RESOURCE_LIMIT: usize = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RowSource {
    Ticket,
    Group,
}

#[derive(Clone, Debug)]
pub(crate) struct LivenessRow {
    pub key: String,
    pub address: String,
    pub source: RowSource,
    pub effective: bool,
}

pub(crate) struct ResourceRows {
    pub rows: Vec<TrayResource>,
    pub more: bool,
    pub liveness: Vec<LivenessRow>,
}

fn request_is_effective(request: &Value) -> bool {
    if request.get("status").and_then(Value::as_str) != Some("authorized") {
        return false;
    }
    let Some(expires) = request.get("expires_at").and_then(Value::as_str) else {
        return true;
    };
    let Ok(parsed) = OffsetDateTime::parse(expires, &Rfc3339) else {
        return false;
    };
    parsed > OffsetDateTime::now_utc()
}

fn address_key(value: &str) -> String {
    let normalised = value.trim().to_ascii_lowercase();
    normalised
        .strip_suffix("/32")
        .or_else(|| normalised.strip_suffix("/128"))
        .unwrap_or(&normalised)
        .to_owned()
}

fn target_addresses(target: &Value) -> Vec<String> {
    let mut addresses = Vec::new();
    if let Some(address) = target.get("address").and_then(Value::as_str) {
        let address = address.trim();
        if !address.is_empty() {
            return vec![address.to_owned()];
        }
    }
    if let Some(snapshot) = target.get("addresses_snapshot").and_then(Value::as_array) {
        for address in snapshot {
            if let Some(address) = address.as_str() {
                let address = address.trim();
                if !address.is_empty() && !addresses.iter().any(|value| value == address) {
                    addresses.push(address.to_owned());
                }
            }
        }
    }
    addresses
}

fn ports_text(target: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(ports) = target.get("ports").and_then(Value::as_array) {
        for port in ports {
            if let Some(port) = port.as_i64() {
                parts.push(port.to_string());
            }
        }
    }
    if let Some(ranges) = target.get("port_ranges").and_then(Value::as_array) {
        for range in ranges {
            let start = range.get("start").and_then(Value::as_i64);
            let end = range.get("end").and_then(Value::as_i64);
            if let (Some(start), Some(end)) = (start, end) {
                parts.push(format!("{start}-{end}"));
            }
        }
    }
    parts.join(", ")
}

fn row_key(address: &str, spec: &Value) -> String {
    let protocol =
        spec.get("protocol").and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase();
    format!("{}|{}|{}", address_key(address), protocol, ports_text(spec))
}

pub(crate) fn rows_from_payload(payload: &Value, lang: &str) -> ResourceRows {
    let mut effective_by_address: BTreeMap<String, bool> = BTreeMap::new();
    let mut liveness = Vec::new();
    let mut seen_keys = HashSet::new();
    if let Some(requests) = payload.get("requests").and_then(Value::as_array) {
        for request in requests {
            let effective = request_is_effective(request);
            let Some(targets) = request.get("targets").and_then(Value::as_array) else {
                continue;
            };
            for target in targets {
                for address in target_addresses(target) {
                    let entry = effective_by_address.entry(address.clone()).or_insert(false);
                    *entry |= effective;
                    let key = row_key(&address, target);
                    if seen_keys.insert(key.clone()) {
                        liveness.push(LivenessRow {
                            key,
                            address,
                            source: RowSource::Ticket,
                            effective,
                        });
                    }
                }
            }
        }
    }
    if let Some(grants) = payload.get("group_grants").and_then(Value::as_array) {
        for grant in grants {
            let Some(address) = grant.get("address").and_then(Value::as_str) else {
                continue;
            };
            let address = address.trim();
            if address.is_empty() {
                continue;
            }
            effective_by_address.insert(address.to_owned(), true);
            let key = row_key(address, grant);
            if seen_keys.insert(key.clone()) {
                liveness.push(LivenessRow {
                    key,
                    address: address.to_owned(),
                    source: RowSource::Group,
                    effective: true,
                });
            }
        }
    }
    let more = effective_by_address.len() > TRAY_RESOURCE_LIMIT;
    let rows = effective_by_address
        .into_iter()
        .take(TRAY_RESOURCE_LIMIT)
        .map(|(label, effective)| TrayResource {
            label,
            status: i18n::text(
                lang,
                if effective { "resources.live" } else { "resources.pending" },
            ),
        })
        .collect();
    ResourceRows { rows, more, liveness }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn authorized_unexpired_requests_are_active() {
        let payload = json!({
            "requests": [{
                "status": "authorized",
                "expires_at": "2999-01-01T00:00:00Z",
                "targets": [{ "address": "10.0.0.1" }, { "addresses_snapshot": ["a.example", "b.example"] }]
            }],
            "group_grants": []
        });
        let rows = rows_from_payload(&payload, "zh");
        assert_eq!(rows.rows.len(), 3);
        assert!(rows.rows.iter().all(|row| row.status == "已开通"));
        assert!(!rows.more);
        assert_eq!(rows.liveness.len(), 3);
        assert!(rows.liveness.iter().all(|row| row.effective && row.source == RowSource::Ticket));
    }

    #[test]
    fn expired_or_non_authorized_requests_are_pending() {
        let payload = json!({
            "requests": [
                { "status": "authorized", "expires_at": "2000-01-01T00:00:00Z", "targets": [{ "address": "old.example" }] },
                { "status": "approved", "targets": [{ "address": "pending.example" }] }
            ],
            "group_grants": [{ "address": "group.example", "protocol": "tcp", "ports": [443] }]
        });
        let rows = rows_from_payload(&payload, "en");
        let by_label: std::collections::HashMap<_, _> =
            rows.rows.iter().map(|row| (row.label.as_str(), row.status.as_str())).collect();
        assert_eq!(by_label["old.example"], "Network onboarding");
        assert_eq!(by_label["pending.example"], "Network onboarding");
        assert_eq!(by_label["group.example"], "Active");
        assert!(rows.liveness.iter().any(|row| row.source == RowSource::Group));
    }

    #[test]
    fn duplicate_addresses_merge_and_limit_reports_more() {
        let mut targets = Vec::new();
        for index in 0..20 {
            targets.push(json!({ "address": format!("h{index}.example") }));
        }
        let payload = json!({
            "requests": [
                { "status": "authorized", "targets": targets },
                { "status": "approved", "targets": [{ "address": "h0.example" }] }
            ],
            "group_grants": []
        });
        let rows = rows_from_payload(&payload, "zh");
        assert_eq!(rows.rows.len(), TRAY_RESOURCE_LIMIT);
        assert!(rows.more);
        assert_eq!(rows.rows[0].status, "已开通");
        assert_eq!(rows.liveness.len(), 20);
    }

    #[test]
    fn liveness_keys_include_protocol_and_ports() {
        let payload = json!({
            "requests": [{
                "status": "authorized",
                "targets": [
                    { "address": "host.example", "protocol": "tcp", "ports": [22, 443] },
                    { "address": "host.example", "protocol": "udp", "port_ranges": [{ "start": 1000, "end": 2000 }] }
                ]
            }],
            "group_grants": [{ "address": "host.example", "protocol": "tcp", "ports": [22, 443] }]
        });
        let rows = rows_from_payload(&payload, "zh");
        assert_eq!(rows.liveness.len(), 2);
        let keys: Vec<_> = rows.liveness.iter().map(|row| row.key.as_str()).collect();
        assert!(keys.contains(&"host.example|tcp|22, 443"));
        assert!(keys.contains(&"host.example|udp|1000-2000"));
        assert!(
            rows.liveness
                .iter()
                .any(|row| row.key == "host.example|tcp|22, 443" && row.source == RowSource::Ticket)
        );
    }
}
