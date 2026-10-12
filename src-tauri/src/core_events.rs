//! The event trail on `Core`: stamping, recording with evidence, and who gets told.
use crate::{
    core::Core,
    events::{Event, Kind},
    evidence::Notice,
    session::Transition,
};

impl Core {
    pub fn list_events(&self, offset: usize, limit: usize) -> Vec<Event> {
        self.events.list(offset, limit)
    }

    pub fn shots_dir(&self) -> std::path::PathBuf {
        self.paths.shots()
    }

    /// Monotonic seconds, the clock every in-process deadline uses.
    pub fn mono(&self) -> u64 {
        self.clock.mono()
    }

    /// Records an event. With `capture`, screen and webcam evidence is attached by the worker thread.
    pub(crate) fn log(&mut self, kind: Kind, detail: impl Into<String>, capture: bool) {
        let born = self.born(kind, detail.into());
        self.record(born, capture);
    }

    /// Stamps an event and fixes who will hear about it. A change that removes recipients is logged from here
    /// before it is applied, so the people being removed still get told.
    pub(crate) fn born(&self, kind: Kind, detail: String) -> (Event, Option<Notice>) {
        let event = Event { id: uuid::Uuid::new_v4().to_string(), at_ms: self.clock.wall_ms(), kind, detail, shots: Vec::new() };
        let notice = self.notifier.as_ref().and_then(|n| (n.prepare)(&event));
        (event, notice)
    }

    pub(crate) fn record(&mut self, born: (Event, Option<Notice>), capture: bool) {
        let (event, notice) = if capture {
            match self.evidence.submit(born.0, born.1) {
                Ok(()) => return,
                Err(back) => back,
            }
        } else {
            born
        };
        if let Err(e) = self.events.append(&event) {
            eprintln!("porteiro: could not record event: {e}");
        }
        if let Some(notice) = notice {
            notice(&event);
        }
    }

    pub fn session_event(&mut self, t: Transition) {
        match t {
            Transition::Unlocked => self.log(Kind::SessionUnlock, "Windows desbloqueado", true),
            Transition::Locked => self.log(Kind::SessionLock, "Windows bloqueado", false),
        }
    }

    /// Everything queued for capture is on disk, and its alerts delivered or given up on, before the app lets go
    /// of the process. Offline this can wait up to the outbox's flush budget, so it is for quitting and tests only.
    pub fn flush_evidence(&self) {
        self.evidence.flush();
        if let Some(n) = &self.notifier {
            (n.flush)();
        }
    }
}
