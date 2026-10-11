//! Periodic enforcement: expiry of unlock windows, drift detection, applying the layers in order.
use crate::{api::LayerStatus, core::Core, events::Kind};

impl Core {
    pub fn tick(&mut self) {
        if self.disabled {
            return;
        }
        let now = self.clock.mono();
        if self.deadline.is_some_and(|d| now >= d) {
            self.deadline = None;
            self.st.unlocked_until = None;
            self.save_state();
            self.log(Kind::Relock, "a janela de liberação expirou", false);
            self.applied = None;
        }
        let locked = self.deadline.is_none();
        if self.applied == Some(locked) {
            let slow = now.saturating_sub(self.last_slow_check) >= 30;
            let drift = self.drifted(locked, slow);
            if slow {
                self.last_slow_check = now;
            }
            if !drift.is_empty() {
                if locked {
                    self.log(Kind::Tamper, format!("bloqueio alterado fora do app: {}", drift.join(", ")), true);
                }
                self.applied = None;
            }
        }
        if self.applied != Some(locked) && now >= self.retry_at {
            self.reapply(locked);
        }
        if locked {
            self.enforce_apps(now);
        }
        self.refresh_watchdog(now);
    }

    fn drifted(&mut self, locked: bool, include_slow: bool) -> Vec<String> {
        let bl = self.blocklist();
        let elevated = self.elevated;
        self.layers
            .iter_mut()
            .filter(|l| (include_slow || !l.slow()) && (elevated || !l.needs_admin()))
            .filter_map(|l| match l.in_state(&bl, locked) {
                Ok(true) => None,
                Ok(false) => Some(l.name().to_string()),
                Err(e) => Some(format!("{} ({e})", l.name())),
            })
            .collect()
    }

    /// Locking runs the layers in order (resolve names, then cut them); unlocking in reverse.
    pub(crate) fn reapply(&mut self, locked: bool) {
        let bl = self.blocklist();
        let n = self.layers.len();
        let order: Vec<usize> = if locked { (0..n).collect() } else { (0..n).rev().collect() };
        let mut report: Vec<Option<LayerStatus>> = vec![None; n];
        for i in order {
            let layer = &mut self.layers[i];
            let name = layer.name().to_string();
            let outcome = if layer.needs_admin() && !self.elevated {
                Err("requer administrador".to_string())
            } else {
                layer.apply(&bl, locked)
            };
            report[i] = Some(match outcome {
                Ok(()) => LayerStatus { name, ok: true, error: None },
                Err(error) => LayerStatus { name, ok: false, error: Some(error) },
            });
        }
        self.report = report.into_iter().flatten().collect();
        let failed: Vec<String> = self.report.iter().filter(|l| !l.ok).map(|l| format!("{}: {}", l.name, l.error.clone().unwrap_or_default())).collect();
        if failed.is_empty() {
            self.applied = Some(locked);
            self.last_enforcement_error = None;
        } else {
            self.applied = None;
            self.retry_at = self.clock.mono() + 30;
            let summary = failed.join("; ");
            if self.last_enforcement_error.as_deref() != Some(summary.as_str()) {
                self.log(Kind::EnforcementError, summary.clone(), false);
                self.last_enforcement_error = Some(summary);
            }
        }
    }

    fn enforce_apps(&mut self, now: u64) {
        if !self.elevated && !self.paths.sandboxed {
            return;
        }
        let apps = self.blocklist().apps;
        for name in self.killer.kill(&apps) {
            let key = name.to_ascii_lowercase();
            let recent = self.last_blocked_app.get(&key).is_some_and(|t| now.saturating_sub(*t) < 60);
            self.last_blocked_app.insert(key, now);
            if !recent {
                self.log(Kind::BlockedApp, format!("{name} encerrado enquanto bloqueado"), false);
            }
        }
    }

    fn refresh_watchdog(&mut self, now: u64) {
        if !self.manage_watchdog || !self.elevated || self.watchdog_checked.is_some_and(|t| now.saturating_sub(t) < 60) {
            return;
        }
        self.watchdog_checked = Some(now);
        let exe = std::env::current_exe().ok();
        // Re-registered on every check so a moved install or a deleted task heals itself.
        let ok = exe.is_some_and(|exe| crate::watchdog::install(&exe, &self.paths.data).is_ok());
        if self.watchdog_ok == Some(true) && !ok {
            self.log(Kind::EnforcementError, "não foi possível registrar a tarefa de reinício", false);
        }
        self.watchdog_ok = Some(ok);
    }
}
