use super::*;

fn hub() -> (Hub, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    (Hub::load(dir.path().join("telegram.json")).unwrap(), dir)
}

fn code_of(h: &Hub, now: u64) -> String {
    h.view(now).pairing.expect("a pairing is open").code
}

#[test]
fn pairing_needs_a_bot_and_hands_out_a_six_digit_code() {
    let (h, _d) = hub();
    assert!(h.start_pairing(0).is_err());
    h.connect("1:token", "bot").unwrap();
    h.start_pairing(0).unwrap();
    let code = code_of(&h, 0);
    assert!(code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()));
    assert_eq!(h.view(100).pairing.unwrap().secs_left, PAIRING_TTL_SECS - 100);
}

#[test]
fn the_right_code_pairs_once_and_is_then_spent() {
    let (h, _d) = hub();
    h.connect("1:token", "bot").unwrap();
    h.start_pairing(0).unwrap();
    let code = code_of(&h, 0);
    assert_eq!(h.try_pair(9, "Ana", &code, 10).unwrap(), PairResult::Paired);
    assert!(h.is_paired(9) && h.view(10).pairing.is_none());
    assert_eq!(h.try_pair(10, "Bia", &code, 11).unwrap(), PairResult::Idle, "single use");
    assert!(!h.is_paired(10));
}

#[test]
fn an_expired_code_pairs_nobody() {
    let (h, _d) = hub();
    h.connect("1:token", "bot").unwrap();
    h.start_pairing(0).unwrap();
    let code = code_of(&h, 0);
    assert_eq!(h.try_pair(9, "Ana", &code, PAIRING_TTL_SECS).unwrap(), PairResult::Idle);
    assert!(h.view(PAIRING_TTL_SECS).pairing.is_none() && !h.is_paired(9));
}

#[test]
fn guessing_is_capped_and_the_window_closes_even_if_the_next_guess_is_right() {
    let (h, _d) = hub();
    h.connect("1:token", "bot").unwrap();
    h.start_pairing(0).unwrap();
    let code = code_of(&h, 0);
    let wrong = if code == "000000" { "000001" } else { "000000" };
    for left in (1..MAX_WRONG_CODES).rev() {
        assert_eq!(h.try_pair(9, "x", wrong, 1).unwrap(), PairResult::Wrong(left));
    }
    assert_eq!(h.try_pair(9, "x", wrong, 1).unwrap(), PairResult::Closed);
    assert_eq!(h.try_pair(9, "x", &code, 2).unwrap(), PairResult::Idle);
    assert!(!h.is_paired(9));
}

#[test]
fn the_chat_limit_holds_by_refusing_a_new_pairing_when_the_list_is_full() {
    let (h, _d) = hub();
    h.connect("1:token", "bot").unwrap();
    for id in 0..MAX_CHATS as i64 {
        h.start_pairing(0).unwrap();
        let code = code_of(&h, 0);
        assert_eq!(h.try_pair(id, "n", &code, 1).unwrap(), PairResult::Paired);
    }
    assert!(h.start_pairing(0).is_err());
    assert_eq!(h.view(0).chats.len(), MAX_CHATS);
}

#[test]
fn pairing_the_same_chat_twice_keeps_a_single_entry() {
    let (h, _d) = hub();
    h.connect("1:token", "bot").unwrap();
    for _ in 0..2 {
        h.start_pairing(0).unwrap();
        let code = code_of(&h, 0);
        assert_eq!(h.try_pair(9, "Ana", &code, 1).unwrap(), PairResult::Paired);
    }
    assert_eq!(h.view(0).chats, vec![Chat { id: 9, name: "Ana".into() }]);
}

#[test]
fn connecting_another_bot_drops_old_chats_but_keeps_the_alert_switches() {
    let (h, _d) = hub();
    h.connect("1:token", "bot").unwrap();
    h.start_pairing(0).unwrap();
    let code = code_of(&h, 0);
    h.try_pair(9, "Ana", &code, 1).unwrap();
    h.set_alerts(Alerts { photo_unlock: false, ..Alerts::default() }).unwrap();
    h.connect("2:token", "other").unwrap();
    let v = h.view(2);
    assert!(v.chats.is_empty() && v.pairing.is_none() && !v.alerts.photo_unlock);
    assert!(h.targets().is_none(), "nobody paired yet, so nobody to tell");
}

#[test]
fn disconnect_forgets_the_token_and_the_chats_and_it_survives_a_restart() {
    let (h, d) = hub();
    h.connect("1:token", "bot").unwrap();
    h.start_pairing(0).unwrap();
    let code = code_of(&h, 0);
    h.try_pair(9, "Ana", &code, 1).unwrap();
    assert!(Hub::load(d.path().join("telegram.json")).unwrap().is_paired(9), "persisted");
    h.disconnect().unwrap();
    let again = Hub::load(d.path().join("telegram.json")).unwrap();
    assert!(!again.view(0).connected && again.view(0).chats.is_empty() && again.polling().is_none());
}

#[test]
fn the_offset_moves_forward_only_for_the_token_that_was_polled() {
    let (h, _d) = hub();
    h.connect("1:token", "bot").unwrap();
    h.advance_offset("1:token", 10).unwrap();
    h.advance_offset("1:token", 4).unwrap();
    h.advance_offset("9:stale", 99).unwrap();
    assert_eq!(h.polling(), Some(("1:token".to_string(), 10)));
}

#[test]
fn strangers_are_reported_once_per_chat_and_only_a_few_per_window() {
    let (h, _d) = hub();
    assert!(h.report_unknown(1, 0) && !h.report_unknown(1, 5));
    for id in 2..=5 {
        assert!(h.report_unknown(id, 6));
    }
    assert!(!h.report_unknown(99, 7), "burst cap");
    assert!(h.report_unknown(99, UNKNOWN_WINDOW_SECS + 1), "window rolled over");
}

#[test]
fn a_poll_problem_and_a_send_problem_do_not_hide_each_other() {
    let (h, _d) = hub();
    h.set_fault(Fault::Poll, Some("poll".into()));
    h.set_fault(Fault::Send, Some("send".into()));
    assert_eq!(h.view(0).last_error.as_deref(), Some("send"));
    h.set_fault(Fault::Send, None);
    assert_eq!(h.view(0).last_error.as_deref(), Some("poll"), "the poll problem is still there");
    h.set_fault(Fault::Poll, None);
    assert!(h.view(0).last_error.is_none());
}

#[test]
fn a_damaged_file_starts_empty_with_a_note_and_the_guard_can_still_boot_without_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("telegram.json");
    std::fs::write(&path, b"{not json").unwrap();
    let h = Hub::load(path.clone()).unwrap();
    assert!(!h.view(0).connected && h.polling().is_none());
    assert!(h.take_startup_note().unwrap().contains("ilegível") && h.take_startup_note().is_none(), "reported once");

    let detached = Hub::detached(dir.path().join("missing").join("telegram.json"), "acesso negado");
    assert!(detached.take_startup_note().unwrap().contains("acesso negado"));
    assert!(detached.view(0).last_error.unwrap().contains("acesso negado"));
    assert!(detached.connect("1:token", "bot").is_err(), "saving fails loudly instead of pretending");
    assert!(!detached.view(0).connected, "memory never gets ahead of the file");
}
