use super::*;
use crate::{
    events::Kind,
    telegram::{
        bot::{Me, Update},
        fake::{FakeBot, Sent},
        settings::Alerts,
    },
};

struct Rig {
    outbox: Outbox,
    bot: Arc<FakeBot>,
    hub: Hub,
    dir: tempfile::TempDir,
}

const TOKEN: &str = "1:token";

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let bot = Arc::new(FakeBot::default());
    let hub = Hub::load(dir.path().join("telegram.json")).unwrap();
    hub.connect(TOKEN, "bot").unwrap();
    let mut outbox = Outbox::new(bot.clone(), hub.clone(), dir.path().to_path_buf(), dir.path().join("outbox.json"));
    outbox.backoff = |_| Duration::ZERO;
    outbox.now_ms = || 1_000_000;
    Rig { outbox, bot, hub, dir }
}

fn pair(hub: &Hub, chat: i64, name: &str) {
    hub.start_pairing(0).unwrap();
    let code = hub.view(0).pairing.unwrap().code;
    hub.try_pair(chat, name, &code, 1).unwrap();
}

fn job(kind: Kind, detail: &str, chats: &[i64], shots: &[String]) -> Job {
    let event = Event { id: "e".into(), at_ms: 1_000_000, kind, detail: detail.into(), shots: shots.to_vec() };
    Job { target: Target { token: TOKEN.into(), chats: chats.to_vec(), alerts: Alerts::default() }, event }
}

fn texts(bot: &FakeBot) -> Vec<(i64, String)> {
    bot.sent()
        .into_iter()
        .map(|s| match s {
            Sent::Text { chat, text } | Sent::Photo { chat, caption: text, .. } => (chat, text),
        })
        .collect()
}

#[test]
fn every_paired_chat_gets_the_alert() {
    let mut r = rig();
    r.outbox.push(job(Kind::Tamper, "hosts", &[1, 2], &[]));
    r.outbox.drain(Instant::now());
    assert_eq!(texts(&r.bot).iter().map(|(c, _)| *c).collect::<Vec<_>>(), vec![1, 2]);
}

#[test]
fn the_webcam_frame_goes_as_a_photo_with_the_text_as_caption() {
    let mut r = rig();
    let name = format!("{}-cam.jpg", uuid::Uuid::new_v4());
    std::fs::write(r.dir.path().join(&name), [0xff, 0xd8, 0xff]).unwrap();
    r.outbox.push(job(Kind::UnlockOk, "acesso liberado por 15 min", &[1], &[name]));
    r.outbox.drain(Instant::now());
    assert!(matches!(&r.bot.sent()[..], [Sent::Photo { chat: 1, caption, bytes }] if caption.contains("15 min") && bytes == &[0xff, 0xd8, 0xff]));
}

#[test]
fn a_refused_photo_falls_back_to_text_and_a_missing_file_does_too() {
    let mut r = rig();
    let name = format!("{}-cam.jpg", uuid::Uuid::new_v4());
    std::fs::write(r.dir.path().join(&name), [1]).unwrap();
    r.bot.fail_next(BotError::Rejected("PHOTO_INVALID_DIMENSIONS".into()));
    r.outbox.push(job(Kind::AuthFail, "senha incorreta", &[1], &[name]));
    r.outbox.push(job(Kind::AuthFail, "senha incorreta", &[1], &[format!("{}-cam.jpg", uuid::Uuid::new_v4())]));
    r.outbox.drain(Instant::now());
    assert!(r.bot.sent().iter().all(|s| matches!(s, Sent::Text { .. })) && r.bot.sent().len() == 2);
}

#[test]
fn going_offline_delays_alerts_without_losing_or_reordering_them() {
    let mut r = rig();
    r.bot.fail_next(BotError::Transient("offline".into()));
    r.outbox.push(job(Kind::Tamper, "primeiro", &[1], &[]));
    r.outbox.push(job(Kind::Tamper, "segundo", &[1], &[]));
    r.outbox.drain(Instant::now());
    assert!(r.bot.sent().is_empty(), "the second alert must not overtake the first");
    r.outbox.drain(Instant::now());
    let order: Vec<String> = texts(&r.bot).into_iter().map(|(_, t)| t).collect();
    assert!(order[0].contains("primeiro") && order[1].contains("segundo"));
}

#[test]
fn a_retry_only_repeats_the_alert_to_the_chat_that_missed_it() {
    let mut r = rig();
    r.bot.fail_after(1, BotError::Transient("blip".into()));
    r.outbox.push(job(Kind::Tamper, "x", &[1, 2], &[]));
    r.outbox.drain(Instant::now());
    r.outbox.drain(Instant::now());
    assert_eq!(texts(&r.bot).iter().map(|(c, _)| *c).collect::<Vec<_>>(), vec![1, 2]);
}

