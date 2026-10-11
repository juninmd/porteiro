//! What gets blocked: built-in services plus user domains, expanded into the forms each layer needs.
use std::collections::BTreeSet;

pub struct Service {
    pub id: &'static str,
    pub name: &'static str,
    pub domains: &'static [&'static str],
    /// Hostnames worth listing explicitly in the hosts file (it has no wildcards).
    pub extra_hosts: &'static [&'static str],
    /// Desktop executables that are killed while locked.
    pub apps: &'static [&'static str],
}

pub const SERVICES: &[Service] = &[
    Service {
        id: "instagram",
        name: "Instagram",
        domains: &["instagram.com"],
        extra_hosts: &["i.instagram.com", "graph.instagram.com", "edge-chat.instagram.com"],
        apps: &[],
    },
    Service {
        id: "facebook",
        name: "Facebook",
        domains: &["facebook.com", "fb.com", "fb.me", "messenger.com"],
        extra_hosts: &[
            "l.facebook.com",
            "lm.facebook.com",
            "graph.facebook.com",
            "edge-chat.facebook.com",
            "web.facebook.com",
            "mbasic.facebook.com",
            "business.facebook.com",
            "pt-br.facebook.com",
        ],
        apps: &[],
    },
    Service {
        id: "twitter",
        name: "Twitter (X)",
        domains: &["x.com", "twitter.com", "t.co"],
        extra_hosts: &["api.x.com", "api.twitter.com", "mobile.twitter.com", "mobile.x.com"],
        apps: &[],
    },
    Service {
        id: "whatsapp",
        name: "WhatsApp",
        domains: &["whatsapp.com", "whatsapp.net", "wa.me"],
        extra_hosts: &[
            "web.whatsapp.com",
            "chat.whatsapp.com",
            "api.whatsapp.com",
            "g.whatsapp.net",
            "c.whatsapp.net",
            "v.whatsapp.net",
            "mmg.whatsapp.net",
            "mmi.whatsapp.net",
            "pps.whatsapp.net",
            "media.whatsapp.net",
            "static.whatsapp.net",
        ],
        apps: &["WhatsApp.exe", "WhatsApp.Root.exe"],
    },
];

const PREFIXES: &[&str] = &["", "www.", "m.", "mobile.", "web.", "api."];

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Blocklist {
    /// Apex domains: browser policies match these and every subdomain.
    pub domains: Vec<String>,
    /// Explicit hostnames for the hosts file and the IP resolver.
    pub hosts: Vec<String>,
    pub apps: Vec<String>,
}

pub fn default_enabled() -> Vec<String> {
    SERVICES.iter().map(|s| s.id.to_string()).collect()
}

pub fn build(enabled: &[String], custom: &[String]) -> Blocklist {
    let (mut domains, mut named, mut apps) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    for s in SERVICES.iter().filter(|s| enabled.iter().any(|e| e == s.id)) {
        domains.extend(s.domains.iter().map(|d| d.to_string()));
        named.extend(s.extra_hosts.iter().map(|h| h.to_string()));
        apps.extend(s.apps.iter().map(|a| a.to_string()));
        if s.id == "whatsapp" {
            named.extend((1..=16).map(|n| format!("e{n}.whatsapp.net")));
        }
    }
    domains.extend(custom.iter().cloned());
    let hosts: BTreeSet<String> = domains
        .iter()
        .flat_map(|d| PREFIXES.iter().map(move |p| format!("{p}{d}")))
        .chain(named)
        .collect();
    Blocklist { domains: domains.into_iter().collect(), hosts: hosts.into_iter().collect(), apps: apps.into_iter().collect() }
}

/// Normalizes user input into a bare lowercase domain. Everything written to hosts, the registry and
/// firewall arguments goes through here, so only letters, digits, dots and hyphens ever get through.
pub fn validate_domain(input: &str) -> Result<String, &'static str> {
    let mut s = input.trim().to_ascii_lowercase();
    if let Some(i) = s.find("://") {
        s = s[i + 3..].to_string();
    }
    s.truncate(s.find(['/', '?', '#']).unwrap_or(s.len()));
    let s = s.trim_matches('.');
    let s = s.strip_prefix("www.").unwrap_or(s);
    if s.is_empty() {
        return Err("domain_empty");
    }
    if s.len() > 253 {
        return Err("domain_too_long");
    }
    let labels: Vec<&str> = s.split('.').collect();
    let label_ok = |l: &&str| {
        (1..=63).contains(&l.len())
            && l.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !l.starts_with('-')
            && !l.ends_with('-')
    };
    if !labels.iter().all(label_ok) {
        return Err("domain_invalid");
    }
    if labels.len() < 2 || labels.last().is_some_and(|t| t.bytes().all(|b| b.is_ascii_digit())) {
        return Err("domain_invalid");
    }
    Ok(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_cover_the_four_requested_services() {
        let bl = build(&default_enabled(), &[]);
        for d in ["instagram.com", "facebook.com", "x.com", "twitter.com", "whatsapp.com"] {
            assert!(bl.domains.iter().any(|x| x == d), "{d} missing");
        }
        assert!(bl.apps.contains(&"WhatsApp.exe".to_string()));
    }

    #[test]
    fn hosts_expand_prefixes_and_whatsapp_edges_without_duplicates() {
        let bl = build(&default_enabled(), &["reddit.com".into()]);
        for h in ["www.instagram.com", "web.whatsapp.com", "e16.whatsapp.net", "mbasic.facebook.com", "www.reddit.com"] {
            assert!(bl.hosts.iter().any(|x| x == h), "{h} missing");
        }
        let unique: BTreeSet<_> = bl.hosts.iter().collect();
        assert_eq!(unique.len(), bl.hosts.len());
    }

    #[test]
    fn disabled_services_are_not_blocked() {
        let bl = build(&["instagram".into()], &[]);
        assert!(bl.domains.iter().all(|d| d.contains("instagram")));
        assert!(bl.apps.is_empty());
    }

    #[test]
    fn urls_are_reduced_to_a_domain() {
        assert_eq!(validate_domain(" HTTPS://www.Reddit.com/r/rust?x=1 ").as_deref(), Ok("reddit.com"));
        assert_eq!(validate_domain("news.ycombinator.com").as_deref(), Ok("news.ycombinator.com"));
    }

    #[test]
    fn hostile_or_malformed_domains_are_rejected() {
        for bad in [
            "", "com", "a b.com", "evil.com\n0.0.0.0 bank.com", "a.com # x", "-a.com", "a-.com", "a..b.com",
            "127.0.0.1", "1.2.3.4", "user@host.com", "host.com:8080", "münchen.de", "a.com\"; calc", "x$(id).com",
        ] {
            assert!(validate_domain(bad).is_err(), "{bad:?} was accepted");
        }
        assert_eq!(validate_domain(&format!("{}.com", "a".repeat(64))), Err("domain_invalid"));
        assert_eq!(validate_domain(&format!("{}.com", "a.".repeat(130))), Err("domain_too_long"));
    }
}
