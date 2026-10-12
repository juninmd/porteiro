use crate::{events::Kind, telegram::Alerts, test_support::*};
use std::sync::atomic::Ordering;

#[test]
fn every_telegram_change_needs_the_password_and_a_miss_is_photographed() {
    let mut h = harness();
    h.pair_chat(7);
    assert_eq!(h.core.telegram_pair("nope").unwrap_err().code, "wrong_password");
    assert_eq!(h.core.telegram_disconnect("nope").unwrap_err().code, "wrong_password");
    assert_eq!(h.core.telegram_remove_chat("nope", 7).unwrap_err().code, "wrong_password");
    assert_eq!(h.core.telegram_set_alerts("nope", Alerts { tamper: false, ..Alerts::default() }).unwrap_err().code, "wrong_password");
    assert!(h.core.telegram_view().connected && h.core.telegram_view().alerts.tamper, "nothing changed");
    let fails = h.events().into_iter().filter(|e| e.kind == Kind::AuthFail && !e.shots.is_empty()).count();
    assert_eq!(fails, 4);
}

#[test]
fn the_password_is_checked_before_a_token_is_accepted() {
    let mut h = harness();
    assert_eq!(h.core.verify("nope", "conectar o Telegram").err().map(|e| e.code), Some("wrong_password"));
    let ok = h.core.verify(TEST_PASSWORD, "conectar o Telegram").unwrap();
    let err = h.core.telegram_connect(ok, "not a token", "bot").unwrap_err();
    assert_eq!(err.code, "telegram_token_invalid");
    let ok = h.core.verify(TEST_PASSWORD, "conectar o Telegram").unwrap();
    let view = h.core.telegram_connect(ok, "123456789:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw", "porteiro_bot").unwrap();
    assert!(view.connected && view.bot_username.as_deref() == Some("porteiro_bot"));
}

#[test]
fn pairing_opens_a_code_only_for_the_owner_and_is_announced() {
    let mut h = harness();
    h.hub.connect("1:token", "porteiro_bot").unwrap();
    let view = h.core.telegram_pair(TEST_PASSWORD).unwrap();
    assert_eq!(view.pairing.as_ref().map(|p| p.code.len()), Some(6));
    assert!(h.events().iter().any(|e| e.kind == Kind::ConfigChange && e.detail.contains("pareamento")));
}

#[test]
fn an_unlock_reaches_the_outbox_with_its_webcam_frame_attached() {
    let mut h = harness();
    h.pair_chat(7);
    h.core.unlock(TEST_PASSWORD, Some(5)).unwrap();
    let alerts = h.alerts();
    let unlock = alerts.iter().find(|j| j.event.kind == Kind::UnlockOk).expect("unlock is announced");
    assert_eq!(unlock.target.chats, vec![7]);
    assert!(unlock.event.shots.iter().any(|s| s.ends_with("-cam.jpg")), "the alert is built after the capture finished");
}

#[test]
fn a_wrong_password_is_announced_and_routine_events_are_not() {
    let mut h = harness();
    h.pair_chat(7);
    let _ = h.core.unlock("nope", None);
    h.core.unlock(TEST_PASSWORD, Some(5)).unwrap();
    h.core.relock();
    h.core.session_event(crate::session::Transition::Locked);
    let kinds: Vec<Kind> = h.alerts().into_iter().map(|j| j.event.kind).collect();
    assert!(kinds.contains(&Kind::AuthFail) && kinds.contains(&Kind::UnlockOk));
    assert!(!kinds.contains(&Kind::Relock) && !kinds.contains(&Kind::SessionLock));
}

#[test]
fn nobody_is_told_while_no_chat_is_paired() {
    let mut h = harness();
    h.hub.connect("1:token", "porteiro_bot").unwrap();
    h.core.unlock(TEST_PASSWORD, Some(5)).unwrap();
    assert!(h.alerts().is_empty());
}

#[test]
fn disconnecting_still_tells_the_chats_that_were_paired() {
    let mut h = harness();
    h.pair_chat(7);
    let _ = h.alerts();
    h.core.telegram_disconnect(TEST_PASSWORD).unwrap();
    assert!(!h.core.telegram_view().connected);
    let farewell = h.alerts().into_iter().find(|j| j.event.detail.contains("desconectado")).expect("the farewell alert exists");
    assert_eq!((farewell.target.chats, farewell.event.kind), (vec![7], Kind::ConfigChange));
}

#[test]
fn removing_a_chat_tells_that_chat_and_an_unknown_chat_is_refused() {
    let mut h = harness();
    h.pair_chat(7);
    let _ = h.alerts();
    assert_eq!(h.core.telegram_remove_chat(TEST_PASSWORD, 99).unwrap_err().code, "telegram_chat_unknown");
    h.core.telegram_remove_chat(TEST_PASSWORD, 7).unwrap();
    assert!(h.core.telegram_view().chats.is_empty());
    let told = h.alerts().into_iter().find(|j| j.event.detail.contains("chat removido")).expect("announced");
    assert_eq!(told.target.chats, vec![7]);
}

#[test]
fn turning_the_tamper_alerts_off_is_itself_announced_under_the_old_switches() {
    let mut h = harness();
    h.pair_chat(7);
    let _ = h.alerts();
    h.core.telegram_set_alerts(TEST_PASSWORD, Alerts { tamper: false, ..Alerts::default() }).unwrap();
    let told = h.alerts().into_iter().find(|j| j.event.detail.contains("alertas do Telegram")).expect("announced");
    assert!(told.target.alerts.tamper, "decided before the change, so the partner hears it");
}

#[test]
fn settings_survive_a_restart_but_an_open_pairing_code_does_not() {
    let h = harness();
    h.pair_chat(7);
    h.hub.start_pairing(h.mono.load(Ordering::SeqCst)).unwrap();
    let h = h.restart();
    let view = h.core.telegram_view();
    assert!(view.connected && view.chats.len() == 1 && view.pairing.is_none());
}

#[test]
fn a_remote_relock_says_where_it_came_from() {
    let mut h = harness();
    h.core.unlock(TEST_PASSWORD, Some(5)).unwrap();
    assert!(h.core.relock_via("pelo Telegram"));
    assert!(!h.core.relock_via("pelo Telegram"), "already locked");
    assert!(h.events().iter().any(|e| e.kind == Kind::Relock && e.detail.contains("pelo Telegram")));
}

#[test]
fn quitting_hands_the_shutdown_alert_over_before_it_returns() {
    let mut h = harness();
    h.pair_chat(7);
    let _ = h.alerts();
    h.core.disable_and_quit(TEST_PASSWORD).unwrap();
    // No extra flush here: disable_and_quit itself must have waited, because the process exits right after.
    let kinds: Vec<Kind> = h.jobs.try_iter().map(|j| j.event.kind).collect();
    assert!(kinds.contains(&Kind::Quit), "{kinds:?}");
}

#[test]
fn a_damaged_telegram_file_is_a_tamper_event_and_the_guard_still_starts() {
    let h = harness_with(|data| std::fs::write(data.join("telegram.json"), b"{not json").unwrap());
    assert!(!h.core.telegram_view().connected);
    assert!(h.events().iter().any(|e| e.kind == Kind::Tamper && e.detail.contains("telegram.json")));
}
