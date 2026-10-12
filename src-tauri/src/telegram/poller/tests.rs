use super::*;
use crate::telegram::{
    bot::{Bot, ChatRef, Me, Update},
    fake::{FakeBot, Sent},
};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Mutex,
};

#[derive(Default)]
struct FakeRemote {
    now: AtomicU64,
    unlocked: AtomicBool,
    recorded: Mutex<Vec<(Kind, String)>>,
}

impl Remote for FakeRemote {
    fn now(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }
    fn status_text(&self) -> String {
        "STATUS".into()
    }
    fn relock(&self) -> bool {
        self.unlocked.swap(false, Ordering::SeqCst)
    }
    fn record(&self, kind: Kind, detail: String) {
        self.recorded.lock().unwrap().push((kind, detail));
    }
}

struct Rig {
    poller: Poller,
    hub: Hub,
    remote: Arc<FakeRemote>,
    _dir: tempfile::TempDir,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let hub = Hub::load(dir.path().join("telegram.json")).unwrap();
    hub.connect("1:t", "bot").unwrap();
    let remote = Arc::new(FakeRemote::default());
    Rig { poller: Poller::new(hub.clone(), Arc::new(FakeBot::default()), remote.clone()), hub, remote, _dir: dir }
}

fn msg(chat: i64, kind: &str, text: &str) -> Message {
    Message { chat: ChatRef { id: chat, kind: kind.into(), first_name: Some("Ana".into()), username: None }, text: Some(text.into()) }
}

fn open_pairing(r: &Rig) -> String {
    r.hub.start_pairing(r.remote.now()).unwrap();
    r.hub.view(r.remote.now()).pairing.unwrap().code
}

#[test]
fn the_code_pairs_a_private_chat_and_the_owner_is_told_through_an_event() {
    let r = rig();
    let code = open_pairing(&r);
    let spaced = format!("{} {}", &code[..3], &code[3..]);
    let reply = r.poller.handle(&msg(7, "private", &spaced)).unwrap();
    assert!(reply.starts_with("✅") && r.hub.is_paired(7));
    assert!(r.remote.recorded.lock().unwrap().iter().any(|(k, d)| *k == Kind::ConfigChange && d.contains("pareado")));
}

#[test]
fn a_group_can_never_be_paired_or_commanded() {
    let r = rig();
    let code = open_pairing(&r);
    assert!(r.poller.handle(&msg(7, "group", &code)).is_none() && !r.hub.is_paired(7));
    assert!(r.poller.handle(&msg(7, "supergroup", "/status")).is_none());
}

#[test]
fn pressing_start_does_not_burn_a_pairing_attempt() {
    let r = rig();
    let code = open_pairing(&r);
    for _ in 0..10 {
        assert!(r.poller.handle(&msg(7, "private", "/start")).unwrap().contains("código de 6 dígitos"));
    }
    assert!(r.poller.handle(&msg(7, "private", &code)).unwrap().starts_with("✅"));
}

#[test]
fn wrong_codes_run_out_and_the_closing_is_recorded() {
    let r = rig();
    let code = open_pairing(&r);
    let wrong = if code == "111111" { "222222" } else { "111111" };
    let mut last = String::new();
    for _ in 0..5 {
        last = r.poller.handle(&msg(7, "private", wrong)).unwrap();
    }
    assert!(last.contains("encerrado") && !r.hub.is_paired(7));
    assert!(r.remote.recorded.lock().unwrap().iter().any(|(k, _)| *k == Kind::Tamper));
    assert!(r.poller.handle(&msg(7, "private", &code)).is_none(), "a closed window stays closed");
}

#[test]
fn strangers_get_silence_and_at_most_one_event_per_window() {
    let r = rig();
    for _ in 0..3 {
        assert!(r.poller.handle(&msg(7, "private", "/bloquear")).is_none());
    }
    assert_eq!(r.remote.recorded.lock().unwrap().len(), 1);
    r.remote.unlocked.store(true, Ordering::SeqCst);
    r.poller.handle(&msg(7, "private", "/bloquear"));
    assert!(r.remote.unlocked.load(Ordering::SeqCst), "a stranger cannot lock or unlock anything");
}

