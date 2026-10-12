//! Telegram integration: alerts with the webcam frame go to paired chats, and those chats can ask for `/status`
//! or `/bloquear`. The evidence leaves the PC, so wiping the local log afterwards does not erase it.
mod bot;
#[cfg(test)]
mod fake;
mod format;
mod http;
mod hub;
mod outbox;
mod poller;
mod remote;
mod settings;

pub use bot::{valid_token, Bot, BotError};
pub use format::{relevant, status_text};
pub use http::{api_base, HttpBot};
pub use hub::{Hub, View};
pub use outbox::{spawn as spawn_outbox, Job, Msg};
pub use poller::{spawn as spawn_poller, Remote};
pub use settings::Alerts;

use crate::evidence::{Notice, Notifier};
use std::{
    sync::{
        mpsc::{self, Sender},
        Arc,
    },
    time::Duration,
};

/// Longer than the outbox's own flush budget, so the caller hears "done" instead of racing it.
const FLUSH_WAIT: Duration = Duration::from_secs(12);

/// Hooks the outbox into the event pipeline. Recipients are fixed when the event is born, not when its evidence
/// finishes, so disconnecting right after still tells the chats that were there.
pub fn notifier(hub: Hub, tx: Sender<Msg>) -> Notifier {
    let alerts = tx.clone();
    let prepare = Arc::new(move |event: &crate::events::Event| {
        if !relevant(event.kind) {
            return None;
        }
        let target = hub.targets()?;
        let tx = alerts.clone();
        let notice: Notice = Box::new(move |done| {
            let _ = tx.send(Msg::Alert(Job { target, event: done.clone() }));
        });
        Some(notice)
    });
    let flush = Arc::new(move || {
        let (done, waiting) = mpsc::channel();
        if tx.send(Msg::Flush(done)).is_ok() {
            let _ = waiting.recv_timeout(FLUSH_WAIT);
        }
    });
    Notifier { prepare, flush }
}
