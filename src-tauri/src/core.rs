//! The state machine: unlock windows, authorization, enforcement ticks and the event trail.
//! Everything touching the OS is injected, so the rules here are tested with fakes.
use crate::{
    api::{ApiError, Clock, LayerStatus, ServiceInfo, Status},
    auth::{self, Proof, Throttle},
    blocklist::{self, Blocklist},
    config::{self, Config, Loaded, Paths, State, MAX_UNLOCK_MINUTES},
    events::{Event, EventLog, Kind},
    capture::Capturer,
    evidence::Evidence,
    layers::{AppKiller, Layer},
    session::Transition,
    watchdog::Restarter,
};
use std::collections::HashMap;

/// A clock that restarts this far behind the last reading inside an unlock window was moved, not corrected.
const CLOCK_SLACK_SECS: i64 = 120;
/// How often the open window records the wall clock; bounds how far a rollback can still go unnoticed.
pub(crate) const HEARTBEAT_SECS: i64 = 15;

pub struct Deps {
    pub paths: Paths,
    pub layers: Vec<Box<dyn Layer>>,
    /// The core builds the evidence worker itself so its event handle and the core's always share one lock.
    pub capturer: Box<dyn Capturer>,
    pub killer: Box<dyn AppKiller>,
    pub clock: Box<dyn Clock>,
    pub elevated: bool,
    /// The restart task; `Some` only for release builds outside any sandbox.
    pub restarter: Option<Box<dyn Restarter>>,
    /// Whether Windows Hello can be offered on this machine (read once at start).
    pub hello_available: bool,
}

pub struct Core {
    pub(crate) cfg: Config,
    pub(crate) st: State,
    pub(crate) paths: Paths,
    pub(crate) events: EventLog,
    pub(crate) evidence: Evidence,
    pub(crate) clock: Box<dyn Clock>,
    pub(crate) throttle: Throttle,
    pub(crate) applied: Option<bool>,
    pub(crate) layers: Vec<Box<dyn Layer>>,
    pub(crate) killer: Box<dyn AppKiller>,
    pub(crate) elevated: bool,
    pub(crate) deadline: Option<u64>,
    pub(crate) report: Vec<LayerStatus>,
    pub(crate) retry_at: u64,
    pub(crate) last_slow_check: u64,
    pub(crate) last_blocked_app: HashMap<String, u64>,
    pub(crate) last_enforcement_error: Option<String>,
    pub(crate) restarter: Option<Box<dyn Restarter>>,
    pub(crate) hello_available: bool,
    /// Set once the user chose to disable protection: ticks stop re-locking while the process winds down.
    pub(crate) disabled: bool,
    pub(crate) watchdog_ok: Option<bool>,
    pub(crate) watchdog_checked: Option<u64>,
}

impl Core {
    pub fn new(deps: Deps) -> Result<Core, String> {
        let Deps { paths, layers, capturer, killer, clock, elevated, restarter, hello_available } = deps;
        if let Err(e) = config::prepare_data_dir(&paths, elevated) {
            eprintln!("porteiro: could not prepare {}: {e}", paths.data.display());
        }
        let cfg = config::load_or_create(&paths.config(), Config::fresh)?;
        let cfg_reset = matches!(cfg, Loaded::Reset(_));
        let st = config::load_or_create(&paths.state(), || Ok(State::default()))?.into_inner();
        let (mono, wall) = (clock.mono(), clock.wall());
        let events = EventLog::new(paths.events(), 5000);
        let evidence = Evidence::spawn(capturer, events.sibling(), paths.shots());
        let mut core = Core {
            throttle: Throttle::restore(st.failures, mono),
            cfg: cfg.into_inner(),
            st,
            events,
            evidence,
            clock,
            applied: None,
            layers,
            killer,
            elevated,
            deadline: None,
            report: Vec::new(),
            retry_at: 0,
            last_slow_check: 0,
            last_blocked_app: HashMap::new(),
            last_enforcement_error: None,
            restarter,
            hello_available,
            disabled: false,
            watchdog_ok: None,
            watchdog_checked: None,
            paths,
        };
        if let Ok(dropped) = core.events.prune() {
            crate::capture::remove(&core.paths.shots(), &dropped);
        }
        core.restore_window(mono, wall);
        core.log(Kind::AppStart, if elevated { "app iniciado" } else { "app iniciado SEM administrador: o bloqueio não é aplicado" }, false);
        if cfg_reset {
            core.log(Kind::Tamper, "config.json ilegível: restaurada sem senha (nova configuração necessária)", false);
        }
        core.tick();
        Ok(core)
    }

