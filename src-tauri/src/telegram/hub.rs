//! Shared state of the integration: the paired chats, the token, the pairing code and the alert switches.
//! Threads share it through `Hub`; its lock is never held across a network call or a call into `Core`.
use super::settings::{Alerts, Chat, Settings, Target};
use crate::config::{self, Loaded};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
};

pub const MAX_CHATS: usize = 3;
pub const PAIRING_TTL_SECS: u64 = 300;
/// The bot username is public, so the 6-digit code is guarded by a guess budget, not only by its short life.
pub(super) const MAX_WRONG_CODES: u32 = 5;
pub(super) const UNKNOWN_WINDOW_SECS: u64 = 600;
const UNKNOWN_BURST: usize = 5;

struct Pairing {
    code: String,
    expires: u64,
    wrong: u32,
}

/// Which side of the integration is failing, so one side recovering does not hide the other's problem.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    Poll,
    Send,
}

#[derive(Default)]
struct Inner {
    settings: Settings,
    pairing: Option<Pairing>,
    poll_error: Option<String>,
    send_error: Option<String>,
    /// Something wrong found while loading; the core turns it into an event once, at start.
    startup_note: Option<String>,
    /// Strangers already reported, by chat, so a spammer cannot turn the bot into an alert flood.
    unknown: HashMap<i64, u64>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PairResult {
    Paired,
    Wrong(u32),
    /// Too many wrong codes: the pairing window was shut.
    Closed,
    /// No pairing is open (or it expired).
    Idle,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct PairingView {
    pub code: String,
    pub secs_left: u64,
}

/// What the UI sees. The token never appears here.
#[derive(Debug, Serialize, PartialEq)]
pub struct View {
    pub connected: bool,
    pub bot_username: Option<String>,
    pub chats: Vec<Chat>,
    pub alerts: Alerts,
    pub pairing: Option<PairingView>,
    pub last_error: Option<String>,
}

#[derive(Clone)]
pub struct Hub {
    path: Arc<PathBuf>,
    inner: Arc<Mutex<Inner>>,
}

fn new_code() -> String {
    let bytes = *uuid::Uuid::new_v4().as_bytes();
    let n = u64::from_le_bytes(bytes[..8].try_into().expect("8 bytes")) % 1_000_000;
    format!("{n:06}")
}

impl Hub {
    fn with(path: PathBuf, settings: Settings, startup_note: Option<String>) -> Hub {
        Hub { path: Arc::new(path), inner: Arc::new(Mutex::new(Inner { settings, startup_note, ..Inner::default() })) }
    }

    pub fn load(path: PathBuf) -> Result<Hub, String> {
        let (settings, note) = match config::load_or_create(&path, || Ok(Settings::default()))? {
            Loaded::Reset(s) => (s, Some("telegram.json estava ilegível: o Telegram foi desconectado".to_string())),
            other => (other.into_inner(), None),
        };
        Ok(Hub::with(path, settings, note))
    }

    /// The optional integration must never stop the guard from starting: when its file cannot be read or written,
    /// the hub runs empty and says why.
    pub fn detached(path: PathBuf, why: &str) -> Hub {
        let hub = Hub::with(path, Settings::default(), Some(format!("telegram.json inacessível: {why}")));
        hub.set_fault(Fault::Poll, Some(format!("não foi possível ler as configurações do Telegram: {why}")));
        hub
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Applies `f` to a copy and keeps it only once it is on disk, so memory never gets ahead of the file.
    fn commit<R>(&self, g: &mut Inner, f: impl FnOnce(&mut Settings) -> R) -> Result<R, String> {
        let mut next = g.settings.clone();
        let out = f(&mut next);
        if next != g.settings {
            config::write_json(&self.path, &next).map_err(|e| format!("não foi possível salvar o Telegram: {e}"))?;
            g.settings = next;
        }
        Ok(out)
    }

    pub fn view(&self, now: u64) -> View {
        let g = self.lock();
        View {
            connected: g.settings.token.is_some(),
            bot_username: g.settings.bot_username.clone(),
            chats: g.settings.chats.clone(),
            alerts: g.settings.alerts,
            pairing: g.pairing.as_ref().filter(|p| now < p.expires).map(|p| PairingView { code: p.code.clone(), secs_left: p.expires - now }),
            last_error: g.send_error.clone().or_else(|| g.poll_error.clone()),
        }
    }

    pub fn take_startup_note(&self) -> Option<String> {
        self.lock().startup_note.take()
    }

    /// `None` unless there is a token and someone to tell.
    pub fn targets(&self) -> Option<Target> {
        let g = self.lock();
        let token = g.settings.token.clone()?;
        (!g.settings.chats.is_empty()).then(|| Target { token, chats: g.settings.chats.iter().map(|c| c.id).collect(), alerts: g.settings.alerts })
    }

    pub fn token(&self) -> Option<String> {
        self.lock().settings.token.clone()
    }

    pub fn chat_name(&self, chat: i64) -> Option<String> {
        self.lock().settings.chats.iter().find(|c| c.id == chat).map(|c| c.name.clone())
    }

    /// Token and offset for the next poll.
    pub fn polling(&self) -> Option<(String, i64)> {
        let g = self.lock();
        g.settings.token.clone().map(|t| (t, g.settings.offset))
    }

    pub fn is_paired(&self, chat: i64) -> bool {
        self.lock().settings.chats.iter().any(|c| c.id == chat)
    }

    pub fn pairing_open(&self, now: u64) -> bool {
        self.lock().pairing.as_ref().is_some_and(|p| now < p.expires)
    }

    /// A different bot means different chats: the old pairings do not carry over, the alert switches do.
    pub fn connect(&self, token: &str, username: &str) -> Result<(), String> {
        let mut g = self.lock();
        self.commit(&mut g, |s| {
            s.token = Some(token.to_string());
            s.bot_username = Some(username.to_string());
            s.chats.clear();
            s.offset = 0;
        })?;
        g.pairing = None;
        g.poll_error = None;
        g.send_error = None;
        Ok(())
    }

    pub fn disconnect(&self) -> Result<(), String> {
        let mut g = self.lock();
        let alerts = g.settings.alerts;
        self.commit(&mut g, |s| *s = Settings { alerts, ..Settings::default() })?;
        g.pairing = None;
        g.poll_error = None;
        g.send_error = None;
        Ok(())
    }

    /// The only place the chat limit is enforced: a pairing cannot be open while the list is full.
    pub fn start_pairing(&self, now: u64) -> Result<(), String> {
        let mut g = self.lock();
        if g.settings.token.is_none() {
            return Err("conecte um bot antes de parear".into());
        }
        if g.settings.chats.len() >= MAX_CHATS {
            return Err(format!("limite de {MAX_CHATS} chats: remova um antes de parear outro"));
        }
        g.pairing = Some(Pairing { code: new_code(), expires: now + PAIRING_TTL_SECS, wrong: 0 });
        Ok(())
    }

    /// `code` is a candidate the sender typed; the caller already checked it looks like one.
    pub fn try_pair(&self, chat: i64, name: &str, code: &str, now: u64) -> Result<PairResult, String> {
        let mut g = self.lock();
        let Some(p) = g.pairing.as_mut().filter(|p| now < p.expires) else {
            g.pairing = None;
            return Ok(PairResult::Idle);
        };
        if p.code != code {
            p.wrong += 1;
            let wrong = p.wrong;
            if wrong >= MAX_WRONG_CODES {
                g.pairing = None;
                return Ok(PairResult::Closed);
            }
            return Ok(PairResult::Wrong(MAX_WRONG_CODES - wrong));
        }
        self.commit(&mut g, |s| {
            if !s.chats.iter().any(|c| c.id == chat) {
                s.chats.push(Chat { id: chat, name: name.to_string() });
            }
        })?;
        g.pairing = None;
        Ok(PairResult::Paired)
    }

    pub fn remove_chat(&self, chat: i64) -> Result<bool, String> {
        let mut g = self.lock();
        self.commit(&mut g, |s| {
            let before = s.chats.len();
            s.chats.retain(|c| c.id != chat);
            s.chats.len() != before
        })
    }

    pub fn set_alerts(&self, alerts: Alerts) -> Result<(), String> {
        let mut g = self.lock();
        self.commit(&mut g, |s| s.alerts = alerts)
    }

    /// Only moves forward, and only for the token that was polled: a reconnect in the middle of a long poll
    /// must not inherit the old bot's position.
    pub fn advance_offset(&self, token: &str, next: i64) -> Result<(), String> {
        let mut g = self.lock();
        if g.settings.token.as_deref() != Some(token) || next <= g.settings.offset {
            return Ok(());
        }
        self.commit(&mut g, |s| s.offset = next)
    }

    pub fn set_fault(&self, fault: Fault, message: Option<String>) {
        let mut g = self.lock();
        match fault {
            Fault::Poll => g.poll_error = message,
            Fault::Send => g.send_error = message,
        }
    }

    /// True when a stranger's message is worth an event: once per chat per window, and a few per window overall.
    pub fn report_unknown(&self, chat: i64, now: u64) -> bool {
        let mut g = self.lock();
        g.unknown.retain(|_, at| now < *at + UNKNOWN_WINDOW_SECS);
        if g.unknown.contains_key(&chat) || g.unknown.len() >= UNKNOWN_BURST {
            return false;
        }
        g.unknown.insert(chat, now);
        true
    }
}

#[cfg(test)]
mod tests;
