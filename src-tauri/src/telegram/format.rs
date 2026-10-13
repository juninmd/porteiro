//! Message texts. Plain text on purpose: event details carry arbitrary characters, and plain text needs no escaping.
use super::settings::Alerts;
use crate::{
    api::Status,
    events::{Event, Kind},
};

/// Telegram caps a photo caption at 1024 characters; the same bound keeps every message short.
const MAX_CHARS: usize = 1000;
/// An alert that waited this long (offline, retries) says when it really happened.
const LATE_SECS: i64 = 90;

pub struct Alert {
    pub text: String,
    /// Whether the webcam frame should go along, when the event has one.
    pub photo: bool,
}

#[derive(Clone, Copy)]
enum Class {
    Unlock,
    Fail,
    /// Violations of the block: the owner can silence these.
    Alarm,
    /// Changes to protection or to who is watching: always announced, or silencing the channel would be silent.
    Change,
}

fn class(kind: Kind) -> Option<Class> {
    match kind {
        Kind::UnlockOk | Kind::SessionUnlock => Some(Class::Unlock),
        Kind::AuthFail => Some(Class::Fail),
        Kind::Lockout | Kind::Tamper | Kind::EnforcementError | Kind::AppStart => Some(Class::Alarm),
        Kind::PasswordChange | Kind::ConfigChange | Kind::Quit => Some(Class::Change),
        Kind::Relock | Kind::SessionLock | Kind::BlockedApp => None,
    }
}

/// Cheap pre-check so events nobody is told about are not even cloned.
pub fn relevant(kind: Kind) -> bool {
    class(kind).is_some()
}

fn head(kind: Kind) -> &'static str {
    match kind {
        Kind::UnlockOk => "🔓 Acesso liberado",
        Kind::SessionUnlock => "🖥️ PC desbloqueado",
        Kind::AuthFail => "❌ Tentativa recusada",
        Kind::Lockout => "⛔ Bloqueio por tentativas",
        Kind::Tamper => "🚨 Violação do bloqueio",
        Kind::EnforcementError => "⚠️ Falha ao aplicar o bloqueio",
        Kind::PasswordChange => "🔑 Senha alterada",
        Kind::ConfigChange => "⚙️ Configuração alterada",
        Kind::AppStart => "▶️ Porteiro iniciado",
        Kind::Quit => "🛑 Proteção desativada",
        Kind::Relock | Kind::SessionLock | Kind::BlockedApp => "Porteiro",
    }
}

fn clip(text: &str) -> String {
    if text.chars().count() <= MAX_CHARS {
        return text.to_string();
    }
    text.chars().take(MAX_CHARS - 1).chain(std::iter::once('…')).collect()
}

fn ago(secs: i64) -> String {
    if secs >= 3600 { format!("{} h", secs / 3600) } else { format!("{} min", secs / 60) }
}

/// The alert for an event under the owner's switches, or `None` when nobody should hear about it.
pub fn alert(event: &Event, alerts: &Alerts, now_ms: i64) -> Option<Alert> {
    let (wanted, photo) = match class(event.kind)? {
        Class::Unlock => (true, alerts.photo_unlock),
        Class::Fail => (true, alerts.photo_fail),
        Class::Alarm => (alerts.tamper, alerts.photo_fail),
        Class::Change => (true, alerts.photo_fail),
    };
    if !wanted {
        return None;
    }
    let mut text = head(event.kind).to_string();
    if !event.detail.is_empty() {
        text.push('\n');
        text.push_str(&event.detail);
    }
    let late = (now_ms - event.at_ms) / 1000;
    if late > LATE_SECS {
        text.push_str(&format!("\n(ocorreu há {})", ago(late)));
    }
    Some(Alert { text: clip(&text), photo })
}

/// The webcam frame of an event, if one was taken. Screens are never sent.
pub fn cam_shot(event: &Event) -> Option<&str> {
    event.shots.iter().map(String::as_str).find(|s| s.ends_with("-cam.jpg"))
}

