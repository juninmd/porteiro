//! Takes screen and webcam evidence off the caller's thread: a webcam needs seconds to open and warm up,
//! and an unlock must not wait for it. The event is written once the capture finishes.
use crate::{
    capture::{self, Capturer},
    events::{Event, EventLog},
};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Sender},
    thread,
};

enum Job {
    Capture(Event),
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
                    Job::Capture(mut event) => {
                        let taken = capturer.capture(&shots_dir, &event.id);
                        event.shots = taken.files;
                        if !taken.notes.is_empty() {
                            event.detail.push_str(&format!(" [{}]", taken.notes.join("; ")));
                        }
                        if let Err(e) = log.append(&event) {
                            eprintln!("porteiro: could not record event {}: {e}", event.id);
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
    pub fn submit(&self, event: Event) -> Result<(), Event> {
        self.tx.send(Job::Capture(event)).map_err(|e| match e.0 {
            Job::Capture(ev) => ev,
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
