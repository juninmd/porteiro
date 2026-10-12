//! Takes screen and webcam evidence off the caller's thread: a webcam needs seconds to open and warm up,
//! and an unlock must not wait for it. The event is written once the capture finishes.
use crate::{
    capture::{self, Capturer},
    events::{Event, EventLog},
};
use std::{
    path::PathBuf,
    sync::{
        mpsc::{self, Sender},
        Arc,
    },
    thread,
};

/// Runs once the event is complete, with its shots. The recipients were chosen when the event happened.
pub type Notice = Box<dyn FnOnce(&Event) + Send>;
/// How alerts leave the process. `prepare` runs when an event is born (`None`: nobody needs to hear about it);
/// `flush` blocks until what was handed over has been delivered, or given up on, so quitting does not cut it off.
pub struct Notifier {
    pub prepare: Arc<dyn Prepare>,
    pub flush: Arc<dyn Fn() + Send + Sync>,
}

pub trait Prepare: Fn(&Event) -> Option<Notice> + Send + Sync {}
impl<F: Fn(&Event) -> Option<Notice> + Send + Sync> Prepare for F {}

enum Job {
    Capture(Event, Option<Notice>),
    Flush(Sender<()>),
}

pub struct Evidence {
    tx: Sender<Job>,
}

impl Evidence {
    pub fn spawn(mut capturer: Box<dyn Capturer>, mut log: EventLog, shots_dir: PathBuf) -> Evidence {
        let (tx, rx) = mpsc::channel::<Job>();
        thread::spawn(move || {
            for job in rx {
                match job {
                    Job::Capture(mut event, notice) => {
                        let taken = capturer.capture(&shots_dir, &event.id);
                        event.shots = taken.files;
                        if !taken.notes.is_empty() {
                            event.detail.push_str(&format!(" [{}]", taken.notes.join("; ")));
                        }
                        if let Err(e) = log.append(&event) {
                            eprintln!("porteiro: could not record event {}: {e}", event.id);
                        }
                        if let Some(notice) = notice {
                            notice(&event);
                        }
                        if log.needs_prune() {
                            if let Ok(dropped) = log.prune() {
                                capture::remove(&shots_dir, &dropped);
                            }
                        }
                    }
                    Job::Flush(done) => {
                        let _ = done.send(());
                    }
                }
            }
        });
        Evidence { tx }
    }

    /// Queues the event for capture. Hands it back when the worker is gone so the caller can still record it.
    pub fn submit(&self, event: Event, notice: Option<Notice>) -> Result<(), (Event, Option<Notice>)> {
        self.tx.send(Job::Capture(event, notice)).map_err(|e| match e.0 {
            Job::Capture(ev, notice) => (ev, notice),
            Job::Flush(_) => unreachable!("only Capture jobs are submitted here"),
        })
    }

    /// Blocks until everything queued so far has been written. Used by tests and by the quit path.
    pub fn flush(&self) {
        let (tx, rx) = mpsc::channel();
        if self.tx.send(Job::Flush(tx)).is_ok() {
            let _ = rx.recv();
        }
    }
}
