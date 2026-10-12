//! `Bot` over HTTPS (ureq with the Windows TLS stack, so the system trust store applies).
//! The token travels in the URL path, which HTTP errors like to echo back: every error text is scrubbed first.
use super::bot::{parse_updates, valid_token, Bot, BotError, Me, Update};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{ffi::OsString, time::Duration};
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

pub const API_BASE: &str = "https://api.telegram.org";
/// Longer than the 25 s long poll, so a quiet poll ends on Telegram's side and not as a timeout here.
const TIMEOUT: Duration = Duration::from_secs(45);
/// A server asking for a longer pause would hold back every later alert, `Quit` included.
const MAX_RETRY_AFTER: u64 = 300;

/// `PORTEIRO_TELEGRAM_API` lets a dev run talk to a local fake. Release builds ignore it: pointing the app at
/// another host would hand it the bot token.
pub fn api_base(get: impl Fn(&str) -> Option<OsString>, allow_overrides: bool) -> String {
    get("PORTEIRO_TELEGRAM_API")
        .filter(|_| allow_overrides)
        .and_then(|v| v.into_string().ok())
        .map(|v| v.trim_end_matches('/').to_string())
        .unwrap_or_else(|| API_BASE.to_string())
}

pub struct HttpBot {
    agent: ureq::Agent,
    base: String,
}

#[derive(Deserialize)]
struct Reply {
    ok: bool,
    #[serde(default)]
    result: Value,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    error_code: Option<u16>,
    #[serde(default)]
    parameters: Option<Parameters>,
}

#[derive(Deserialize)]
struct Parameters {
    retry_after: Option<u64>,
}

pub(super) fn redact(text: &str, token: &str) -> String {
    text.replace(token, "<token>")
}

/// Every ureq error goes through here: its text can carry the URL, and the URL carries the token.
fn scrubber(token: &str) -> impl Fn(ureq::Error) -> BotError + '_ {
    move |e| BotError::Transient(redact(&e.to_string(), token))
}

fn classify(status: u16, body: &str) -> Result<Value, BotError> {
    let Ok(reply) = serde_json::from_str::<Reply>(body) else {
        // An HTML 4xx (proxy, size limit) cannot be fixed by retrying and must not hold up the queue.
        let what = format!("resposta inesperada do Telegram (HTTP {status})");
        // 408, 425 and 429 (a proxy's rate limit) are waits, not refusals.
        return Err(if (400..500).contains(&status) && !matches!(status, 408 | 425 | 429) { BotError::Rejected(what) } else { BotError::Transient(what) });
    };
    if reply.ok {
        return Ok(reply.result);
    }
    let why = reply.description.unwrap_or_default();
    match reply.error_code.unwrap_or(status) {
        401 | 404 => Err(BotError::Unauthorized),
        429 => Err(BotError::RetryAfter(reply.parameters.and_then(|p| p.retry_after).unwrap_or(5).min(MAX_RETRY_AFTER))),
        code if code >= 500 => Err(BotError::Transient(why)),
        _ => Err(BotError::Rejected(why)),
    }
}

/// A hand-built `multipart/form-data` body: the ureq helper for it is marked unstable.
pub(super) fn photo_form(boundary: &str, chat: i64, caption: &str, jpeg: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(jpeg.len() + 512);
    for (name, value) in [("chat_id", chat.to_string()), ("caption", caption.to_string())] {
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
    }
    body.extend_from_slice(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"photo\"; filename=\"foto.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n").as_bytes(),
    );
    body.extend_from_slice(jpeg);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

/// SChannel with the Windows certificate store, where a corporate proxy's own CA lives, as the browsers do.
fn tls_config() -> TlsConfig {
    TlsConfig::builder().provider(TlsProvider::NativeTls).root_certs(RootCerts::PlatformVerifier).build()
}

impl HttpBot {
    pub fn new(base: impl Into<String>) -> Self {
        let config = ureq::Agent::config_builder().tls_config(tls_config()).http_status_as_error(false).timeout_global(Some(TIMEOUT)).user_agent("porteiro").build();
        HttpBot { agent: ureq::Agent::new_with_config(config), base: base.into() }
    }

    fn call(&self, token: &str, method: &str, content_type: &str, body: &[u8]) -> Result<Value, BotError> {
        if !valid_token(token) {
            return Err(BotError::Unauthorized);
        }
        let url = format!("{}/bot{token}/{method}", self.base);
        let scrub = scrubber(token);
        let mut response = self.agent.post(&url).header("content-type", content_type).send(body).map_err(&scrub)?;
        let status = response.status().as_u16();
        let text = response.body_mut().read_to_string().map_err(&scrub)?;
        classify(status, &text)
    }

    fn json(&self, token: &str, method: &str, body: &Value) -> Result<Value, BotError> {
        self.call(token, method, "application/json", body.to_string().as_bytes())
    }
}

impl Bot for HttpBot {
    fn get_me(&self, token: &str) -> Result<Me, BotError> {
        let result = self.json(token, "getMe", &json!({}))?;
        serde_json::from_value(result).map_err(|_| BotError::Transient("resposta inesperada do Telegram".into()))
    }

    fn get_updates(&self, token: &str, offset: i64, timeout_secs: u64) -> Result<Vec<Update>, BotError> {
        let body = json!({ "offset": offset, "timeout": timeout_secs, "allowed_updates": ["message"] });
        Ok(parse_updates(&self.json(token, "getUpdates", &body)?))
    }

