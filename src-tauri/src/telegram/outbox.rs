//! Delivers alerts in the background. An alert that cannot go out yet (offline, rate limit) waits and is retried in
//! order, and the wait list survives a restart; one that can never go out (blocked bot, revoked token) is dropped
//! so it cannot jam the queue, and the owner is told why in the Telegram tab.
use super::{
    bot::{Bot, BotError},
    format::{self, Alert},
    hub::{Fault, Hub},
    settings::{Alerts, Target},
};
use crate::{capture, config, events::Event};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    panic::{catch_unwind, AssertUnwindSafe},
    path::{Path, PathBuf},
    sync::{
        mpsc::{self, RecvTimeoutError},
        Arc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub struct Job {
    pub target: Target,
    pub event: Event,
}

pub enum Msg {
    Alert(Job),
    /// Answered once the queue is empty or the flush budget ran out.
    Flush(mpsc::Sender<()>),
}

const QUEUE_CAP: usize = 100;
/// How long quitting waits for the last alerts; offline, it gives up rather than hold the exit.
const FLUSH_BUDGET: Duration = Duration::from_secs(10);
const FLUSH_PAUSE: Duration = Duration::from_millis(300);
/// Offline for a couple of attempts is routine; past this the owner sees it in the tab.
const REPORT_AFTER: u32 = 3;
/// A panic in delivery is a bug: the alert is tried again a couple of times, then dropped with a visible fault.
const PANIC_RETRIES: u32 = 3;
/// An alert this old is stale news; the event log still has it.
const GIVE_UP_AFTER_MS: i64 = 6 * 3600 * 1000;

struct Pending {
    target: Target,
    event: Event,
    /// Chats still waiting for this alert; a retry never repeats it to the ones that already got it.
    todo: Vec<i64>,
    attempts: u32,
    due: Instant,
}

/// What survives a restart. The token is deliberately not here: it is read from the hub when the alert is sent.
#[derive(Serialize, Deserialize)]
struct Stored {
    event: Event,
    chats: Vec<i64>,
    alerts: Alerts,
}

enum Step {
    Done,
    Wait(Duration),
}

fn default_backoff(attempt: u32) -> Duration {
    Duration::from_secs((1u64 << attempt.min(6)).min(60))
}

fn system_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

pub struct Outbox {
    bot: Arc<dyn Bot>,
    hub: Hub,
    shots: PathBuf,
    store: PathBuf,
    backoff: fn(u32) -> Duration,
    now_ms: fn() -> i64,
    flush_budget: Duration,
    queue: VecDeque<Pending>,
    dirty: bool,
}

fn send(bot: &dyn Bot, shots: &Path, token: &str, chat: i64, event: &Event, alert: &Alert) -> Result<(), BotError> {
    if alert.photo {
        if let Some(jpeg) = format::cam_shot(event).and_then(|name| capture::read(shots, name).ok()) {
            match bot.send_photo(token, chat, &alert.text, jpeg) {
                // The photo itself was refused: the text is still worth sending.
                Err(BotError::Rejected(_)) => (),
                other => return other,
            }
        }
    }
    bot.send_message(token, chat, &alert.text)
}

/// A panic in the delivery path (a broken TLS setup, say) must not silently kill the thread: it is reported.
fn guarded(hub: &Hub, f: impl FnOnce()) {
    if catch_unwind(AssertUnwindSafe(f)).is_err() {
        hub.set_fault(Fault::Send, Some("falha interna ao enviar alertas do Telegram".into()));
    }
}

impl Outbox {
    pub fn new(bot: Arc<dyn Bot>, hub: Hub, shots: PathBuf, store: PathBuf) -> Self {
        let mut outbox = Outbox { bot, hub, shots, store, backoff: default_backoff, now_ms: system_ms, flush_budget: FLUSH_BUDGET, queue: VecDeque::new(), dirty: false };
        outbox.restore();
        outbox
    }

    /// Alerts that were still waiting when the app last stopped. Without a token there is nobody to send to.
    fn restore(&mut self) {
        let Some(token) = self.hub.token() else { return };
        let Ok(bytes) = std::fs::read(&self.store) else { return };
        let stored: Vec<Stored> = serde_json::from_slice(&bytes).unwrap_or_default();
        for s in stored.into_iter().rev().take(QUEUE_CAP).rev() {
            let target = Target { token: token.clone(), chats: s.chats.clone(), alerts: s.alerts };
            self.queue.push_back(Pending { target, event: s.event, todo: s.chats, attempts: 0, due: Instant::now() });
        }
    }

    fn persist(&mut self) {
        if !std::mem::take(&mut self.dirty) {
            return;
        }
        let stored: Vec<Stored> = self.queue.iter().map(|p| Stored { event: p.event.clone(), chats: p.todo.clone(), alerts: p.target.alerts }).collect();
        if let Err(e) = config::write_json(&self.store, &stored) {
            eprintln!("porteiro: could not save the alert queue: {e}");
        }
    }

    pub fn push(&mut self, job: Job) {
        if self.queue.len() >= QUEUE_CAP {
            self.queue.pop_front();
        }
        let todo = job.target.chats.clone();
        self.queue.push_back(Pending { target: job.target, event: job.event, todo, attempts: 0, due: Instant::now() });
        self.dirty = true;
        self.persist();
    }

    fn fail(&self, chat: i64, why: &str) {
        let who = self.hub.chat_name(chat).unwrap_or_else(|| format!("chat {chat}"));
        self.hub.set_fault(Fault::Send, Some(format!("o Telegram recusou o alerta para {who}: {why}")));
    }

    fn deliver(&self, p: &mut Pending) -> Step {
        let now_ms = (self.now_ms)();
        let Some(alert) = format::alert(&p.event, &p.target.alerts, now_ms) else { return Step::Done };
        if now_ms - p.event.at_ms > GIVE_UP_AFTER_MS {
            self.hub.set_fault(Fault::Send, Some("um alerta ficou 6 h sem sair e foi descartado: veja o Registro".into()));
            return Step::Done;
        }
        let (mut wait, mut delivered, mut rejected, mut todo) = (None, false, false, Vec::new());
        // `p.todo` stays whole until the pass ends, so a panic halfway repeats an alert rather than losing it.
        for &chat in &p.todo {
            match send(&*self.bot, &self.shots, &p.target.token, chat, &p.event, &alert) {
                Ok(()) => delivered = true,
                Err(BotError::Rejected(why)) => {
                    rejected = true;
                    self.fail(chat, &why);
                }
                Err(BotError::Unauthorized) => {
                    self.hub.set_fault(Fault::Send, Some("o Telegram recusou o token: conecte o bot de novo".into()));
                    return Step::Done;
                }
                Err(BotError::RetryAfter(secs)) => {
                    todo.push(chat);
                    wait = Some(Duration::from_secs(secs));
                }
                Err(err @ BotError::Transient(_)) => {
                    if p.attempts + 1 >= REPORT_AFTER {
                        self.hub.set_fault(Fault::Send, Some(err.describe()));
                    }
                    todo.push(chat);
                    wait.get_or_insert(Duration::ZERO);
                }
            }
        }
        // Another chat taking the alert must not hide that one refused it in the same pass.
        if delivered && !rejected {
            self.hub.set_fault(Fault::Send, None);
        }
        p.todo = todo;
        if p.todo.is_empty() { Step::Done } else { Step::Wait(wait.unwrap_or_default()) }
    }

    /// Delivers what is due, oldest first, and stops at the first alert that has to wait so the order holds.
    pub fn drain(&mut self, now: Instant) {
        while let Some(mut front) = self.queue.pop_front() {
            if front.due > now {
                self.queue.push_front(front);
                break;
            }
            self.dirty = true;
            match catch_unwind(AssertUnwindSafe(|| self.deliver(&mut front))) {
                Ok(Step::Done) => (),
                Ok(Step::Wait(extra)) => {
                    front.attempts += 1;
                    front.due = now + (self.backoff)(front.attempts).max(extra);
                    self.queue.push_front(front);
                    break;
                }
                Err(_) => {
                    self.hub.set_fault(Fault::Send, Some("falha interna ao enviar alertas do Telegram".into()));
                    front.attempts += 1;
                    if front.attempts < PANIC_RETRIES {
                        front.due = now + (self.backoff)(front.attempts);
                        self.queue.push_front(front);
                        break;
                    }
                }
            }
        }
        self.persist();
    }

    /// Retries everything now, backoff aside, until the queue is empty or the budget is spent.
    fn flush(&mut self) {
        let end = Instant::now() + self.flush_budget;
        loop {
            let now = Instant::now();
            self.queue.iter_mut().for_each(|p| p.due = now);
            self.drain(now);
            if self.queue.is_empty() || Instant::now() >= end {
                return;
            }
            thread::sleep(FLUSH_PAUSE);
        }
    }

    fn next_due(&self) -> Option<Instant> {
        self.queue.front().map(|p| p.due)
    }

    fn run(mut self, rx: mpsc::Receiver<Msg>) {
        let hub = self.hub.clone();
        loop {
            let msg = match self.next_due() {
                None => match rx.recv() {
                    Ok(msg) => Some(msg),
                    Err(_) => return,
                },
                Some(due) => match rx.recv_timeout(due.saturating_duration_since(Instant::now())) {
                    Ok(msg) => Some(msg),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return,
                },
            };
            match msg {
                Some(Msg::Alert(job)) => guarded(&hub, || self.push(job)),
                Some(Msg::Flush(done)) => {
                    guarded(&hub, || self.flush());
                    let _ = done.send(());
                }
                None => (),
            }
            guarded(&hub, || self.drain(Instant::now()));
        }
    }
}

pub fn spawn(bot: Arc<dyn Bot>, hub: Hub, shots: PathBuf, store: PathBuf) -> mpsc::Sender<Msg> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || Outbox::new(bot, hub, shots, store).run(rx));
    tx
}

#[cfg(test)]
mod tests;
