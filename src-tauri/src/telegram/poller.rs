//! Receives what people send to the bot: pairing codes from strangers, and `/status` and `/bloquear` from
//! paired chats. Outbound long polling only: nothing listens on the PC.
use super::{
    bot::{Bot, BotError, Message},
    hub::{Fault, Hub, PairResult},
};
use crate::events::Kind;
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    sync::Arc,
    thread,
    time::Duration,
};

const LONG_POLL_SECS: u64 = 25;
const IDLE: Duration = Duration::from_secs(2);
const REVOKED_PAUSE: Duration = Duration::from_secs(30);

/// What the poller may ask of the app. Kept small so the handler is tested without a `Core`.
pub trait Remote: Send + Sync {
    /// Monotonic seconds, the same clock the pairing window uses.
    fn now(&self) -> u64;
    fn status_text(&self) -> String;
    /// True when the block had been lifted. Locking never needs a password.
    fn relock(&self) -> bool;
    fn record(&self, kind: Kind, detail: String);
}

pub struct Poller {
    hub: Hub,
    bot: Arc<dyn Bot>,
    remote: Arc<dyn Remote>,
}

/// Offline for a couple of polls is routine; past this the owner sees it in the tab.
const REPORT_AFTER: u32 = 3;

const HELP: &str = "Comandos:\n/status mostra o estado do bloqueio\n/bloquear reativa o bloqueio agora\n/ajuda esta lista";

/// Six digits, spaces allowed ("482 913"); anything else is not a pairing attempt.
fn as_code(text: &str) -> Option<String> {
    let squeezed: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    (squeezed.len() == 6 && squeezed.bytes().all(|b| b.is_ascii_digit())).then_some(squeezed)
}

impl Poller {
    pub fn new(hub: Hub, bot: Arc<dyn Bot>, remote: Arc<dyn Remote>) -> Self {
        Poller { hub, bot, remote }
    }

    fn command(&self, text: &str) -> String {
        let word = text.split_whitespace().next().unwrap_or("").split('@').next().unwrap_or("").to_lowercase();
        match word.as_str() {
            "/status" => self.remote.status_text(),
            "/bloquear" => {
                if self.remote.relock() { "🔒 Bloqueado de novo." } else { "🔒 Já estava bloqueado." }.to_string()
            }
            "/start" | "/ajuda" | "/help" => HELP.to_string(),
            _ => format!("Não entendi. {HELP}"),
        }
    }

    fn stranger(&self, message: &Message, text: &str) -> Option<String> {
        let (id, name, now) = (message.chat.id, message.chat.display_name(), self.remote.now());
        let outcome = match as_code(text) {
            Some(code) => self.hub.try_pair(id, &name, &code, now),
            None if self.hub.pairing_open(now) => return Some("Envie o código de 6 dígitos que o Porteiro mostra na tela.".into()),
            None => Ok(PairResult::Idle),
        };
        match outcome {
            Ok(PairResult::Paired) => {
                self.remote.record(Kind::ConfigChange, format!("Telegram pareado com {name}"));
                Some(format!("✅ Pareado. Você receberá os alertas do Porteiro aqui.\n{HELP}"))
            }
            Ok(PairResult::Wrong(left)) => Some(format!("Código incorreto. Restam {left} tentativas.")),
            Ok(PairResult::Closed) => {
                self.remote.record(Kind::Tamper, format!("pareamento do Telegram encerrado por códigos errados ({name})"));
                Some("Pareamento encerrado por tentativas demais. Gere outro código no Porteiro.".into())
            }
            Ok(PairResult::Idle) => {
                // Silent on purpose: replying would turn the bot into a free echo for anyone who finds it.
                if self.hub.report_unknown(id, now) {
                    self.remote.record(Kind::Tamper, format!("mensagem de chat não pareado no bot do Telegram ({name})"));
                }
                None
            }
            Err(why) => {
                eprintln!("porteiro: pareamento do Telegram falhou: {why}");
                None
            }
        }
    }

    /// The reply for one incoming message, if any. Only private chats are served: a group would show the alerts,
    /// and their photos, to everyone in it.
    pub fn handle(&self, message: &Message) -> Option<String> {
        if message.chat.kind != "private" {
            return None;
        }
        let text = message.text.as_deref().unwrap_or("").trim();
        if self.hub.is_paired(message.chat.id) { Some(self.command(text)) } else { self.stranger(message, text) }
    }

    /// One poll: fetch, answer, and move the offset past what was handled.
    pub fn poll_once(&self) -> Result<(), BotError> {
        let Some((token, offset)) = self.hub.polling() else { return Ok(()) };
        let updates = self.bot.get_updates(&token, offset, LONG_POLL_SECS)?;
        self.hub.set_fault(Fault::Poll, None);
        for update in updates {
            // A reconnect during the long poll means these belong to the old bot: leave them.
            if self.hub.polling().map(|(t, _)| t).as_deref() != Some(token.as_str()) {
                break;
            }
            if let Some(reply) = update.message.as_ref().and_then(|m| self.handle(m).map(|r| (m.chat.id, r))) {
                let _ = self.bot.send_message(&token, reply.0, &reply.1);
            }
            // Without a saved offset the same batch would come straight back: let the backoff slow that down.
            if let Err(why) = self.hub.advance_offset(&token, update.update_id + 1) {
                return Err(BotError::Transient(why));
            }
        }
        Ok(())
    }

    fn note_transient(&self, failures: &mut u32, err: &BotError) {
        *failures += 1;
        if *failures >= REPORT_AFTER {
            self.hub.set_fault(Fault::Poll, Some(err.describe()));
        }
    }

    fn run(self) {
        let (mut backoff, mut failures) = (1u64, 0u32);
        loop {
            if self.hub.polling().is_none() {
                thread::sleep(IDLE);
                continue;
            }
            // A panic (a broken TLS setup, say) is reported and retried, not left to kill the thread silently.
            let polled = catch_unwind(AssertUnwindSafe(|| self.poll_once())).unwrap_or_else(|_| Err(BotError::Transient("falha interna ao consultar o Telegram".into())));
            match polled {
                Ok(()) => (backoff, failures) = (1, 0),
                Err(BotError::Unauthorized) => {
                    self.hub.set_fault(Fault::Poll, Some("o Telegram recusou o token: conecte o bot de novo".into()));
                    thread::sleep(REVOKED_PAUSE);
                }
                Err(BotError::RetryAfter(secs)) => thread::sleep(Duration::from_secs(secs.min(300))),
                Err(err @ BotError::Rejected(_)) => {
                    // Typically a 409: the same token is polled somewhere else, or a webhook is set.
                    self.hub.set_fault(Fault::Poll, Some(err.describe()));
                    thread::sleep(Duration::from_secs(backoff));
                    backoff = (backoff * 2).min(60);
                }
                Err(err @ BotError::Transient(_)) => {
                    self.note_transient(&mut failures, &err);
                    thread::sleep(Duration::from_secs(backoff));
                    backoff = (backoff * 2).min(60);
                }
            }
        }
    }
}

pub fn spawn(hub: Hub, bot: Arc<dyn Bot>, remote: Arc<dyn Remote>) {
    let poller = Poller::new(hub, bot, remote);
    thread::spawn(move || poller.run());
}

#[cfg(test)]
mod tests;