    /// An unlock window survives a restart only while it is plausible: a deadline further out than the maximum
    /// window, or a clock behind the last reading taken inside the window, means the clock was moved, and the
    /// safe answer is "locked".
    fn restore_window(&mut self, mono: u64, wall: i64) {
        let Some(until) = self.st.unlocked_until else { return };
        let remaining = until - wall;
        let moved_back = self.st.seen_wall.is_some_and(|seen| wall + CLOCK_SLACK_SECS < seen);
        if remaining > 0 && remaining <= i64::from(MAX_UNLOCK_MINUTES) * 60 + 60 && !moved_back {
            self.deadline = Some(mono + remaining as u64);
        } else {
            if remaining > 0 {
                self.log(Kind::Tamper, "janela de liberação fora do limite (relógio alterado?): bloqueado", false);
            }
            self.st.seen_wall = None;
            self.st.unlocked_until = None;
            self.save_state();
        }
    }

    pub fn blocklist(&self) -> Blocklist {
        blocklist::build(&self.cfg.enabled_services, &self.cfg.custom_domains)
    }

    fn remaining(&self) -> Option<u64> {
        let now = self.clock.mono();
        self.deadline.filter(|d| now < *d).map(|d| d - now)
    }

    pub fn status(&self) -> Status {
        let now = self.clock.mono();
        Status {
            locked: self.remaining().is_none(),
            unlock_remaining_secs: self.remaining(),
            lockout_secs: self.throttle.check(now).err(),
            elevated: self.elevated,
            needs_setup: self.cfg.password_hash.is_none(),
            hello_available: self.hello_available,
            hello_enabled: self.cfg.hello,
            layers: self.report.clone(),
            services: blocklist::SERVICES
                .iter()
                .map(|s| ServiceInfo { id: s.id, name: s.name, enabled: self.cfg.enabled_services.iter().any(|e| e == s.id), domains: s.domains.to_vec() })
                .collect(),
            custom_domains: self.cfg.custom_domains.clone(),
            unlock_minutes: self.cfg.unlock_minutes,
            watchdog: self.watchdog_ok,
            event_count: self.events.count(),
            data_dir: self.paths.data.display().to_string(),
        }
    }

    pub fn list_events(&self, offset: usize, limit: usize) -> Vec<Event> {
        self.events.list(offset, limit)
    }

    pub fn shots_dir(&self) -> std::path::PathBuf {
        self.paths.shots()
    }

    /// Records an event. With `capture`, screen and webcam evidence is attached by the worker thread.
    pub(crate) fn log(&mut self, kind: Kind, detail: impl Into<String>, capture: bool) {
        let event = Event { id: uuid::Uuid::new_v4().to_string(), at_ms: self.clock.wall_ms(), kind, detail: detail.into(), shots: Vec::new() };
        let event = if capture {
            match self.evidence.submit(event) {
                Ok(()) => return,
                Err(back) => back,
            }
        } else {
            event
        };
        if let Err(e) = self.events.append(&event) {
            eprintln!("porteiro: could not record event: {e}");
        }
    }

    pub(crate) fn save_state(&mut self) {
        if let Err(e) = config::write_json(&self.paths.state(), &self.st) {
            eprintln!("porteiro: could not save state: {e}");
        }
    }

    pub(crate) fn save_config(&mut self) -> Result<(), ApiError> {
        config::write_json(&self.paths.config(), &self.cfg).map_err(|e| ApiError::new("internal", format!("não foi possível salvar a configuração: {e}")))?;
        // The desired state changed; the next tick must apply it rather than read the difference as tampering.
        self.applied = None;
        self.retry_at = 0;
        Ok(())
    }

    /// Checks the lockout and that the proof can work at all, without consuming an attempt. The command layer
    /// calls it before it opens a Windows Hello prompt.
    pub fn precheck(&self, proof: &Proof) -> Result<(), ApiError> {
        if self.cfg.password_hash.is_none() {
            return Err(ApiError::new("setup_required", "defina uma senha antes de usar o Porteiro"));
        }
        if matches!(proof, Proof::Hello) && !self.cfg.hello {
            return Err(ApiError::new("hello_disabled", "o Windows Hello não está ativado no Porteiro"));
        }
        if let Err(wait) = self.throttle.check(self.clock.mono()) {
            let mut e = ApiError::new("locked_out", format!("muitas tentativas: aguarde {wait} s"));
            e.retry_after = Some(wait);
            return Err(e);
        }
        Ok(())
    }