#[test]
fn a_rate_limit_holds_the_queue_for_the_time_telegram_asked() {
    let mut r = rig();
    r.bot.fail_next(BotError::RetryAfter(30));
    r.outbox.push(job(Kind::Tamper, "x", &[1], &[]));
    let start = Instant::now();
    r.outbox.drain(start);
    r.outbox.drain(start + Duration::from_secs(10));
    assert!(r.bot.sent().is_empty());
    r.outbox.drain(start + Duration::from_secs(31));
    assert_eq!(r.bot.sent().len(), 1);
}

#[test]
fn a_revoked_token_drops_the_alert_and_tells_the_user() {
    let mut r = rig();
    r.bot.fail_next(BotError::Unauthorized);
    r.outbox.push(job(Kind::Tamper, "x", &[1], &[]));
    r.outbox.drain(Instant::now());
    assert!(r.outbox.queue.is_empty() && r.hub.view(0).last_error.unwrap().contains("token"));
}

#[test]
fn a_blocked_chat_is_named_to_the_owner_and_does_not_jam_the_queue() {
    let mut r = rig();
    pair(&r.hub, 1, "Ana");
    r.bot.fail_next(BotError::Rejected("Forbidden: bot was blocked by the user".into()));
    r.outbox.push(job(Kind::Tamper, "primeiro", &[1], &[]));
    r.outbox.push(job(Kind::Tamper, "segundo", &[1], &[]));
    r.outbox.drain(Instant::now());
    assert!(r.outbox.queue.is_empty());
    let texts = texts(&r.bot);
    assert!(matches!(&texts[..], [(1, t)] if t.contains("segundo")), "the next alert still goes out: {texts:?}");
    // The later success clears the problem, but only after it was visible in between.
    assert!(r.hub.view(0).last_error.is_none());
    r.bot.fail_next(BotError::Rejected("Forbidden".into()));
    r.outbox.push(job(Kind::Tamper, "terceiro", &[1], &[]));
    r.outbox.drain(Instant::now());
    assert!(r.hub.view(0).last_error.unwrap().contains("Ana"));
}

#[test]
fn stale_alerts_are_dropped() {
    let mut r = rig();
    r.outbox.push(job(Kind::Tamper, "velho", &[1], &[]));
    r.outbox.now_ms = || 1_000_000 + GIVE_UP_AFTER_MS + 1;
    r.outbox.drain(Instant::now());
    assert!(r.outbox.queue.is_empty() && r.bot.sent().is_empty());
    assert!(r.hub.view(0).last_error.unwrap().contains("descartado"), "dropping news is never silent");
}

#[test]
fn a_flush_delivers_what_is_waiting_in_backoff_and_gives_up_when_offline() {
    let mut r = rig();
    r.outbox.backoff = |_| Duration::from_secs(3600);
    r.bot.fail_next(BotError::Transient("blip".into()));
    r.outbox.push(job(Kind::Quit, "protecao desativada", &[1], &[]));
    r.outbox.drain(Instant::now());
    assert!(r.bot.sent().is_empty() && r.outbox.queue.len() == 1, "waiting out the backoff");
    r.outbox.flush();
    assert_eq!(r.bot.sent().len(), 1, "quitting does not wait for the backoff");

    r.outbox.flush_budget = Duration::from_millis(400);
    for _ in 0..50 {
        r.bot.fail_next(BotError::Transient("offline".into()));
    }
    r.outbox.push(job(Kind::Quit, "de novo", &[1], &[]));
    let started = Instant::now();
    r.outbox.flush();
    assert!(started.elapsed() < Duration::from_secs(5) && r.outbox.queue.len() == 1, "gave up, alert kept");
}

#[test]
fn the_queue_is_bounded() {
    let mut r = rig();
    r.bot.fail_next(BotError::Transient("offline".into()));
    for n in 0..QUEUE_CAP + 10 {
        r.outbox.push(job(Kind::Tamper, &n.to_string(), &[1], &[]));
    }
    assert_eq!(r.outbox.queue.len(), QUEUE_CAP);
}

#[test]
fn waiting_alerts_survive_a_restart_and_go_out_after_it() {
    let mut r = rig();
    r.bot.fail_next(BotError::Transient("offline".into()));
    r.bot.fail_next(BotError::Transient("offline".into()));
    r.outbox.push(job(Kind::Quit, "desligado offline", &[1, 2], &[]));
    r.outbox.drain(Instant::now());
    assert!(r.bot.sent().is_empty());
    let on_disk = std::fs::read_to_string(r.dir.path().join("outbox.json")).unwrap();
    assert!(on_disk.contains("desligado offline") && !on_disk.contains(TOKEN), "the token is not copied into the queue file");

    let after = Arc::new(FakeBot::default());
    let mut reborn = Outbox::new(after.clone(), r.hub.clone(), r.dir.path().to_path_buf(), r.dir.path().join("outbox.json"));
    reborn.now_ms = || 1_000_000;
    assert_eq!(reborn.queue.len(), 1);
    reborn.drain(Instant::now());
    assert_eq!(texts(&after).iter().map(|(c, _)| *c).collect::<Vec<_>>(), vec![1, 2]);
    let left: Vec<Stored> = serde_json::from_slice(&std::fs::read(r.dir.path().join("outbox.json")).unwrap()).unwrap();
    assert!(left.is_empty(), "delivered alerts leave the file");
}

