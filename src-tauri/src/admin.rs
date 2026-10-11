//! Configuration changes. Every one needs the password: weakening the block must be a deliberate act,
//! and each is recorded with evidence.
use crate::{
    auth,
    blocklist::{self, SERVICES},
    config::{DEFAULT_PASSWORD, MAX_UNLOCK_MINUTES},
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

impl Core {
    fn changed(&mut self, detail: String) -> Result<Status, ApiError> {
        self.save_config()?;
        self.log(Kind::ConfigChange, detail, true);
        self.tick();
        Ok(self.status())
    }

    pub fn add_domain(&mut self, password: &str, input: &str) -> Result<Status, ApiError> {
        let domain = blocklist::validate_domain(input).map_err(domain_error)?;
        self.authorize(password, "adicionar um site")?;
        let covered = SERVICES.iter().any(|s| self.cfg.enabled_services.iter().any(|e| e == s.id) && s.domains.contains(&domain.as_str()));
        if !covered && !self.cfg.custom_domains.contains(&domain) {
            self.cfg.custom_domains.push(domain.clone());
        }
        self.changed(format!("site adicionado: {domain}"))
    }

    pub fn remove_domain(&mut self, password: &str, input: &str) -> Result<Status, ApiError> {
        self.authorize(password, "remover um site")?;
        self.cfg.custom_domains.retain(|d| d != input);
        self.changed(format!("site removido: {input}"))
    }

    pub fn set_service(&mut self, password: &str, id: &str, enabled: bool) -> Result<Status, ApiError> {
        let service = SERVICES.iter().find(|s| s.id == id).ok_or_else(|| ApiError::new("unknown_service", "serviço desconhecido"))?;
        self.authorize(password, "alterar um serviço")?;
        self.cfg.enabled_services.retain(|e| e != id);
        if enabled {
            self.cfg.enabled_services.push(id.to_string());
        }
        self.changed(format!("{} {}", service.name, if enabled { "bloqueado" } else { "liberado em definitivo" }))
    }

    pub fn set_unlock_minutes(&mut self, password: &str, minutes: u32) -> Result<Status, ApiError> {
        if !(1..=MAX_UNLOCK_MINUTES).contains(&minutes) {
            return Err(ApiError::new("minutes_out_of_range", format!("use de 1 a {MAX_UNLOCK_MINUTES} minutos")));
        }
        self.authorize(password, "mudar a duração da liberação")?;
        self.cfg.unlock_minutes = minutes;
        self.changed(format!("duração da liberação: {minutes} min"))
    }

    pub fn change_password(&mut self, current: &str, next: &str) -> Result<Status, ApiError> {
        auth::validate_new_password(next).map_err(|code| {
            ApiError::new(code, if code == "password_too_short" { "use ao menos 4 caracteres" } else { "senha longa demais" })
        })?;
        self.authorize(current, "trocar a senha")?;
        self.cfg.password_hash = auth::hash(next).map_err(|e| ApiError::new("internal", e))?;
        self.cfg.default_password = next == DEFAULT_PASSWORD;
        self.save_config()?;
        self.log(Kind::PasswordChange, "senha alterada", true);
        Ok(self.status())
    }

    /// The only way out: lifts every layer, removes the restart task and records it. Caller exits the process.
    pub fn disable_and_quit(&mut self, password: &str) -> Result<(), ApiError> {
        self.authorize(password, "desativar a proteção")?;
        self.log(Kind::Quit, "proteção desativada e app encerrado", true);
        self.begin_disable();
        self.reapply(false);
        if self.watchdog_managed() {
            crate::watchdog::remove().map_err(|e| ApiError::new("internal", format!("não foi possível remover a tarefa de reinício: {e}")))?;
        }
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
    fn password_change_works_clears_the_default_flag_and_rejects_short_ones() {
        let mut h = harness();
        assert!(h.core.status().default_password);
        assert_eq!(h.core.change_password("1406", "123").unwrap_err().code, "password_too_short");
        h.core.change_password("1406", "senha-forte").unwrap();
        assert!(!h.core.status().default_password);
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
    fn minutes_are_bounded() {
        let mut h = harness();
        assert_eq!(h.core.set_unlock_minutes("1406", 0).unwrap_err().code, "minutes_out_of_range");
        assert_eq!(h.core.set_unlock_minutes("1406", 241).unwrap_err().code, "minutes_out_of_range");
        assert_eq!(h.core.set_unlock_minutes("1406", 30).unwrap().unlock_minutes, 30);
    }
}
