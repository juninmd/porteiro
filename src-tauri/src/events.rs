//! Append-only event log (JSON Lines) that powers the monitoring view. Newest-first reads, bounded size.
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    AppStart,
    UnlockOk,
    AuthFail,
    Lockout,
    Relock,
    SessionLock,
    SessionUnlock,
    Tamper,
    ConfigChange,
    PasswordChange,
    EnforcementError,
    BlockedApp,
    Quit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    pub id: String,
    /// Unix milliseconds, stamped when the event happened (not when its evidence finished).
    pub at_ms: i64,
    pub kind: Kind,
    pub detail: String,
    /// File names inside the shots directory (screen captures and the webcam frame).
    #[serde(default)]
    pub shots: Vec<String>,
}

pub struct EventLog {
    path: PathBuf,
    max_events: usize,
    since_prune: usize,
    /// Shared by every handle on the same file so an append never lands in the middle of a prune rewrite.
    lock: Arc<Mutex<()>>,
}

impl EventLog {
    pub fn new(path: PathBuf, max_events: usize) -> Self {
        EventLog { path, max_events, since_prune: 0, lock: Arc::default() }
    }

    /// Another handle on the same file (the evidence worker owns one).
    pub fn sibling(&self) -> Self {
        EventLog { path: self.path.clone(), max_events: self.max_events, since_prune: 0, lock: self.lock.clone() }
    }

    pub fn append(&mut self, event: &Event) -> io::Result<()> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let mut file = OpenOptions::new().create(true).append(true).open(&self.path)?;
        let mut line = serde_json::to_string(event).map_err(io::Error::other)?;
        line.push('\n');
        file.write_all(line.as_bytes())?;
        self.since_prune += 1;
        Ok(())
    }

    fn read_all(&self) -> Vec<Event> {
        let text = fs::read_to_string(&self.path).unwrap_or_default();
        // A damaged line (crash mid-write) must not hide the rest of the history.
        text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
    }

    /// Newest first.
    pub fn list(&self, offset: usize, limit: usize) -> Vec<Event> {
        let mut all = self.read_all();
        // Evidence is appended after the (slow) capture, so file order is not always time order.
        all.sort_by_key(|e| e.at_ms);
        all.into_iter().rev().skip(offset).take(limit.min(200)).collect()
    }

    pub fn count(&self) -> usize {
        self.read_all().len()
    }

    pub fn needs_prune(&self) -> bool {
        self.since_prune >= 50
    }

    /// Keeps the newest `max_events` and returns the screenshot files that belonged to dropped events.
    pub fn prune(&mut self) -> io::Result<Vec<String>> {
        self.since_prune = 0;
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let all = self.read_all();
        if all.len() <= self.max_events {
            return Ok(Vec::new());
        }
        let cut = all.len() - self.max_events;
        let dropped: Vec<String> = all[..cut].iter().flat_map(|e| e.shots.clone()).collect();
        let body: String = all[cut..].iter().filter_map(|e| serde_json::to_string(e).ok()).map(|l| l + "\n").collect();
        let tmp = self.path.with_extension("jsonl.tmp");
        fs::write(&tmp, body)?;
        fs::rename(&tmp, &self.path)?;
        Ok(dropped)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(n: i64, shots: &[&str]) -> Event {
        Event { id: format!("e{n}"), at_ms: n, kind: Kind::UnlockOk, detail: String::new(), shots: shots.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    fn lists_newest_first_with_paging() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = EventLog::new(dir.path().join("events.jsonl"), 100);
        (1..=5).for_each(|n| log.append(&ev(n, &[])).unwrap());
        assert_eq!(log.list(0, 2).iter().map(|e| e.at_ms).collect::<Vec<_>>(), vec![5, 4]);
        assert_eq!(log.list(4, 10).iter().map(|e| e.at_ms).collect::<Vec<_>>(), vec![1]);
        assert!(log.list(9, 10).is_empty());
    }

    #[test]
    fn prune_keeps_the_newest_and_reports_orphaned_shots() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = EventLog::new(dir.path().join("events.jsonl"), 2);
        log.append(&ev(1, &["a.jpg", "b.jpg"])).unwrap();
        log.append(&ev(2, &["c.jpg"])).unwrap();
        log.append(&ev(3, &["d.jpg"])).unwrap();
        assert_eq!(log.prune().unwrap(), vec!["a.jpg", "b.jpg"]);
        assert_eq!(log.list(0, 10).iter().map(|e| e.at_ms).collect::<Vec<_>>(), vec![3, 2]);
        assert!(log.prune().unwrap().is_empty());
    }

    #[test]
    fn a_corrupt_line_is_skipped_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        let mut log = EventLog::new(path.clone(), 10);
        log.append(&ev(1, &[])).unwrap();
        fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b"{\"id\":\"half\n").unwrap();
        log.append(&ev(2, &[])).unwrap();
        assert_eq!(log.count(), 2);
    }

    #[test]
    fn kinds_serialize_as_snake_case_for_the_ui() {
        assert_eq!(serde_json::to_string(&Kind::SessionUnlock).unwrap(), "\"session_unlock\"");
    }
}