pub fn status_text(s: &Status) -> String {
    let head = match s.unlock_remaining_secs.filter(|_| !s.locked) {
        Some(secs) => format!("🔓 Liberado, faltam {} min", secs.div_ceil(60)),
        None => "🔒 Bloqueado".to_string(),
    };
    let failing: Vec<&str> = s.layers.iter().filter(|l| !l.ok).map(|l| l.name.as_str()).collect();
    let layers = match (s.layers.is_empty(), failing.is_empty()) {
        (true, _) => "Camadas: ainda não aplicadas".to_string(),
        (false, true) => format!("Camadas: {} ativas", s.layers.len()),
        (false, false) => format!("Camadas com falha: {}", failing.join(", ")),
    };
    let watchdog = match s.watchdog {
        Some(true) => "ativo",
        Some(false) => "FORA DO AR",
        None => "não verificado",
    };
    let mut text = format!("{head}\n{layers}\nVigia: {watchdog}\nEventos registrados: {}", s.event_count);
    if !s.elevated {
        text.push_str("\n⚠️ Sem administrador: o bloqueio não está sendo aplicado");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::LayerStatus;

    fn event(kind: Kind, detail: &str) -> Event {
        Event { id: "e".into(), at_ms: 1_000_000, kind, detail: detail.into(), shots: Vec::new() }
    }

    fn status() -> Status {
        Status {
            locked: true,
            unlock_remaining_secs: None,
            lockout_secs: None,
            elevated: true,
            needs_setup: false,
            hello_available: false,
            hello_enabled: false,
            layers: vec![LayerStatus { name: "hosts".into(), ok: true, error: None }],
            services: Vec::new(),
            custom_domains: Vec::new(),
            custom_apps: Vec::new(),
            unlock_minutes: 15,
            schedule: None,
            watchdog: Some(true),
            event_count: 7,
            data_dir: String::new(),
        }
    }

    #[test]
    fn unlocks_and_failures_always_go_out_and_the_switches_only_decide_the_photo() {
        let off = Alerts { photo_unlock: false, photo_fail: false, tamper: false };
        let unlock = alert(&event(Kind::UnlockOk, "acesso liberado por 15 min"), &off, 1_000_000).unwrap();
        assert!(!unlock.photo && unlock.text.contains("15 min"));
        let fail = alert(&event(Kind::AuthFail, "senha incorreta"), &Alerts::default(), 1_000_000).unwrap();
        assert!(fail.photo && fail.text.starts_with("❌"));
    }

    #[test]
    fn violations_can_be_silenced_but_changes_to_protection_never_are() {
        let quiet = Alerts { tamper: false, photo_fail: false, ..Alerts::default() };
        for kind in [Kind::Tamper, Kind::Lockout, Kind::EnforcementError, Kind::AppStart] {
            assert!(alert(&event(kind, "x"), &quiet, 1_000_000).is_none(), "{kind:?}");
            assert!(alert(&event(kind, "x"), &Alerts::default(), 1_000_000).is_some(), "{kind:?}");
        }
        for kind in [Kind::Quit, Kind::ConfigChange, Kind::PasswordChange] {
            let a = alert(&event(kind, "x"), &quiet, 1_000_000).expect("always announced");
            assert!(!a.photo, "{kind:?}: the photo still follows its switch");
        }
    }

    #[test]
    fn routine_events_are_never_sent() {
        for kind in [Kind::Relock, Kind::SessionLock, Kind::BlockedApp] {
            assert!(!relevant(kind) && alert(&event(kind, "x"), &Alerts::default(), 1_000_000).is_none());
        }
    }

    #[test]
    fn a_late_alert_says_when_it_happened_and_a_long_one_is_clipped() {
        let late = alert(&event(Kind::Tamper, "hosts"), &Alerts::default(), 1_000_000 + 7_500_000).unwrap();
        assert!(late.text.ends_with("(ocorreu há 2 h)"), "{}", late.text);
        let prompt = alert(&event(Kind::Tamper, "hosts"), &Alerts::default(), 1_000_000 + 30_000).unwrap();
        assert!(!prompt.text.contains("ocorreu"));
        let long = alert(&event(Kind::Tamper, &"é".repeat(5000)), &Alerts::default(), 1_000_000).unwrap();
        assert_eq!(long.text.chars().count(), MAX_CHARS);
    }

    #[test]
    fn only_the_webcam_frame_is_ever_picked_for_sending() {
        let mut e = event(Kind::UnlockOk, "");
        e.shots = vec!["a-screen0.jpg".into(), "a-cam.jpg".into()];
        assert_eq!(cam_shot(&e), Some("a-cam.jpg"));
        e.shots = vec!["a-screen0.jpg".into()];
        assert_eq!(cam_shot(&e), None);
    }

    #[test]
    fn the_status_reply_shows_lock_state_layers_and_the_watchdog() {
        let locked = status_text(&status());
        assert!(locked.starts_with("🔒 Bloqueado") && locked.contains("Camadas: 1 ativas") && locked.contains("Vigia: ativo"));
        let mut open = status();
        open.locked = false;
        open.unlock_remaining_secs = Some(601);
        open.layers.push(LayerStatus { name: "firewall".into(), ok: false, error: Some("x".into()) });
        open.watchdog = Some(false);
        open.elevated = false;
        let text = status_text(&open);
        assert!(text.contains("faltam 11 min") && text.contains("com falha: firewall") && text.contains("FORA DO AR") && text.contains("Sem administrador"));
    }
}
