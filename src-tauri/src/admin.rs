//! Configuration changes. Every one needs the password: weakening the block must be a deliberate act,
//! and each is recorded with evidence.
use crate::{
    auth::{self, Proof},
    blocklist::{self, SERVICES},
    config::MAX_UNLOCK_MINUTES,
    api::{ApiError, Status},
    core::Core,
    events::Kind,
};

fn domain_error(code: &'static str) -> ApiError {
    let message = match code {
        "domain_empty" => "informe um domínio",
        "domain_too_long" => "domínio longo demais",
        _ => "domínio inválido: use só letras, números, pontos e hífens (ex.: reddit.com)",
    };
    ApiError::new(code, message)
}

fn app_error(code: &'static str) -> ApiError {
    let message = match code {
        "app_empty" => "informe o nome do aplicativo",
        "app_too_long" => "nome do aplicativo longo demais",
        _ => "nome de aplicativo inválido (ex.: Discord.exe)",
    };
    ApiError::new(code, message)
}

impl Core {
    fn changed(&mut self, detail: String) -> Result<Status, ApiError> {
        self.save_config()?;
        self.log(Kind::ConfigChange, detail, true);
        self.tick();
        Ok(self.status())
    }

    pub fn add_domain(&mut self, proof: impl Into<Proof>, input: &str) -> Result<Status, ApiError> {
        let domain = blocklist::validate_domain(input).map_err(domain_error)?;
        self.authorize(proof, "adicionar um site")?;
        let covered = SERVICES.iter().any(|s| self.cfg.enabled_services.iter().any(|e| e == s.id) && s.domains.contains(&domain.as_str()));
        if !covered && !self.cfg.custom_domains.contains(&domain) {
            self.cfg.custom_domains.push(domain.clone());
        }
        self.changed(format!("site adicionado: {domain}"))
    }

    pub fn remove_domain(&mut self, proof: impl Into<Proof>, input: &str) -> Result<Status, ApiError> {
        self.authorize(proof, "remover um site")?;
        self.cfg.custom_domains.retain(|d| d != input);
        self.changed(format!("site removido: {input}"))
    }

    pub fn add_app(&mut self, proof: impl Into<Proof>, input: &str) -> Result<Status, ApiError> {
        let app = blocklist::validate_app(input).map_err(app_error)?;
        self.authorize(proof, "adicionar um aplicativo")?;
        if !self.cfg.custom_apps.iter().any(|a| a.eq_ignore_ascii_case(&app)) {
            self.cfg.custom_apps.push(app.clone());
        }
        self.changed(format!("aplicativo adicionado: {app}"))
    }

    pub fn remove_app(&mut self, proof: impl Into<Proof>, input: &str) -> Result<Status, ApiError> {
        self.authorize(proof, "remover um aplicativo")?;
        self.cfg.custom_apps.retain(|a| !a.eq_ignore_ascii_case(input));
        self.changed(format!("aplicativo removido: {input}"))
    }

    pub fn set_schedule(&mut self, proof: impl Into<Proof>, schedule: Option<crate::config::ScheduleConfig>) -> Result<Status, ApiError> {
        self.authorize(proof, "alterar o agendamento")?;
        let detail = match &schedule {
            Some(s) if s.enabled => format!("agendamento ativado: {:02}h às {:02}h", s.start_hour, s.end_hour),
            _ => "agendamento desativado".to_string(),
        };
        self.cfg.schedule = schedule;
        self.changed(detail)
    }

    pub fn set_service(&mut self, proof: impl Into<Proof>, id: &str, enabled: bool) -> Result<Status, ApiError> {
        let service = SERVICES.iter().find(|s| s.id == id).ok_or_else(|| ApiError::new("unknown_service", "serviço desconhecido"))?;
        self.authorize(proof, "alterar um serviço")?;
        self.cfg.enabled_services.retain(|e| e != id);
        if enabled {
            self.cfg.enabled_services.push(id.to_string());
        }
        self.changed(format!("{} {}", service.name, if enabled { "bloqueado" } else { "liberado em definitivo" }))
    }

    pub fn set_unlock_minutes(&mut self, proof: impl Into<Proof>, minutes: u32) -> Result<Status, ApiError> {
        if !(1..=MAX_UNLOCK_MINUTES).contains(&minutes) {
            return Err(ApiError::new("minutes_out_of_range", format!("use de 1 a {MAX_UNLOCK_MINUTES} minutos")));
        }
        self.authorize(proof, "mudar a duração da liberação")?;
        self.cfg.unlock_minutes = minutes;
        self.changed(format!("duração da liberação: {minutes} min"))
    }

