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
    Service {
        id: "tiktok",
        name: "TikTok",
        domains: &["tiktok.com"],
        extra_hosts: &["www.tiktok.com", "v16-webapp-prime.tiktok.com", "api.tiktokv.com"],
        apps: &[],
    },
    Service {
        id: "reddit",
        name: "Reddit",
        domains: &["reddit.com", "redd.it"],
        extra_hosts: &["www.reddit.com", "old.reddit.com", "oauth.reddit.com"],
        apps: &[],
    },
    Service {
        id: "youtube",
        name: "YouTube",
        domains: &["youtube.com", "youtu.be"],
        extra_hosts: &["www.youtube.com", "m.youtube.com", "i.ytimg.com"],
        apps: &[],
    },
    Service {
        id: "bets",
        name: "Apostas / Bets",
        domains: &["bet365.com", "betano.com", "blaze.com", "sportingbet.com", "betfair.com", "kto.com", "estrela.bet", "stake.com"],
        extra_hosts: &["www.bet365.com", "br.betano.com", "blaze.com"],
        apps: &[],
    },
    Service {
        id: "discord",
        name: "Discord",
        domains: &["discord.com", "discord.gg", "discordapp.com"],
        extra_hosts: &["gateway.discord.gg", "cdn.discordapp.com"],
        apps: &["Discord.exe"],
    },
];

pub const DOH_HOSTS: &[&str] = &[
    "cloudflare-dns.com",
    "dns.google",
    "dns.quad9.net",
    "doh.opendns.com",
    "dns.adguard-dns.com",
    "dns.nextdns.io",
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

pub const DEFAULT_ENABLED_IDS: &[&str] = &["instagram", "facebook", "twitter", "whatsapp"];

pub fn default_enabled() -> Vec<String> {
    DEFAULT_ENABLED_IDS.iter().map(|s| s.to_string()).collect()
}

pub fn build(enabled: &[String], custom: &[String], custom_apps: &[String]) -> Blocklist {
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
    apps.extend(custom_apps.iter().cloned());
    named.extend(DOH_HOSTS.iter().map(|h| h.to_string()));
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

/// Normalizes and validates desktop application executable names.
pub fn validate_app(input: &str) -> Result<String, &'static str> {
    let s = input.trim();
    if s.is_empty() {
        return Err("app_empty");
    }
    if s.contains('/') || s.contains('\\') || s.contains(':') {
        return Err("app_invalid");
    }
    let lower = s.to_ascii_lowercase();
    let name = if lower.ends_with(".exe") {
        s.to_string()
    } else {
        format!("{s}.exe")
    };
    if name.len() > 64 {
        return Err("app_too_long");
    }
    let stem = &name[..name.len() - 4];
    if stem.is_empty()
        || !stem
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b' ')
    {
        return Err("app_invalid");
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_cover_the_four_requested_services() {
        let bl = build(&default_enabled(), &[], &[]);
        for d in ["instagram.com", "facebook.com", "x.com", "twitter.com", "whatsapp.com"] {
            assert!(bl.domains.iter().any(|x| x == d), "{d} missing");
        }
        assert!(bl.apps.contains(&"WhatsApp.exe".to_string()));
    }

    #[test]
    fn hosts_expand_prefixes_and_whatsapp_edges_without_duplicates() {
        let bl = build(&default_enabled(), &["reddit.com".into()], &[]);
        for h in ["www.instagram.com", "web.whatsapp.com", "e16.whatsapp.net", "mbasic.facebook.com", "www.reddit.com"] {
            assert!(bl.hosts.iter().any(|x| x == h), "{h} missing");
        }
        let unique: BTreeSet<_> = bl.hosts.iter().collect();
        assert_eq!(unique.len(), bl.hosts.len());
    }

    #[test]
    fn disabled_services_are_not_blocked() {
        let bl = build(&["instagram".into()], &[], &[]);
        assert!(bl.domains.iter().all(|d| d.contains("instagram")));
        assert!(bl.apps.is_empty());
    }

    #[test]
    fn custom_apps_are_included() {
        let bl = build(&[], &[], &["Discord.exe".into(), "Spotify.exe".into()]);
        assert_eq!(bl.apps, vec!["Discord.exe", "Spotify.exe"]);
    }

    #[test]
    fn validate_app_handles_extensions_and_rejects_malformed() {
        assert_eq!(validate_app("discord").as_deref(), Ok("discord.exe"));
        assert_eq!(validate_app("Discord.exe").as_deref(), Ok("Discord.exe"));
        assert_eq!(validate_app("my-app_1.0.exe").as_deref(), Ok("my-app_1.0.exe"));
        assert_eq!(validate_app("").err(), Some("app_empty"));
        assert_eq!(validate_app("c:\\bad.exe").err(), Some("app_invalid"));
        assert_eq!(validate_app("../bad.exe").err(), Some("app_invalid"));
        assert_eq!(validate_app(".exe").err(), Some("app_invalid"));
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
