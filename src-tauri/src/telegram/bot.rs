//! The slice of the Telegram Bot API this app uses, behind a trait so everything else is tested without a network.
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum BotError {
    /// Network trouble, a timeout or a 5xx: worth retrying.
    Transient(String),
    /// 429: Telegram asks for this many seconds of quiet.
    RetryAfter(u64),
    /// 401/404: the token is wrong or was revoked.
    Unauthorized,
    /// Any other 4xx (blocked bot, bad photo): the request itself is wrong, so retrying cannot help.
    Rejected(String),
}

impl BotError {
    pub fn describe(&self) -> String {
        match self {
            BotError::Transient(why) => format!("sem conexão com o Telegram ({why})"),
            BotError::RetryAfter(secs) => format!("o Telegram pediu para aguardar {secs} s"),
            BotError::Unauthorized => "o Telegram recusou o token".into(),
            BotError::Rejected(why) => format!("o Telegram recusou o pedido ({why})"),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Me {
    #[serde(default)]
    pub username: String,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct ChatRef {
    pub id: i64,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
}

impl ChatRef {
    /// Comes from the sender, so it is trimmed and stripped of control characters before it reaches an event.
    pub fn display_name(&self) -> String {
        let raw = self.first_name.as_deref().or(self.username.as_deref()).unwrap_or("sem nome");
        let clean: String = raw.chars().filter(|c| !c.is_control()).take(40).collect();
        if clean.trim().is_empty() { "sem nome".into() } else { clean }
    }
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct Message {
    pub chat: ChatRef,
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, PartialEq)]
pub struct Update {
    pub update_id: i64,
    pub message: Option<Message>,
}

/// One malformed update must not wedge the poll loop on the same batch forever: it is kept without a message
/// (so the offset still moves past it) and an entry without an id is dropped.
pub fn parse_updates(result: &Value) -> Vec<Update> {
    let Some(items) = result.as_array() else { return Vec::new() };
    items
        .iter()
        .filter_map(|item| {
            let update_id = item.get("update_id")?.as_i64()?;
            let message = item.get("message").and_then(|m| serde_json::from_value(m.clone()).ok());
            Some(Update { update_id, message })
        })
        .collect()
}

/// Telegram tokens look like `123456789:AA...`. The token becomes part of the request path, so anything outside
/// that alphabet is refused instead of escaped.
pub fn valid_token(token: &str) -> bool {
    let Some((id, secret)) = token.split_once(':') else { return false };
    (1..=15).contains(&id.len())
        && id.bytes().all(|b| b.is_ascii_digit())
        && (20..=80).contains(&secret.len())
        && secret.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub trait Bot: Send + Sync {
    fn get_me(&self, token: &str) -> Result<Me, BotError>;
    fn get_updates(&self, token: &str, offset: i64, timeout_secs: u64) -> Result<Vec<Update>, BotError>;
    fn send_message(&self, token: &str, chat: i64, text: &str) -> Result<(), BotError>;
    fn send_photo(&self, token: &str, chat: i64, caption: &str, jpeg: Vec<u8>) -> Result<(), BotError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tokens_that_could_bend_the_request_path_are_refused() {
        assert!(valid_token("123456789:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw"));
        for bad in ["", "abc", "123:short", "123456789:AAHdqTcvCH1vGWJxfSeofSAs0K5/../x", "123456789:AAHdqTcvCH1vGWJxfSeof?x=1&y", "x23456789:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw"] {
            assert!(!valid_token(bad), "{bad}");
        }
    }

    #[test]
    fn a_broken_update_does_not_stop_the_offset_from_advancing() {
        let batch = json!([
            {"update_id": 5, "message": {"chat": {"id": 7, "type": "private", "first_name": "Ana"}, "text": "oi"}},
            {"update_id": 6, "message": {"chat": "not-an-object"}},
            {"no_id": true},
            {"update_id": 8, "edited_message": {}}
        ]);
        let updates = parse_updates(&batch);
        assert_eq!(updates.iter().map(|u| u.update_id).collect::<Vec<_>>(), vec![5, 6, 8]);
        assert!(updates[0].message.is_some() && updates[1].message.is_none() && updates[2].message.is_none());
    }

    #[test]
    fn sender_names_are_cleaned_before_they_reach_the_event_log() {
        let chat = ChatRef { id: 1, kind: "private".into(), first_name: Some("A\nna\u{7}".repeat(30)), username: None };
        let name = chat.display_name();
        assert!(name.chars().count() <= 40 && !name.chars().any(char::is_control));
        let blank = ChatRef { id: 1, kind: "private".into(), first_name: Some("\n ".into()), username: None };
        assert_eq!(blank.display_name(), "sem nome");
    }
}