#[test]
fn a_partly_delivered_alert_is_restored_only_for_the_chats_that_missed_it() {
    let mut r = rig();
    r.bot.fail_after(1, BotError::Transient("blip".into()));
    r.outbox.push(job(Kind::Quit, "x", &[1, 2], &[]));
    r.outbox.drain(Instant::now());
    let after = Arc::new(FakeBot::default());
    let mut reborn = Outbox::new(after.clone(), r.hub.clone(), r.dir.path().to_path_buf(), r.dir.path().join("outbox.json"));
    reborn.now_ms = || 1_000_000;
    reborn.drain(Instant::now());
    assert_eq!(texts(&after).iter().map(|(c, _)| *c).collect::<Vec<_>>(), vec![2]);
}

#[test]
fn nothing_is_restored_once_the_bot_was_disconnected() {
    let mut r = rig();
    r.bot.fail_next(BotError::Transient("offline".into()));
    r.outbox.push(job(Kind::Quit, "x", &[1], &[]));
    r.outbox.drain(Instant::now());
    r.hub.disconnect().unwrap();
    let reborn = Outbox::new(Arc::new(FakeBot::default()), r.hub.clone(), r.dir.path().to_path_buf(), r.dir.path().join("outbox.json"));
    assert!(reborn.queue.is_empty());
}

#[test]
fn a_panic_in_delivery_is_reported_instead_of_killing_the_thread() {
    let r = rig();
    guarded(&r.hub, || panic!("boom"));
    assert!(r.hub.view(0).last_error.unwrap().contains("falha interna"));
}

#[test]
fn a_long_outage_shows_in_the_tab_and_a_short_one_does_not() {
    let mut r = rig();
    for _ in 0..REPORT_AFTER {
        r.bot.fail_next(BotError::Transient("dns".into()));
    }
    r.outbox.push(job(Kind::Tamper, "x", &[1], &[]));
    for _ in 0..REPORT_AFTER - 1 {
        r.outbox.drain(Instant::now());
    }
    assert!(r.hub.view(0).last_error.is_none(), "a blip is routine");
    r.outbox.drain(Instant::now());
    assert!(r.hub.view(0).last_error.unwrap().contains("dns"));
    r.outbox.drain(Instant::now());
    assert!(r.hub.view(0).last_error.is_none(), "the first success clears it");
}

#[test]
fn a_blocked_chat_stays_visible_while_another_chat_keeps_receiving() {
    let mut r = rig();
    pair(&r.hub, 1, "Ana");
    r.bot.fail_next(BotError::Rejected("Forbidden: bot was blocked by the user".into()));
    r.outbox.push(job(Kind::Tamper, "x", &[1, 2], &[]));
    r.outbox.drain(Instant::now());
    assert!(texts(&r.bot).iter().any(|(c, _)| *c == 2));
    assert!(r.hub.view(0).last_error.unwrap().contains("Ana"));
}

struct PanicBot;

impl Bot for PanicBot {
    fn get_me(&self, _: &str) -> Result<Me, BotError> {
        panic!("boom")
    }
    fn get_updates(&self, _: &str, _: i64, _: u64) -> Result<Vec<Update>, BotError> {
        panic!("boom")
    }
    fn send_message(&self, _: &str, _: i64, _: &str) -> Result<(), BotError> {
        panic!("boom")
    }
    fn send_photo(&self, _: &str, _: i64, _: &str, _: Vec<u8>) -> Result<(), BotError> {
        panic!("boom")
    }
}

#[test]
fn a_panic_while_delivering_keeps_the_alert_for_a_few_tries_then_drops_it_loudly() {
    let mut r = rig();
    r.outbox.bot = Arc::new(PanicBot);
    r.outbox.push(job(Kind::Quit, "x", &[1], &[]));
    r.outbox.drain(Instant::now());
    assert_eq!(r.outbox.queue.len(), 1, "not lost on the first panic");
    assert!(r.hub.view(0).last_error.unwrap().contains("falha interna"));
    for _ in 0..PANIC_RETRIES {
        r.outbox.drain(Instant::now());
    }
    assert!(r.outbox.queue.is_empty(), "a poison alert cannot jam the queue forever");
}
