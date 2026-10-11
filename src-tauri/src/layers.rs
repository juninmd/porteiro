//! The enforcement layers behind one trait. Order matters when locking: the firewall resolves names
//! first (needs a working resolver), then hosts and browser policies cut the names off.
use crate::{blocklist::Blocklist, config::write_json, firewall, hosts, policy};
use std::{
    net::IpAddr,
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};

pub trait Layer: Send {
    fn name(&self) -> &'static str;
    fn needs_admin(&self) -> bool {
        true
    }
    /// Expensive to verify (spawns processes): the drift check runs it less often.
    fn slow(&self) -> bool {
        false
    }
    fn apply(&mut self, bl: &Blocklist, locked: bool) -> Result<(), String>;
    fn in_state(&mut self, bl: &Blocklist, locked: bool) -> Result<bool, String>;
}

pub trait AppKiller: Send {
    /// Terminates running processes whose executable name matches; returns the names it ended.
    fn kill(&mut self, names: &[String]) -> Vec<String>;
}

fn quiet(program: &str, args: &[String]) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("{program}: {e}"))?;
    status.success().then_some(()).ok_or_else(|| format!("{program} saiu com {status}"))
}

pub struct HostsLayer {
    pub path: PathBuf,
    pub sandboxed: bool,
}

impl Layer for HostsLayer {
    fn name(&self) -> &'static str {
        "hosts"
    }
    fn needs_admin(&self) -> bool {
        !self.sandboxed
    }
    fn apply(&mut self, bl: &Blocklist, locked: bool) -> Result<(), String> {
        let changed = hosts::apply(&self.path, &bl.hosts, locked).map_err(|e| e.to_string())?;
        if changed && !self.sandboxed {
            // Drop cached answers so a name blocked now stops resolving immediately.
            let _ = quiet("ipconfig", &["/flushdns".to_string()]);
        }
        Ok(())
    }
    fn in_state(&mut self, bl: &Blocklist, locked: bool) -> Result<bool, String> {
        hosts::is_in_state(&self.path, &bl.hosts, locked).map_err(|e| e.to_string())
    }
}

#[cfg(windows)]
pub struct PolicyLayer(pub winreg::RegKey);

#[cfg(windows)]
impl Layer for PolicyLayer {
    fn name(&self) -> &'static str {
        "políticas dos navegadores"
    }
    fn apply(&mut self, bl: &Blocklist, locked: bool) -> Result<(), String> {
        let plan = policy::plan(&bl.domains);
        let result = if locked { crate::policy_reg::apply(&self.0, &plan) } else { crate::policy_reg::clear(&self.0, &plan) };
        result.map_err(|e| e.to_string())
    }
    fn in_state(&mut self, bl: &Blocklist, locked: bool) -> Result<bool, String> {
        Ok(if locked {
            crate::policy_reg::is_applied(&self.0, &policy::plan(&bl.domains))
        } else {
            crate::policy_reg::is_cleared(&self.0, &policy::plan(&bl.domains))
        })
    }
}

pub struct FirewallLayer {
    ips: Vec<IpAddr>,
    store: PathBuf,
    netsh: fn(&[String]) -> Result<(), String>,
}

fn netsh(args: &[String]) -> Result<(), String> {
    quiet("netsh", args)
}

impl FirewallLayer {
    pub fn new(store: PathBuf) -> Self {
        let ips = std::fs::read(&store).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        FirewallLayer { ips, store, netsh }
    }

    #[cfg(test)]
    fn with_netsh(store: PathBuf, netsh: fn(&[String]) -> Result<(), String>) -> Self {
        FirewallLayer { netsh, ..FirewallLayer::new(store) }
    }

    fn delete_all(&self) {
        // Sweep past the known count too: rules from a previous, larger IP set may still exist.
        for i in 0..(firewall::rule_count(&self.ips) + 8).max(8) {
            let _ = (self.netsh)(&firewall::delete_args(i));
        }
    }
}

impl Layer for FirewallLayer {
    fn name(&self) -> &'static str {
        "firewall"
    }
    fn slow(&self) -> bool {
        true
    }
    fn apply(&mut self, bl: &Blocklist, locked: bool) -> Result<(), String> {
        self.delete_all();
        if !locked {
            return Ok(());
        }
        let fresh = firewall::resolve(&bl.hosts, Duration::from_secs(4));
        self.ips = firewall::merge(&self.ips, &fresh);
        write_json(&self.store, &self.ips).map_err(|e| e.to_string())?;
        for (i, chunk) in firewall::chunks(&self.ips).enumerate() {
            (self.netsh)(&firewall::add_args(i, chunk))?;
        }
        Ok(())
    }
    fn in_state(&mut self, _bl: &Blocklist, locked: bool) -> Result<bool, String> {
        let present = |i| (self.netsh)(&firewall::show_args(i)).is_ok();
        Ok(if locked { (0..firewall::rule_count(&self.ips)).all(present) } else { !present(0) })
    }
}

pub struct SysinfoKiller;

impl AppKiller for SysinfoKiller {
    fn kill(&mut self, names: &[String]) -> Vec<String> {
        if names.is_empty() {
            return Vec::new();
        }
        let mut sys = sysinfo::System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        sys.processes()
            .values()
            .filter_map(|p| {
                let name = p.name().to_string_lossy().into_owned();
                (names.iter().any(|n| n.eq_ignore_ascii_case(&name)) && p.kill()).then_some(name)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static CALLS: Mutex<Vec<String>> = Mutex::new(Vec::new());

    fn record(args: &[String]) -> Result<(), String> {
        CALLS.lock().unwrap().push(args[2..].join(" "));
        Ok(())
    }

    #[test]
    fn unlocking_deletes_every_rule_and_adds_none() {
        let dir = tempfile::tempdir().unwrap();
        CALLS.lock().unwrap().clear();
        let mut layer = FirewallLayer::with_netsh(dir.path().join("ips.json"), record);
        layer.apply(&Blocklist::default(), false).unwrap();
        let calls = CALLS.lock().unwrap().clone();
        assert!(calls.iter().all(|c| c.starts_with("delete rule name=Porteiro-")) && calls.len() >= 8);
    }

    #[test]
    fn hosts_layer_blocks_and_reports_drift_without_admin_when_sandboxed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts");
        let mut layer = HostsLayer { path: path.clone(), sandboxed: true };
        let bl = Blocklist { hosts: vec!["instagram.com".into()], ..Default::default() };
        assert!(!layer.needs_admin());
        layer.apply(&bl, true).unwrap();
        assert!(layer.in_state(&bl, true).unwrap());
        std::fs::write(&path, "127.0.0.1 localhost\n").unwrap();
        assert!(!layer.in_state(&bl, true).unwrap(), "someone removed the block");
        layer.apply(&bl, false).unwrap();
        assert!(layer.in_state(&bl, false).unwrap());
    }

    #[test]
    fn killer_ignores_an_empty_list_and_unknown_names() {
        assert!(SysinfoKiller.kill(&[]).is_empty());
        assert!(SysinfoKiller.kill(&["definitely-not-running-xyz.exe".to_string()]).is_empty());
    }
}
