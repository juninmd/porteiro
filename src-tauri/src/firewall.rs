//! Outbound IP block through Windows Firewall. It is what cuts connections that were already open
//! (a WhatsApp Web socket, an Instagram tab) when the unlock window ends; names alone cannot do that.
use std::{
    collections::BTreeSet,
    net::{IpAddr, ToSocketAddrs},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

pub const RULE_PREFIX: &str = "Porteiro";
const IPS_PER_RULE: usize = 64;
const MAX_IPS: usize = 2000;

/// Only public unicast addresses may be blocked: a sinkhole answer (0.0.0.0, ::) or a LAN address
/// returned by a captive portal must never end up in a rule that cuts the router or localhost.
pub fn blockable(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => !(v.is_unspecified() || v.is_loopback() || v.is_private() || v.is_link_local() || v.is_multicast() || v.is_broadcast()),
        IpAddr::V6(v) => !(v.is_unspecified() || v.is_loopback() || v.is_multicast() || v.is_unique_local() || v.is_unicast_link_local()),
    }
}

/// Merges fresh answers into the remembered set (newest last), dropping unusable ones and the oldest overflow.
pub fn merge(previous: &[IpAddr], fresh: &[IpAddr]) -> Vec<IpAddr> {
    let mut seen = BTreeSet::new();
    let mut out: Vec<IpAddr> = Vec::new();
    for ip in previous.iter().chain(fresh).filter(|ip| blockable(ip)) {
        if seen.insert(*ip) {
            out.push(*ip);
        } else if fresh.contains(ip) {
            out.retain(|x| x != ip);
            out.push(*ip);
        }
    }
    let skip = out.len().saturating_sub(MAX_IPS);
    out.split_off(skip)
}

pub fn rule_name(index: usize) -> String {
    format!("{RULE_PREFIX}-{index}")
}

pub fn rule_count(ips: &[IpAddr]) -> usize {
    ips.len().div_ceil(IPS_PER_RULE)
}

/// `netsh` arguments, one argv entry each: nothing is ever interpreted by a shell.
pub fn add_args(index: usize, ips: &[IpAddr]) -> Vec<String> {
    let list = ips.iter().map(IpAddr::to_string).collect::<Vec<_>>().join(",");
    ["advfirewall", "firewall", "add", "rule"]
        .iter()
        .map(|s| s.to_string())
        .chain([format!("name={}", rule_name(index)), "dir=out".into(), "action=block".into(), "enable=yes".into(), "profile=any".into(), format!("remoteip={list}")])
        .collect()
}

pub fn delete_args(index: usize) -> Vec<String> {
    ["advfirewall", "firewall", "delete", "rule"].iter().map(|s| s.to_string()).chain([format!("name={}", rule_name(index))]).collect()
}

pub fn show_args(index: usize) -> Vec<String> {
    ["advfirewall", "firewall", "show", "rule"].iter().map(|s| s.to_string()).chain([format!("name={}", rule_name(index))]).collect()
}

pub fn chunks(ips: &[IpAddr]) -> impl Iterator<Item = &[IpAddr]> {
    ips.chunks(IPS_PER_RULE)
}

/// Resolves hostnames through the system resolver with a hard deadline. Lookups that overrun are
/// abandoned (their threads end when the OS answers). Must run while the hosts block is NOT in place.
pub fn resolve(hosts: &[String], budget: Duration) -> Vec<IpAddr> {
    let (tx, rx) = mpsc::channel();
    let deadline = Instant::now() + budget;
    for batch in hosts.chunks(16) {
        for host in batch {
            let (tx, host) = (tx.clone(), host.clone());
            thread::spawn(move || {
                let ips: Vec<IpAddr> = (host.as_str(), 443).to_socket_addrs().map(|a| a.map(|s| s.ip()).collect()).unwrap_or_default();
                let _ = tx.send(ips);
            });
        }
    }
    drop(tx);
    let mut out = Vec::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(ips) => out.extend(ips),
            Err(_) => break,
        }
    }
    out.into_iter().filter(blockable).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn only_public_unicast_addresses_are_blockable() {
        for ok in ["157.240.1.35", "104.244.42.1", "2a03:2880:f12f:83:face:b00c:0:25de"] {
            assert!(blockable(&ip(ok)), "{ok}");
        }
        for bad in ["0.0.0.0", "::", "127.0.0.1", "::1", "192.168.0.1", "10.1.2.3", "172.16.0.1", "169.254.1.1", "fe80::1", "fd00::1", "224.0.0.1", "255.255.255.255"] {
            assert!(!blockable(&ip(bad)), "{bad}");
        }
    }

    #[test]
    fn merge_dedupes_filters_and_bounds_the_remembered_set() {
        let merged = merge(&[ip("1.1.1.1"), ip("192.168.0.1")], &[ip("2.2.2.2"), ip("1.1.1.1"), ip("0.0.0.0")]);
        assert_eq!(merged, vec![ip("2.2.2.2"), ip("1.1.1.1")], "refreshed entries move to the newest end");
        let many: Vec<IpAddr> = (0..2500u32).map(|n| IpAddr::from([20, (n >> 8) as u8, n as u8, 1])).collect();
        let bounded = merge(&many, &[]);
        assert_eq!(bounded.len(), MAX_IPS);
        assert_eq!(bounded.last(), many.last());
    }

    #[test]
    fn rules_are_chunked_and_built_as_separate_arguments() {
        let ips: Vec<IpAddr> = (1..=130u32).map(|n| IpAddr::from([20, 0, 0, n as u8])).collect();
        assert_eq!(rule_count(&ips), 3);
        assert_eq!(chunks(&ips).map(<[IpAddr]>::len).collect::<Vec<_>>(), vec![64, 64, 2]);
        let args = add_args(1, &ips[..2]);
        assert_eq!(&args[..4], ["advfirewall", "firewall", "add", "rule"]);
        assert!(args.contains(&"name=Porteiro-1".to_string()) && args.contains(&"remoteip=20.0.0.1,20.0.0.2".to_string()));
        assert!(args.contains(&"dir=out".to_string()) && args.contains(&"action=block".to_string()));
        assert_eq!(delete_args(0).last().unwrap(), "name=Porteiro-0");
    }

    #[test]
    fn resolve_honours_its_deadline_and_drops_sinkhole_answers() {
        let started = Instant::now();
        let out = resolve(&vec!["localhost".to_string(); 3], Duration::from_millis(1500));
        assert!(out.is_empty(), "loopback is not blockable");
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
