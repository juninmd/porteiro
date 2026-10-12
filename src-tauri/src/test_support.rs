//! Fakes and a ready-made `Core` for tests: hosts redirected to a temp file, a controllable clock, no hardware.
use crate::{
    api::Clock,
    blocklist::Blocklist,
    capture::{Capture, Capturer},
    auth,
    config::{write_json, Config, Paths},
    core::{Core, Deps},
    events::{Event, Kind},
    layers::{AppKiller, HostsLayer, Layer},
    telegram::{self, Hub, Job, Msg},
    watchdog::Restarter,
};
use std::{
    fs,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
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

/// Stands in for the scheduled task: records the calls and fails removal on demand.
#[derive(Default)]
pub struct FakeTask {
    pub fail_remove: AtomicBool,
    pub removed: AtomicBool,
}

pub struct FakeRestarter(pub Arc<FakeTask>);

impl Restarter for FakeRestarter {
    fn install(&mut self, _: &Path) -> Result<(), String> {
        Ok(())
    }
    fn remove(&mut self) -> Result<(), String> {
        if self.0.fail_remove.load(Ordering::SeqCst) {
            return Err("access denied".into());
        }
        self.0.removed.store(true, Ordering::SeqCst);
        Ok(())
    }
}

pub struct Harness {
    pub core: Core,
    pub dir: tempfile::TempDir,
    pub mono: Arc<AtomicU64>,
    pub wall: Arc<AtomicI64>,
    pub running: Arc<Mutex<Vec<String>>>,
    pub flaky: Arc<AtomicBool>,
    pub task: Arc<FakeTask>,
    /// The Telegram settings, and what the notifier handed to the outbox (nothing until a chat is paired).
    pub hub: Hub,
    pub jobs: Receiver<Job>,
    tx: Sender<Msg>,
}

fn build(dir: &tempfile::TempDir, mono: &Arc<AtomicU64>, wall: &Arc<AtomicI64>, running: &Arc<Mutex<Vec<String>>>, flaky: &Arc<AtomicBool>, task: &Arc<FakeTask>, tx: &Sender<Msg>) -> (Core, Hub) {
    let paths = Paths { data: dir.path().join("data"), hosts: dir.path().join("hosts"), hosts_overridden: true, sandboxed: true };
    let layers: Vec<Box<dyn Layer>> = vec![Box::new(FlakyLayer(flaky.clone())), Box::new(HostsLayer { path: paths.hosts.clone(), sandboxed: true })];
    fs::create_dir_all(paths.shots()).unwrap();
    let telegram = Hub::load(paths.telegram()).unwrap();
    let notifier = telegram::notifier(telegram.clone(), tx.clone());
    let core = Core::new(Deps {
        paths,
        telegram: telegram.clone(),
        notifier: Some(notifier),
        layers,
        capturer: Box::new(FakeCapturer(Arc::default())),
        killer: Box::new(FakeKiller(running.clone())),
        clock: Box::new(FakeClock { mono: mono.clone(), wall: wall.clone(), ticks: AtomicU64::new(0) }),
        elevated: true,
        restarter: Some(Box::new(FakeRestarter(task.clone()))),
        hello_available: true,
    })
    .expect("core starts");
    (core, telegram)
}

/// The password every test harness is set up with.
pub const TEST_PASSWORD: &str = "1406";

/// A harness on its first run: no password exists yet.
pub fn unconfigured_harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("hosts"), "127.0.0.1 localhost\r\n").unwrap();
    start(dir)
}

/// A harness whose owner already chose `TEST_PASSWORD`.
pub fn harness() -> Harness {
    harness_with(|_| ())
}

/// Like `harness`, with a chance to put files in the data dir before the core reads them.
pub fn harness_with(prepare: impl FnOnce(&std::path::Path)) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("hosts"), "127.0.0.1 localhost\r\n").unwrap();
    fs::create_dir_all(dir.path().join("data")).unwrap();
    let config = Config { password_hash: Some(auth::hash(TEST_PASSWORD).unwrap()), ..Config::fresh().unwrap() };
    write_json(&dir.path().join("data").join("config.json"), &config).unwrap();
    prepare(&dir.path().join("data"));
    start(dir)
}

fn start(dir: tempfile::TempDir) -> Harness {
    let (mono, wall) = (Arc::new(AtomicU64::new(100)), Arc::new(AtomicI64::new(START_WALL)));
    let (running, flaky, task) = (Arc::new(Mutex::new(Vec::new())), Arc::new(AtomicBool::new(false)), Arc::new(FakeTask::default()));
    // Stands in for the outbox thread: hands alerts over in order and answers flushes, like the real one.
    let (tx, queue) = mpsc::channel::<Msg>();
    let (handed, jobs) = mpsc::channel::<Job>();
    std::thread::spawn(move || {
        for msg in queue {
            match msg {
                Msg::Alert(job) => drop(handed.send(job)),
                Msg::Flush(done) => drop(done.send(())),
            }
        }
    });
    let (core, hub) = build(&dir, &mono, &wall, &running, &flaky, &task, &tx);
    Harness { core, dir, mono, wall, running, flaky, task, hub, jobs, tx }
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

    /// Pairs one chat the way a person would: open the window, send the code.
    pub fn pair_chat(&self, chat: i64) {
        let now = self.mono.load(Ordering::SeqCst);
        self.hub.connect("1:token", "porteiro_bot").unwrap();
        self.hub.start_pairing(now).unwrap();
        let code = self.hub.view(now).pairing.unwrap().code;
        self.hub.try_pair(chat, "Ana", &code, now).unwrap();
    }

    /// Alerts queued for delivery so far, once the evidence worker has finished its backlog.
    pub fn alerts(&self) -> Vec<Job> {
        self.core.flush_evidence();
        self.jobs.try_iter().collect()
    }

    pub fn kinds(&self) -> Vec<Kind> {
        self.events().into_iter().map(|e| e.kind).collect()
    }

    /// Simulates quitting and relaunching the app on the same disk, with the clocks as they are now.
    pub fn restart(self) -> Harness {
        self.core.flush_evidence();
        let Harness { core, dir, mono, wall, running, flaky, task, jobs, tx, .. } = self;
        drop(core);
        let (core, hub) = build(&dir, &mono, &wall, &running, &flaky, &task, &tx);
        Harness { core, dir, mono, wall, running, flaky, task, hub, jobs, tx }
    }
}
