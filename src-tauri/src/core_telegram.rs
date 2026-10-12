//! Telegram settings on `Core`. Every change needs the password (or Windows Hello): an intruder at the keyboard
//! must not be able to silence or redirect the alerts. Each change is an event, so the chats hear about it, and
//! changes that remove recipients are announced before they take effect.
use crate::{
    api::ApiError,
    auth::Proof,
    core::Core,
    events::Kind,
    telegram::{valid_token, Alerts, View},
};

/// Proof that `Core::verify` accepted the owner. The connect flow checks the password first, then talks to
/// Telegram without holding the core lock, then applies the result.
pub struct Verified(());

fn storage(why: String) -> ApiError {
    ApiError::new("internal", why)
}

fn onoff(on: bool) -> &'static str {
    if on { "ligado" } else { "desligado" }
}

impl Core {
    pub fn telegram_view(&self) -> View {
        self.telegram.view(self.clock.mono())
    }

    pub fn verify(&mut self, proof: impl Into<Proof>, action: &str) -> Result<Verified, ApiError> {
        self.authorize(proof, action)?;
        Ok(Verified(()))
    }

    pub fn telegram_connect(&mut self, _: Verified, token: &str, bot_username: &str) -> Result<View, ApiError> {
        if !valid_token(token) {
            return Err(ApiError::new("telegram_token_invalid", "token inválido: copie-o inteiro do @BotFather"));
        }
        let born = self.born(Kind::ConfigChange, format!("Telegram conectado ao bot @{bot_username}"));
        self.telegram.connect(token, bot_username).map_err(storage)?;
        self.record(born, true);
        Ok(self.telegram_view())
    }

    pub fn telegram_pair(&mut self, proof: impl Into<Proof>) -> Result<View, ApiError> {
        self.authorize(proof, "parear um chat do Telegram")?;
        let born = self.born(Kind::ConfigChange, "pareamento do Telegram aberto: um novo chat pode ser adicionado".to_string());
        self.telegram.start_pairing(self.clock.mono()).map_err(|why| ApiError::new("telegram_not_ready", why))?;
        self.record(born, true);
        Ok(self.telegram_view())
    }

    pub fn telegram_remove_chat(&mut self, proof: impl Into<Proof>, chat: i64) -> Result<View, ApiError> {
        self.authorize(proof, "remover um chat do Telegram")?;
        let name = self.telegram.view(0).chats.into_iter().find(|c| c.id == chat).map(|c| c.name);
        let name = name.ok_or_else(|| ApiError::new("telegram_chat_unknown", "esse chat não está pareado"))?;
        let born = self.born(Kind::ConfigChange, format!("chat removido do Telegram: {name}"));
        self.telegram.remove_chat(chat).map_err(storage)?;
        self.record(born, true);
        Ok(self.telegram_view())
    }

    pub fn telegram_set_alerts(&mut self, proof: impl Into<Proof>, alerts: Alerts) -> Result<View, ApiError> {
        self.authorize(proof, "mudar os alertas do Telegram")?;
        let detail = format!(
            "alertas do Telegram: foto no desbloqueio {}, foto na senha errada {}, violações {}",
            onoff(alerts.photo_unlock),
            onoff(alerts.photo_fail),
            onoff(alerts.tamper)
        );
        let born = self.born(Kind::ConfigChange, detail);
        self.telegram.set_alerts(alerts).map_err(storage)?;
        self.record(born, true);
        Ok(self.telegram_view())
    }

    pub fn telegram_disconnect(&mut self, proof: impl Into<Proof>) -> Result<View, ApiError> {
        self.authorize(proof, "desconectar o Telegram")?;
        let born = self.born(Kind::ConfigChange, "Telegram desconectado: os alertas deixam de sair do PC".to_string());
        self.telegram.disconnect().map_err(storage)?;
        self.record(born, true);
        Ok(self.telegram_view())
    }
}