    /// Verifies the proof under the lockout policy. Failed attempts are logged with evidence.
    pub(crate) fn authorize(&mut self, proof: impl Into<Proof>, action: &str) -> Result<(), ApiError> {
        let proof = proof.into();
        self.precheck(&proof)?;
        let ok = match &proof {
            Proof::Password(p) => self.cfg.password_hash.as_deref().is_some_and(|h| auth::verify(h, p)),
            Proof::Hello => true,
        };
        if ok {
            self.throttle.succeed();
            self.st.failures = 0;
            self.save_state();
            return Ok(());
        }
        Err(self.failed_attempt(action, "senha incorreta", "wrong_password"))
    }

    /// Windows Hello said no: the same trail and lockout as a wrong password.
    pub fn hello_failed(&mut self, action: &str, why: &str) -> ApiError {
        self.failed_attempt(action, &format!("Windows Hello recusou ({why})"), "hello_failed")
    }

    fn failed_attempt(&mut self, action: &str, what: &str, code: &'static str) -> ApiError {
        let now = self.clock.mono();
        let started_lockout = self.throttle.fail(now);
        self.st.failures = self.throttle.failures();
        self.save_state();
        let left = self.throttle.attempts_left();
        self.log(Kind::AuthFail, format!("{what} ao {action} ({left} tentativas restantes)"), true);
        let mut e = ApiError::new(code, what.to_string());
        e.attempts_left = Some(left);
        if let Some(wait) = started_lockout {
            self.log(Kind::Lockout, format!("{} falhas seguidas: bloqueado por {wait} s", self.throttle.failures()), false);
            e.code = "locked_out";
            e.message = format!("muitas tentativas: aguarde {wait} s");
            e.retry_after = Some(wait);
        }
        e
    }

    /// First run only: there is no default password, so the owner chooses one before anything can be unlocked.
    pub fn setup_password(&mut self, password: &str) -> Result<Status, ApiError> {
        if self.cfg.password_hash.is_some() {
            return Err(ApiError::new("already_configured", "já existe uma senha"));
        }
        auth::validate_new_password(password).map_err(|code| {
            ApiError::new(code, if code == "password_too_short" { "use ao menos 4 caracteres" } else { "senha longa demais" })
        })?;
        self.cfg.password_hash = Some(auth::hash(password).map_err(|e| ApiError::new("internal", e))?);
        self.save_config()?;
        self.log(Kind::PasswordChange, "senha definida", true);
        Ok(self.status())
    }

    pub fn unlock(&mut self, proof: impl Into<Proof>, minutes: Option<u32>) -> Result<Status, ApiError> {
        self.authorize(proof, "liberar o acesso")?;
        let minutes = minutes.unwrap_or(self.cfg.unlock_minutes).clamp(1, MAX_UNLOCK_MINUTES);
        self.deadline = Some(self.clock.mono() + u64::from(minutes) * 60);
        self.st.unlocked_until = Some(self.clock.wall() + i64::from(minutes) * 60);
        self.st.seen_wall = Some(self.clock.wall());
        self.save_state();
        self.reapply(false);
        self.log(Kind::UnlockOk, format!("acesso liberado por {minutes} min"), true);
        Ok(self.status())
    }

    /// Locking never needs a password.
    pub fn relock(&mut self) -> Status {
        if self.deadline.take().is_some() {
            self.st.unlocked_until = None;
            self.save_state();
            self.log(Kind::Relock, "bloqueio reativado manualmente", false);
            self.applied = None;
            self.retry_at = 0;
            self.tick();
        }
        self.status()
    }

    pub fn session_event(&mut self, t: Transition) {
        match t {
            Transition::Unlocked => self.log(Kind::SessionUnlock, "Windows desbloqueado", true),
            Transition::Locked => self.log(Kind::SessionLock, "Windows bloqueado", false),
        }
    }

    pub(crate) fn begin_disable(&mut self) {
        self.disabled = true;
        self.deadline = None;
        self.st.unlocked_until = None;
        self.save_state();
    }

    pub(crate) fn watchdog_managed(&self) -> bool {
        self.restarter.is_some() && self.elevated
    }

    /// Everything queued for capture is on disk before the app lets go of the process.
    pub fn flush_evidence(&self) {
        self.evidence.flush();
    }
}
