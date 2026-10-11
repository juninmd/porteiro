//! Fakes and a ready-made `Core` for tests: hosts redirected to a temp file, a controllable clock, no hardware.
use crate::{
    api::Clock,
    blocklist::Blocklist,
    capture::{Capture, Capturer},
    config::Paths,
    core::{Core, Deps},
    events::{Event, EventLog, Kind},
    evidence::Evidence,
    layers::{AppKiller, HostsLayer, Layer},
};
use std::{
    fs,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
        Arc, Mutex,
    },
};

pub const START_WALL: i64 = 1_800_000_000;

pub struct FakeClock {
    pub mono: Arc<AtomicU64>,
    pub wall: Arc<AtomicI64>,
    pub ticks: AtomicU64,
}

impl Clock for FakeClock {
    fn wall(&self) -> i64 {
        self.wall.load(Ordering::SeqCst)
    }
    /// Advances on every call, like a real millisecond clock, so same-second events are still ordered.
    fn wall_ms(&self) -> i64 {
        self.wall() * 1000 + self.ticks.fetch_add(1, Ordering::SeqCst) as i64
    }
    fn mono(&self) -> u64 {
        self.mono.load(Ordering::SeqCst)
    }
}

pub struct FakeCapturer(pub Arc<Mutex<Vec<String>>>);

impl Capturer for FakeCapturer {
    fn capture(&mut self, _dir: &Path, id: &str) -> Capture {
        // Real captures take seconds; a slow fake is what lets tests see events finish out of order.
        std::thread::sleep(std::time::Duration::from_millis(80));
        self.0.lock().unwrap().push(id.to_string());
        Capture { files: vec![format!("{id}-cam.jpg")], notes: Vec::new() }
    }
}

pub struct FakeKiller(pub Arc<Mutex<Vec<String>>>);

impl AppKiller for FakeKiller {
    fn kill(&mut self, names: &[String]) -> Vec<String> {
        let mut running = self.0.lock().unwrap();
        let (hit, keep): (Vec<String>, Vec<String>) = running.drain(..).partition(|r| names.iter().any(|n| n.eq_ignore_ascii_case(r)));
        *running = keep;
        hit
    }
}

/// A layer whose failures the test controls.
pub struct FlakyLayer(pub Arc<AtomicBool>);

impl Layer for FlakyLayer {
    fn name(&self) -> &'static str {
        "flaky"
    }
    fn needs_admin(&self) -> bool {
        false
    }
    fn apply(&mut self, _: &Blocklist, _: bool) -> Result<(), String> {
        if self.0.load(Ordering::SeqCst) { Err("boom".into()) } else { Ok(()) }
    }
    fn in_state(&mut self, _: &Blocklist, _: bool) -> Result<bool, String> {
        Ok(!self.0.load(Ordering::SeqCst))
    }
}

pub struct Harness {
    pub core: Core,
    pub dir: tempfile::TempDir,
    pub mono: Arc<AtomicU64>,
    pub wall: Arc<AtomicI64>,
    pub running: Arc<Mutex<Vec<String>>>,
    pub flaky: Arc<AtomicBool>,
}

fn build(dir: &tempfile::TempDir, mono: &Arc<AtomicU64>, wall: &Arc<AtomicI64>, running: &Arc<Mutex<Vec<String>>>, flaky: &Arc<AtomicBool>) -> Core {
    let paths = Paths { data: dir.path().join("data"), hosts: dir.path().join("hosts"), hosts_overridden: true, sandboxed: true };
    let layers: Vec<Box<dyn Layer>> = vec![Box::new(FlakyLayer(flaky.clone())), Box::new(HostsLayer { path: paths.hosts.clone(), sandboxed: true })];
    fs::create_dir_all(paths.shots()).unwrap();
    let evidence = Evidence::spawn(Box::new(FakeCapturer(Arc::default())), EventLog::new(paths.events(), 5000), paths.shots());
    Core::new(Deps {
        paths,
        layers,
        evidence,
        killer: Box::new(FakeKiller(running.clone())),
        clock: Box::new(FakeClock { mono: mono.clone(), wall: wall.clone(), ticks: AtomicU64::new(0) }),
        elevated: true,
        manage_watchdog: false,
    })
    .expect("core starts")
}

pub fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("hosts"), "127.0.0.1 localhost\r\n").unwrap();
    let (mono, wall) = (Arc::new(AtomicU64::new(100)), Arc::new(AtomicI64::new(START_WALL)));
    let (running, flaky) = (Arc::new(Mutex::new(Vec::new())), Arc::new(AtomicBool::new(false)));
    let core = build(&dir, &mono, &wall, &running, &flaky);
    Harness { core, dir, mono, wall, running, flaky }
}

impl Harness {
    pub fn hosts_text(&self) -> String {
        fs::read_to_string(self.dir.path().join("hosts")).unwrap()
    }

    pub fn advance(&self, secs: u64) {
        self.mono.fetch_add(secs, Ordering::SeqCst);
        self.wall.fetch_add(secs as i64, Ordering::SeqCst);
    }

    pub fn events(&self) -> Vec<Event> {
        self.core.flush_evidence();
        self.core.list_events(0, 200)
    }

    pub fn kinds(&self) -> Vec<Kind> {
        self.events().into_iter().map(|e| e.kind).collect()
    }

    /// Simulates quitting and relaunching the app on the same disk, with the clocks as they are now.
    pub fn restart(self) -> Harness {
        self.core.flush_evidence();
        let Harness { core, dir, mono, wall, running, flaky } = self;
        drop(core);
        let core = build(&dir, &mono, &wall, &running, &flaky);
        Harness { core, dir, mono, wall, running, flaky }
    }
}