    /// Turning Hello on or off. The command layer has already had Windows Hello confirm itself when enabling.
    pub fn set_hello(&mut self, proof: impl Into<Proof>, enabled: bool) -> Result<Status, ApiError> {
        if enabled && !self.hello_available {
            return Err(ApiError::new("hello_unavailable", "o Windows Hello não está disponível neste PC"));
        }
        self.authorize(proof, "alterar o Windows Hello")?;
        self.cfg.hello = enabled;
        self.changed(format!("Windows Hello {}", if enabled { "ativado" } else { "desativado" }))
    }

    pub fn change_password(&mut self, current: &str, next: &str) -> Result<Status, ApiError> {
        auth::validate_new_password(next).map_err(|code| {
            ApiError::new(code, if code == "password_too_short" { "use ao menos 4 caracteres" } else { "senha longa demais" })
        })?;
        self.authorize(current, "trocar a senha")?;
        self.cfg.password_hash = Some(auth::hash(next).map_err(|e| ApiError::new("internal", e))?);
        self.save_config()?;
        self.log(Kind::PasswordChange, "senha alterada", true);
        Ok(self.status())
    }

    /// The only way out: lifts every layer, removes the restart task and records it. Caller exits the process.
    pub fn disable_and_quit(&mut self, proof: impl Into<Proof>) -> Result<(), ApiError> {
        self.authorize(proof, "desativar a proteção")?;
        // Task first: if it cannot go, nothing else was touched and the block stays whole.
        if self.watchdog_managed() {
            if let Some(task) = self.restarter.as_mut() {
                task.remove().map_err(|e| ApiError::new("internal", format!("não foi possível remover a tarefa de reinício: {e}")))?;
            }
        }
        self.log(Kind::Quit, "proteção desativada e app encerrado", true);
        self.begin_disable();
        self.reapply(false);
        self.flush_evidence();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::*;

    #[test]
    fn config_changes_need_the_password_and_apply_on_the_next_tick() {
        let mut h = harness();
        assert!(h.core.add_domain("wrong", "reddit.com").is_err());
        let status = h.core.add_domain("1406", "https://www.Reddit.com/r/rust").unwrap();
        assert_eq!(status.custom_domains, vec!["reddit.com"]);
        assert!(h.hosts_text().contains("0.0.0.0 reddit.com"), "applied without waiting for a tick");
        h.core.remove_domain("1406", "reddit.com").unwrap();
        assert!(!h.hosts_text().contains("reddit.com"));
    }

    #[test]
    fn disabling_a_service_needs_the_password_and_lifts_only_that_service() {
        let mut h = harness();
        assert_eq!(h.core.set_service("nope", "instagram", false).unwrap_err().code, "wrong_password");
        assert!(h.hosts_text().contains("instagram.com"));
        h.core.set_service("1406", "instagram", false).unwrap();
        let hosts = h.hosts_text();
        assert!(!hosts.contains("instagram.com") && hosts.contains("facebook.com"));
        assert_eq!(h.core.set_service("1406", "bogus", true).unwrap_err().code, "unknown_service");
    }

    #[test]
    fn password_change_works_and_rejects_short_ones() {
        let mut h = harness();
        assert_eq!(h.core.change_password("1406", "123").unwrap_err().code, "password_too_short");
        h.core.change_password("1406", "senha-forte").unwrap();
        assert_eq!(h.core.unlock("1406", None).unwrap_err().code, "wrong_password", "the old password is dead");
        assert!(h.core.unlock("senha-forte", None).is_ok());
    }

    #[test]
    fn disable_and_quit_lifts_the_block_and_is_recorded() {
        let mut h = harness();
        assert!(h.core.disable_and_quit("0000").is_err());
        assert!(h.hosts_text().contains("instagram.com"), "a wrong password changes nothing");
        h.core.disable_and_quit("1406").unwrap();
        assert!(!h.hosts_text().contains("instagram.com"));
        h.core.flush_evidence();
        assert!(h.kinds().contains(&crate::events::Kind::Quit));
    }

    #[test]
    fn a_restart_task_that_cannot_be_removed_leaves_the_block_whole() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = harness();
        h.task.fail_remove.store(true, SeqCst);
        assert_eq!(h.core.disable_and_quit("1406").unwrap_err().code, "internal");
        assert!(h.hosts_text().contains("instagram.com"), "the block must survive a failed quit");
        h.core.tick();
        assert!(h.core.status().locked, "ticks keep enforcing after the failed attempt");
        h.task.fail_remove.store(false, SeqCst);
        h.core.disable_and_quit("1406").unwrap();
        assert!(!h.hosts_text().contains("instagram.com") && h.task.removed.load(SeqCst));
    }

