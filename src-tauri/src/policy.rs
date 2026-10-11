//! Browser enterprise policies: wildcard URL blocking that works with DNS-over-HTTPS and open connections.
//! This module only plans; `policy_reg` writes the plan to the registry.

#[derive(Debug, Clone, PartialEq)]
pub enum Data {
    Str(String),
    Dword(u32),
}

#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    /// (registry key under HKLM, ordered entries stored as values "1", "2", ...)
    pub lists: Vec<(String, Vec<String>)>,
    /// (registry key under HKLM, value name, data)
    pub values: Vec<(String, String, Data)>,
}

const CHROMIUM_VENDORS: &[&str] = &["Google\\Chrome", "Microsoft\\Edge", "BraveSoftware\\Brave"];
const FIREFOX: &str = "SOFTWARE\\Policies\\Mozilla\\Firefox";

/// The state wanted while locked. `domains` are apex domains; each browser policy covers their subdomains.
pub fn plan(domains: &[String]) -> Plan {
    let mut p = Plan::default();
    for vendor in CHROMIUM_VENDORS {
        let root = format!("SOFTWARE\\Policies\\{vendor}");
        // "example.com" in URLBlocklist matches the domain and all of its subdomains.
        p.lists.push((format!("{root}\\URLBlocklist"), domains.to_vec()));
        // Without this, a browser resolving over HTTPS ignores the hosts file.
        p.values.push((root, "DnsOverHttpsMode".into(), Data::Str("off".into())));
    }
    p.lists.push((
        format!("{FIREFOX}\\WebsiteFilter\\Block"),
        domains.iter().map(|d| format!("*://*.{d}/*")).collect(),
    ));
    let doh = format!("{FIREFOX}\\DNSOverHTTPS");
    p.values.push((doh.clone(), "Enabled".into(), Data::Dword(0)));
    p.values.push((doh, "Locked".into(), Data::Dword(1)));
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn domains() -> Vec<String> {
        vec!["instagram.com".into(), "x.com".into()]
    }

    #[test]
    fn chromium_browsers_get_a_url_blocklist_and_doh_off() {
        let p = plan(&domains());
        for vendor in ["Google\\Chrome", "Microsoft\\Edge", "BraveSoftware\\Brave"] {
            let list = p.lists.iter().find(|(k, _)| k == &format!("SOFTWARE\\Policies\\{vendor}\\URLBlocklist")).unwrap();
            assert_eq!(list.1, domains());
            assert!(p.values.contains(&(format!("SOFTWARE\\Policies\\{vendor}"), "DnsOverHttpsMode".into(), Data::Str("off".into()))));
        }
    }

    #[test]
    fn firefox_gets_wildcard_patterns_and_a_locked_doh_policy() {
        let p = plan(&domains());
        let block = p.lists.iter().find(|(k, _)| k.ends_with("Mozilla\\Firefox\\WebsiteFilter\\Block")).unwrap();
        assert_eq!(block.1, vec!["*://*.instagram.com/*", "*://*.x.com/*"]);
        let doh = |n: &str| p.values.iter().find(|(k, name, _)| k.ends_with("DNSOverHTTPS") && name == n).map(|v| v.2.clone());
        assert_eq!((doh("Enabled"), doh("Locked")), (Some(Data::Dword(0)), Some(Data::Dword(1))));
    }

    #[test]
    fn firefox_patterns_never_contain_anything_but_the_validated_domain() {
        let p = plan(&["a-b.example.com".to_string()]);
        assert!(p.lists.iter().flat_map(|(_, v)| v).all(|e| e.chars().all(|c| c.is_ascii_alphanumeric() || "*:/.-".contains(c))));
    }
}
