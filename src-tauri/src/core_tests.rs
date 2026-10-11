use crate::{events::Kind, session::Transition, test_support::*};
use std::sync::atomic::Ordering;

fn blocked(h: &Harness) -> bool {
    h.hosts_text().contains("0.0.0.0 www.instagram.com")
}

#[test]
fn starts_locked_and_applies_the_block_on_boot() {
    let h = harness();
    assert!(h.core.status().locked && blocked(&h));
    assert!(h.hosts_text().contains("127.0.0.1 localhost"), "user lines survive");
    assert_eq!(h.kinds(), vec![Kind::AppStart]);
}

#[test]
fn unlock_lifts_the_block_with_evidence_and_it_returns_when_the_window_ends() {
    let mut h = harness();
    let status = h.core.unlock("1406", Some(15)).unwrap();
    assert!(!status.locked && status.unlock_remaining_secs == Some(900));
    assert!(!blocked(&h));
    let unlock = h.events().into_iter().find(|e| e.kind == Kind::UnlockOk).unwrap();
    assert_eq!(unlock.shots.len(), 1, "a capture is attached to every unlock");

    h.advance(899);
    h.core.tick();
    assert!(!blocked(&h), "still inside the window");
    h.advance(2);
    h.core.tick();
    assert!(blocked(&h) && h.core.status().locked);
    assert!(h.kinds().contains(&Kind::Relock));
}

#[test]
fn a_wrong_password_changes_nothing_and_leaves_a_photographed_trail() {
    let mut h = harness();
    let err = h.core.unlock("0000", None).unwrap_err();
    assert_eq!((err.code, err.attempts_left), ("wrong_password", Some(4)));
    assert!(blocked(&h) && h.core.status().locked);
    let fail = h.events().into_iter().find(|e| e.kind == Kind::AuthFail).unwrap();
    assert_eq!(fail.shots.len(), 1);
}

#[test]
fn five_misses_lock_out_even_the_right_password_until_the_wait_passes() {
    let mut h = harness();
    for _ in 0..5 {
        let _ = h.core.unlock("bad", None);
    }
    let err = h.core.unlock("1406", None).unwrap_err();
    assert_eq!(err.code, "locked_out");
    assert!(err.retry_after.unwrap() > 0 && h.core.status().lockout_secs.is_some());
    assert!(blocked(&h));
    h.advance(31);
    assert!(h.core.unlock("1406", None).is_ok());
    assert!(h.kinds().contains(&Kind::Lockout));
}

#[test]
fn restarting_the_app_does_not_reset_the_lockout() {
    let mut h = harness();
    for _ in 0..5 {
        let _ = h.core.unlock("bad", None);
    }
    let mut h = h.restart();
    assert_eq!(h.core.unlock("1406", None).unwrap_err().code, "locked_out");
}

#[test]
fn removing_the_block_behind_the_apps_back_is_detected_restored_and_recorded() {
    let mut h = harness();
    std::fs::write(h.dir.path().join("hosts"), "127.0.0.1 localhost\r\n").unwrap();
    assert!(!blocked(&h));
    h.core.tick();
    assert!(blocked(&h), "restored on the next tick");
    let tamper = h.events().into_iter().find(|e| e.kind == Kind::Tamper).unwrap();
    assert!(tamper.detail.contains("hosts") && tamper.shots.len() == 1);
}

#[test]
fn killing_the_app_mid_window_keeps_the_window_but_never_extends_it() {
    let mut h = harness();
    h.core.unlock("1406", Some(10)).unwrap();
    h.advance(60);
    let h = h.restart();
    assert!(!h.core.status().locked && !blocked(&h), "still inside the persisted window");
    assert_eq!(h.core.status().unlock_remaining_secs, Some(540));
    h.advance(600);
    let h = h.restart();
    assert!(h.core.status().locked && blocked(&h), "an expired window never revives");
}

#[test]
fn a_clock_moved_back_cannot_stretch_a_persisted_window() {
    let mut h = harness();
    h.core.unlock("1406", Some(10)).unwrap();
    h.wall.fetch_sub(10 * 3600, Ordering::SeqCst);
    let h = h.restart();
    assert!(h.core.status().locked && blocked(&h));
    assert!(h.events().iter().any(|e| e.kind == Kind::Tamper && e.detail.contains("relógio")));
}

#[test]
fn events_keep_real_time_order_even_though_evidence_is_written_later() {
    let mut h = harness();
    h.core.unlock("1406", None).unwrap();
    h.core.relock();
    let kinds = h.kinds();
    assert_eq!(kinds[..3], [Kind::Relock, Kind::UnlockOk, Kind::AppStart], "relock happened after the unlock, whichever file write finished first");
}

#[test]
fn locking_again_needs_no_password() {
    let mut h = harness();
    h.core.unlock("1406", None).unwrap();
    assert!(h.core.relock().locked && blocked(&h));
    assert!(h.kinds().contains(&Kind::Relock));
}

#[test]
fn blocked_desktop_apps_are_ended_only_while_locked_and_logged_once_a_minute() {
    let mut h = harness();
    h.running.lock().unwrap().push("WhatsApp.exe".into());
    h.core.tick();
    assert!(h.running.lock().unwrap().is_empty());
    h.running.lock().unwrap().push("whatsapp.exe".into());
    h.advance(5);
    h.core.tick();
    assert_eq!(h.kinds().iter().filter(|k| **k == Kind::BlockedApp).count(), 1, "second kill inside a minute is not logged again");
    h.core.unlock("1406", None).unwrap();
    h.running.lock().unwrap().push("WhatsApp.exe".into());
    h.core.tick();
    assert_eq!(h.running.lock().unwrap().len(), 1, "left alone while unlocked");
}

#[test]
fn windows_unlock_is_recorded_with_evidence_and_lock_without() {
    let mut h = harness();
    h.core.session_event(Transition::Locked);
    h.core.session_event(Transition::Unlocked);
    let events = h.events();
    assert_eq!(events.iter().find(|e| e.kind == Kind::SessionUnlock).unwrap().shots.len(), 1);
    assert!(events.iter().find(|e| e.kind == Kind::SessionLock).unwrap().shots.is_empty());
}

#[test]
fn a_failing_layer_is_reported_logged_once_and_retried_after_the_backoff() {
    let mut h = harness();
    h.flaky.store(true, Ordering::SeqCst);
    h.core.add_domain("1406", "reddit.com").unwrap();
    let status = h.core.status();
    assert!(status.layers.iter().any(|l| l.name == "flaky" && !l.ok && l.error.as_deref() == Some("boom")));
    assert!(blocked(&h), "the healthy layers are still applied");
    h.advance(31);
    h.core.tick();
    assert_eq!(h.kinds().iter().filter(|k| **k == Kind::EnforcementError).count(), 1, "same error is not spammed");
    h.flaky.store(false, Ordering::SeqCst);
    h.advance(31);
    h.core.tick();
    assert!(h.core.status().layers.iter().all(|l| l.ok));
}