    #[test]
    fn there_is_no_default_password_and_nothing_works_before_the_first_one_is_set() {
        let mut h = unconfigured_harness();
        let status = h.core.status();
        assert!(status.needs_setup && status.locked);
        assert!(h.hosts_text().contains("instagram.com"), "the block is up while waiting for setup");
        for guess in ["1406", "", "0000", "admin"] {
            assert_eq!(h.core.unlock(guess, None).unwrap_err().code, "setup_required", "{guess:?} must not unlock");
        }
        assert_eq!(h.core.add_domain("1406", "reddit.com").unwrap_err().code, "setup_required");
        assert_eq!(h.core.disable_and_quit("1406").unwrap_err().code, "setup_required");
        assert_eq!(h.core.setup_password("123").unwrap_err().code, "password_too_short");
        let status = h.core.setup_password("minha-senha").unwrap();
        assert!(!status.needs_setup && status.locked, "setting a password does not unlock");
        assert_eq!(h.core.unlock("1406", None).unwrap_err().code, "wrong_password");
        assert!(h.core.unlock("minha-senha", None).is_ok());
    }

    #[test]
    fn setup_is_refused_once_a_password_exists() {
        let mut h = harness();
        assert_eq!(h.core.setup_password("outra-senha").unwrap_err().code, "already_configured");
        assert!(h.core.unlock("1406", None).is_ok(), "the existing password is untouched");
    }

    #[test]
    fn the_password_chosen_at_setup_survives_a_restart() {
        let mut h = unconfigured_harness();
        h.core.setup_password("minha-senha").unwrap();
        let mut h = h.restart();
        assert!(!h.core.status().needs_setup);
        assert!(h.core.unlock("minha-senha", None).is_ok());
    }

    #[test]
    fn hello_stands_in_for_the_password_only_after_the_owner_turned_it_on() {
        use crate::auth::Proof;
        let mut h = harness();
        assert_eq!(h.core.unlock(Proof::Hello, None).unwrap_err().code, "hello_disabled");
        assert!(h.core.status().locked);
        assert_eq!(h.core.set_hello("wrong", true).unwrap_err().code, "wrong_password");
        assert!(h.core.set_hello("1406", true).unwrap().hello_enabled);
        assert!(!h.core.unlock(Proof::Hello, Some(5)).unwrap().locked);
        assert!(h.core.set_hello(Proof::Hello, false).is_ok());
        assert_eq!(h.core.unlock(Proof::Hello, None).unwrap_err().code, "hello_disabled");
    }

    #[test]
    fn a_hello_refusal_is_a_failed_attempt_with_a_photo_and_counts_toward_the_lockout() {
        use crate::auth::{Proof, FREE_ATTEMPTS};
        let mut h = harness();
        h.core.set_hello("1406", true).unwrap();
        for _ in 0..FREE_ATTEMPTS {
            assert!(h.core.precheck(&Proof::Hello).is_ok());
            let err = h.core.hello_failed("liberar o acesso", "rosto não reconhecido");
            assert!(matches!(err.code, "hello_failed" | "locked_out"));
        }
        assert_eq!(h.core.precheck(&Proof::Hello).unwrap_err().code, "locked_out", "Hello is locked out like the password");
        assert_eq!(h.core.unlock("1406", None).unwrap_err().code, "locked_out", "and so is the password");
        assert!(h.kinds().contains(&crate::events::Kind::AuthFail));
        assert!(h.events().iter().any(|e| e.kind == crate::events::Kind::AuthFail && !e.shots.is_empty()));
    }

    #[test]
    fn hello_cannot_be_enabled_where_windows_does_not_offer_it() {
        let mut h = harness();
        h.core.hello_available = false;
        assert_eq!(h.core.set_hello("1406", true).unwrap_err().code, "hello_unavailable");
        assert!(!h.core.status().hello_enabled);
    }

    #[test]
    fn minutes_are_bounded() {
        let mut h = harness();
        assert_eq!(h.core.set_unlock_minutes("1406", 0).unwrap_err().code, "minutes_out_of_range");
        assert_eq!(h.core.set_unlock_minutes("1406", 241).unwrap_err().code, "minutes_out_of_range");
        assert_eq!(h.core.set_unlock_minutes("1406", 30).unwrap().unlock_minutes, 30);
    }

    #[test]
    fn custom_apps_and_schedule_can_be_managed() {
        use crate::config::ScheduleConfig;
        let mut h = harness();
        assert!(h.core.add_app("wrong", "Discord.exe").is_err());
        let st = h.core.add_app("1406", "discord").unwrap();
        assert_eq!(st.custom_apps, vec!["discord.exe"]);
        let st = h.core.remove_app("1406", "discord.exe").unwrap();
        assert!(st.custom_apps.is_empty());

        let sched = ScheduleConfig { enabled: true, start_hour: 9, end_hour: 18, days: vec![1, 2, 3, 4, 5] };
        let st = h.core.set_schedule("1406", Some(sched)).unwrap();
        assert!(st.schedule.as_ref().unwrap().enabled);
        let st = h.core.set_schedule("1406", None).unwrap();
        assert!(st.schedule.is_none());
    }
}
