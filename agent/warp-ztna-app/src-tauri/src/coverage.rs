use std::net::IpAddr;
use std::str::FromStr;

use ipnet::IpNet;

const DOMAIN_SENTINEL: &str = "192.0.2.0/32";

pub(crate) struct Coverage {
    nets: Vec<IpNet>,
    domains: Vec<String>,
}

fn parse_net(entry: &str) -> Option<IpNet> {
    if let Ok(net) = IpNet::from_str(entry) {
        return Some(net);
    }
    let address = IpAddr::from_str(entry).ok()?;
    let prefix = if address.is_ipv4() { 32 } else { 128 };
    IpNet::new(address, prefix).ok()
}

pub(crate) fn normalize_routes(routes: &[String]) -> Coverage {
    let mut nets = Vec::new();
    let mut domains = Vec::new();
    for route in routes {
        for part in route.split(',') {
            let entry = part.trim().to_ascii_lowercase();
            if entry.is_empty() || entry == DOMAIN_SENTINEL {
                continue;
            }
            match parse_net(&entry) {
                Some(net) => nets.push(net),
                None => domains.push(entry),
            }
        }
    }
    Coverage { nets, domains }
}

fn covers_domain(destination: &str, route: &str) -> bool {
    let name = destination.strip_prefix("*.").unwrap_or(destination);
    if let Some(base) = route.strip_prefix("*.") {
        name == base || name.ends_with(&format!(".{base}"))
    } else {
        name == route
    }
}

pub(crate) fn is_covered(address: &str, coverage: &Coverage) -> bool {
    let target = address.trim().to_ascii_lowercase();
    if target.is_empty() {
        return false;
    }
    if let Some(net) = parse_net(&target) {
        return coverage.nets.iter().any(|route| route.contains(&net));
    }
    coverage.domains.iter().any(|route| covers_domain(&target, route))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_and_ipv6_cidrs_are_covered() {
        let coverage = normalize_routes(&["10.0.0.0/8, fd00::/8, 192.168.1.5".to_owned()]);
        assert!(is_covered("10.1.2.3", &coverage));
        assert!(is_covered("fd00::1", &coverage));
        assert!(is_covered("192.168.1.5", &coverage));
        assert!(!is_covered("11.0.0.1", &coverage));
    }

    #[test]
    fn wildcard_domains_match_subdomains() {
        let coverage = normalize_routes(&["*.example.com, exact.example".to_owned()]);
        assert!(is_covered("a.example.com", &coverage));
        assert!(is_covered("example.com", &coverage));
        assert!(is_covered("exact.example", &coverage));
        assert!(!is_covered("badexample.com", &coverage));
        assert!(!is_covered("other.example", &coverage));
    }

    #[test]
    fn domain_sentinel_is_ignored() {
        let coverage = normalize_routes(&["192.0.2.0/32".to_owned()]);
        assert!(!is_covered("192.0.2.0", &coverage));
        assert!(coverage.nets.is_empty() && coverage.domains.is_empty());
    }
}