    fn send_message(&self, token: &str, chat: i64, text: &str) -> Result<(), BotError> {
        self.json(token, "sendMessage", &json!({ "chat_id": chat, "text": text, "disable_web_page_preview": true })).map(drop)
    }

    fn send_photo(&self, token: &str, chat: i64, caption: &str, jpeg: Vec<u8>) -> Result<(), BotError> {
        let boundary = format!("porteiro{}", uuid::Uuid::new_v4().simple());
        let body = photo_form(&boundary, chat, caption, &jpeg);
        self.call(token, "sendPhoto", &format!("multipart/form-data; boundary={boundary}"), &body).map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telegram_errors_are_sorted_by_what_a_retry_can_fix() {
        let err = |code: u16, extra: &str| classify(code, &format!(r#"{{"ok":false,"error_code":{code},"description":"d"{extra}}}"#)).unwrap_err();
        assert_eq!(err(401, ""), BotError::Unauthorized);
        assert_eq!(err(404, ""), BotError::Unauthorized);
        assert_eq!(err(429, r#","parameters":{"retry_after":17}"#), BotError::RetryAfter(17));
        assert_eq!(err(502, ""), BotError::Transient("d".into()));
        assert_eq!(err(403, ""), BotError::Rejected("d".into()));
        assert!(matches!(classify(200, "<html>proxy</html>"), Err(BotError::Transient(_))));
        assert_eq!(classify(200, r#"{"ok":true,"result":[1]}"#).unwrap(), json!([1]));
    }

    #[test]
    fn an_html_4xx_is_rejected_not_retried_and_a_huge_retry_after_is_capped() {
        assert!(matches!(classify(413, "<html>too large</html>"), Err(BotError::Rejected(_))));
        assert!(matches!(classify(407, ""), Err(BotError::Rejected(_))));
        assert!(matches!(classify(408, ""), Err(BotError::Transient(_))));
        assert!(matches!(classify(502, "<html>bad gateway</html>"), Err(BotError::Transient(_))));
        assert!(matches!(classify(429, "<html>slow down</html>"), Err(BotError::Transient(_))), "a proxy's rate limit must not drop alerts");
        let body = r#"{"ok":false,"error_code":429,"description":"slow down","parameters":{"retry_after":86400}}"#;
        assert_eq!(classify(429, body).unwrap_err(), BotError::RetryAfter(MAX_RETRY_AFTER));
    }

    #[test]
    fn error_text_never_carries_the_token() {
        let token = "123456789:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw";
        let scrubbed = redact(&format!("https://api.telegram.org/bot{token}/getMe: timed out"), token);
        assert!(!scrubbed.contains("AAHdq") && scrubbed.contains("<token>"));
        // The same path every ureq failure takes inside `call`.
        let io = std::io::Error::other(format!("https://api.telegram.org/bot{token}/sendPhoto: connection reset"));
        let BotError::Transient(text) = scrubber(token)(ureq::Error::Io(io)) else { panic!("transient expected") };
        assert!(!text.contains("AAHdq") && text.contains("<token>"), "{text}");
    }

    #[test]
    fn the_windows_certificate_store_is_what_decides_who_is_trusted() {
        assert!(matches!(tls_config().root_certs(), RootCerts::PlatformVerifier));
    }

    #[test]
    fn the_tls_setup_does_not_panic_on_the_first_https_handshake() {
        // Regression: without ureq's `native-tls` feature the first https request panics once the TCP connection
        // is up. A peer that accepts and hangs up gets the client as far as the handshake.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let bot = HttpBot::new(format!("https://127.0.0.1:{}", listener.local_addr().unwrap().port()));
        std::thread::spawn(move || drop(listener.accept()));
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| bot.send_message("123456789:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw", 1, "x")));
        assert!(matches!(outcome, Ok(Err(BotError::Transient(_)))), "{outcome:?}");
    }

    /// Talks to the real Telegram with a made-up token: a 401 proves DNS, TLS and the system trust store work.
    /// Run by hand with `cargo test --lib live_https -- --ignored`.
    #[test]
    #[ignore = "needs internet"]
    fn live_https_reaches_telegram_and_is_refused_for_a_fake_token() {
        let bot = HttpBot::new(API_BASE);
        assert_eq!(bot.get_me("123456789:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw").map(|m| m.username), Err(BotError::Unauthorized));
    }

    #[test]
    fn the_photo_body_is_well_formed_multipart() {
        let body = photo_form("B", 42, "olá\r\n--B", &[0xff, 0xd8, 0x00, 0xff]);
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with("--B\r\nContent-Disposition: form-data; name=\"chat_id\"\r\n\r\n42\r\n"));
        assert!(text.contains("name=\"photo\"; filename=\"foto.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n"));
        assert!(body.windows(4).any(|w| w == [0xff, 0xd8, 0x00, 0xff]) && text.ends_with("\r\n--B--\r\n"));
    }

    #[test]
    fn a_release_build_ignores_the_api_override() {
        let env = |k: &str| (k == "PORTEIRO_TELEGRAM_API").then(|| "http://127.0.0.1:9/".into());
        assert_eq!(api_base(env, false), API_BASE);
        assert_eq!(api_base(env, true), "http://127.0.0.1:9");
        assert_eq!(api_base(|_| None, true), API_BASE);
    }

    #[test]
    fn an_invalid_token_never_reaches_the_network() {
        let bot = HttpBot::new("http://127.0.0.1:1");
        assert_eq!(bot.send_message("../../x", 1, "hi"), Err(BotError::Unauthorized));
    }
}
