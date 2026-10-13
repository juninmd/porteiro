//! Types that cross the IPC boundary, plus the clock abstraction.
use serde::Serialize;
use std::time::Instant;

pub trait Clock: Send {
    /// Unix seconds; used for timestamps and for the window that must survive a restart.
    fn wall(&self) -> i64;
    /// Unix milliseconds; event timestamps use it so two events in the same second keep their real order.
    fn wall_ms(&self) -> i64 {
        self.wall() * 1000
    }
    /// Seconds since start; immune to clock changes, used for every in-process deadline.
    fn mono(&self) -> u64;
}

pub struct SystemClock(Instant);

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemClock {
    pub fn new() -> Self {
        SystemClock(Instant::now())
    }
}

impl Clock for SystemClock {
    fn wall(&self) -> i64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
    }
    fn wall_ms(&self) -> i64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
    }
    fn mono(&self) -> u64 {
        self.0.elapsed().as_secs()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiError {
    pub code: &'static str,
    pub message: String,
    pub retry_after: Option<u64>,
    pub attempts_left: Option<u32>,
}

impl ApiError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        ApiError { code, message: message.into(), retry_after: None, attempts_left: None }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LayerStatus {
    pub name: String,
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ServiceInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub enabled: bool,
    pub domains: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct ScheduleInfo {
    pub enabled: bool,
    pub start_hour: u8,
    pub end_hour: u8,
    pub days: Vec<u8>,
}

#[derive(Debug, Serialize)]
pub struct Status {
    pub locked: bool,
    pub unlock_remaining_secs: Option<u64>,
    pub lockout_secs: Option<u64>,
    pub elevated: bool,
    /// No password exists yet (first run, or the config was lost): the app is locked until one is set.
    pub needs_setup: bool,
    pub hello_available: bool,
    pub hello_enabled: bool,
    pub layers: Vec<LayerStatus>,
    pub services: Vec<ServiceInfo>,
    pub custom_domains: Vec<String>,
    pub custom_apps: Vec<String>,
    pub unlock_minutes: u32,
    pub schedule: Option<ScheduleInfo>,
    pub watchdog: Option<bool>,
    pub event_count: usize,
    pub data_dir: String,
}
