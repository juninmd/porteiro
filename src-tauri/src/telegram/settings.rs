//! What the Telegram integration persists (`telegram.json` in the data dir). The bot token is a secret: it lives
//! only in this file, and the `Debug` impls below keep it out of logs and assertion output.
use serde::{Deserialize, Serialize};
use std::fmt;

/// Which alerts go out and which carry the webcam frame. Screens are never sent: they can show anything.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Alerts {
    pub photo_unlock: bool,
    pub photo_fail: bool,
    pub tamper: bool,
}

impl Default for Alerts {
    fn default() -> Self {
        Alerts { photo_unlock: true, photo_fail: true, tamper: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Chat {
    pub id: i64,
    pub name: String,
}

#[derive(Default, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub token: Option<String>,
    pub bot_username: Option<String>,
    pub chats: Vec<Chat>,
    /// Next `getUpdates` offset: without it a restart would replay everything Telegram still holds.
    pub offset: i64,
    pub alerts: Alerts,
}

impl fmt::Debug for Settings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Settings")
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .field("bot_username", &self.bot_username)
            .field("chats", &self.chats)
            .field("offset", &self.offset)
            .field("alerts", &self.alerts)
            .finish()
    }
}

/// Everything needed to deliver one alert, captured when the event happens so that disconnecting afterwards
/// cannot swallow the alert about the disconnection itself.
#[derive(Clone)]
pub struct Target {
    pub token: String,
    pub chats: Vec<i64>,
    pub alerts: Alerts,
}

impl fmt::Debug for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Target").field("token", &"<redacted>").field("chats", &self.chats).field("alerts", &self.alerts).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_never_contains_the_token() {
        let s = Settings { token: Some("123456:SECRET-SECRET-SECRET-SECRET".into()), ..Settings::default() };
        let t = Target { token: "123456:SECRET-SECRET-SECRET-SECRET".into(), chats: vec![1], alerts: Alerts::default() };
        assert!(!format!("{s:?} {t:?}").contains("SECRET"));
    }

    #[test]
    fn an_old_file_without_newer_fields_still_loads_with_alerts_on() {
        let s: Settings = serde_json::from_str(r#"{"token":null,"chats":[]}"#).unwrap();
        assert_eq!(s.alerts, Alerts::default());
        assert!(s.alerts.photo_unlock && s.alerts.photo_fail && s.alerts.tamper);
    }
}