#[test]
fn paired_chats_get_status_and_can_relock() {
    let r = rig();
    let code = open_pairing(&r);
    r.poller.handle(&msg(7, "private", &code));
    assert_eq!(r.poller.handle(&msg(7, "private", "/status@porteiro_bot")).unwrap(), "STATUS");
    assert!(r.poller.handle(&msg(7, "private", "/bloquear")).unwrap().contains("Já estava"));
    r.remote.unlocked.store(true, Ordering::SeqCst);
    assert!(r.poller.handle(&msg(7, "private", "/BLOQUEAR")).unwrap().contains("de novo"));
    assert!(r.poller.handle(&msg(7, "private", "/desbloquear")).unwrap().starts_with("Não entendi"));
}

#[test]
fn a_poll_answers_each_message_and_moves_the_offset_past_the_batch() {
    let dir = tempfile::tempdir().unwrap();
    let hub = Hub::load(dir.path().join("telegram.json")).unwrap();
    hub.connect("1:t", "bot").unwrap();
    hub.start_pairing(0).unwrap();
    let code = hub.view(0).pairing.unwrap().code;
    struct Scripted(FakeBot, Mutex<Option<Vec<Update>>>);
    impl Bot for Scripted {
        fn get_me(&self, t: &str) -> Result<crate::telegram::bot::Me, BotError> {
            self.0.get_me(t)
        }
        fn get_updates(&self, _: &str, _: i64, _: u64) -> Result<Vec<Update>, BotError> {
            Ok(self.1.lock().unwrap().take().unwrap_or_default())
        }
        fn send_message(&self, t: &str, c: i64, x: &str) -> Result<(), BotError> {
            self.0.send_message(t, c, x)
        }
        fn send_photo(&self, t: &str, c: i64, x: &str, j: Vec<u8>) -> Result<(), BotError> {
            self.0.send_photo(t, c, x, j)
        }
    }
    let batch = vec![Update { update_id: 40, message: Some(msg(7, "private", &code)) }, Update { update_id: 41, message: None }];
    let bot = Arc::new(Scripted(FakeBot::default(), Mutex::new(Some(batch))));
    let poller = Poller::new(hub.clone(), bot.clone(), Arc::new(FakeRemote::default()));
    poller.poll_once().unwrap();
    assert_eq!(hub.polling().unwrap().1, 42);
    assert!(matches!(&bot.0.sent()[..], [Sent::Text { chat: 7, text }] if text.starts_with("✅")));
}

#[test]
fn being_offline_is_reported_only_when_it_lasts() {
    let r = rig();
    let err = BotError::Transient("dns".into());
    let mut failures = 0;
    for _ in 0..REPORT_AFTER - 1 {
        r.poller.note_transient(&mut failures, &err);
    }
    assert!(r.hub.view(0).last_error.is_none(), "a blip is routine");
    r.poller.note_transient(&mut failures, &err);
    assert!(r.hub.view(0).last_error.unwrap().contains("dns"));
}

struct OneUpdate;

impl Bot for OneUpdate {
    fn get_me(&self, _: &str) -> Result<Me, BotError> {
        Ok(Me { username: "bot".into() })
    }
    fn get_updates(&self, _: &str, _: i64, _: u64) -> Result<Vec<Update>, BotError> {
        Ok(vec![Update { update_id: 5, message: Some(msg(777, "private", "oi")) }])
    }
    fn send_message(&self, _: &str, _: i64, _: &str) -> Result<(), BotError> {
        Ok(())
    }
    fn send_photo(&self, _: &str, _: i64, _: &str, _: Vec<u8>) -> Result<(), BotError> {
        Ok(())
    }
}

#[test]
fn a_failing_offset_save_backs_off_instead_of_replaying_the_batch_in_a_tight_loop() {
    let r = rig();
    let poller = Poller::new(r.hub.clone(), Arc::new(OneUpdate), r.remote.clone());
    std::fs::remove_dir_all(r._dir.path()).unwrap();
    assert!(matches!(poller.poll_once(), Err(BotError::Transient(_))), "the settings file can no longer be saved");
}
