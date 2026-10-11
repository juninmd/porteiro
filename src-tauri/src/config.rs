//! Persisted settings and runtime state. Both live in the data dir, which is locked to SYSTEM and Administrators.
use crate::{auth, blocklist};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
};

pub const DEFAULT_PASSWORD: &str = "1406";
pub const MAX_UNLOCK_MINUTES: u32 = 240;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub password_hash: String,
    /// True until the user changes the password; the UI nags while it is.
    pub default_password: bool,
    pub enabled_services: Vec<String>,
    pub custom_domains: Vec<String>,
    pub unlock_minutes: u32,
}

impl Config {
    pub fn fresh() -> Result<Config, String> {
        Ok(Config {
            password_hash: auth::hash(DEFAULT_PASSWORD)?,
            default_password: true,
            enabled_services: blocklist::default_enabled(),
            custom_domains: Vec::new(),
            unlock_minutes: 15,
        })
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct State {
    /// Wall-clock deadline of the current unlock window; only honoured within MAX_UNLOCK_MINUTES.
    pub unlocked_until: Option<i64>,
    pub failures: u32,
    /// Latest wall-clock reading taken while a window was open; a restart that finds the clock behind it
    /// knows the clock was moved back.
    #[serde(default)]
    pub seen_wall: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct Paths {
    pub data: PathBuf,
    pub hosts: PathBuf,
    /// True when `hosts` was redirected (dev/test), so no administrator rights are needed to write it.
    pub hosts_overridden: bool,
    /// Any override in play: the app then touches nothing outside its sandbox (no scheduled task).
    pub sandboxed: bool,
}

impl Paths {
    /// `PORTEIRO_DATA_DIR` and `PORTEIRO_HOSTS_FILE` exist for tests and dev runs; production uses ProgramData.
    /// Release builds ignore them: the process runs elevated, and an environment variable set by an unelevated
    /// process must not be able to point it at a fresh config with the default password.
    pub fn resolve() -> Paths {
        Paths::from_env(|k| std::env::var_os(k), cfg!(debug_assertions))
    }

    pub fn from_env(get: impl Fn(&str) -> Option<OsString>, allow_overrides: bool) -> Paths {
        let pick = |k: &str| get(k).filter(|_| allow_overrides).map(PathBuf::from);
        let data_override = pick("PORTEIRO_DATA_DIR");
        let data_overridden = data_override.is_some();
        let data = data_override.unwrap_or_else(|| PathBuf::from(get("ProgramData").unwrap_or_else(|| "C:\\ProgramData".into())).join("Porteiro"));
        let override_hosts = pick("PORTEIRO_HOSTS_FILE");
        let hosts_overridden = override_hosts.is_some();
        let hosts = override_hosts.unwrap_or_else(|| {
            PathBuf::from(get("SystemRoot").unwrap_or_else(|| "C:\\Windows".into())).join("System32\\drivers\\etc\\hosts")
        });
        Paths { data, hosts, hosts_overridden, sandboxed: data_overridden || hosts_overridden }
    }

    pub fn config(&self) -> PathBuf {
        self.data.join("config.json")
    }
    pub fn state(&self) -> PathBuf {
        self.data.join("state.json")
    }
    pub fn events(&self) -> PathBuf {
        self.data.join("events.jsonl")
    }
    pub fn shots(&self) -> PathBuf {
        self.data.join("shots")
    }
    pub fn ips(&self) -> PathBuf {
        self.data.join("ips.json")
    }
}

/// Atomic replace so a crash never leaves half a JSON file; synced first so a power cut cannot leave it empty
/// (an empty config is read as damaged and falls back to the default password).
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut file = fs::File::create(&tmp)?;
    file.write_all(&serde_json::to_vec_pretty(value).map_err(io::Error::other)?)?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp, path)
}

pub enum Loaded<T> {
    Fresh(T),
    Read(T),
    /// The file was unreadable; it was moved aside and defaults were written.
    Reset(T),
}

impl<T> Loaded<T> {
    pub fn into_inner(self) -> T {
        match self {
            Loaded::Fresh(v) | Loaded::Read(v) | Loaded::Reset(v) => v,
        }
    }
}

pub fn load_or_create<T>(path: &Path, make: impl FnOnce() -> Result<T, String>) -> Result<Loaded<T>, String>
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(v) => Ok(Loaded::Read(v)),
            Err(_) => {
                let aside = path.with_extension(format!("corrupt-{}", std::process::id()));
                fs::rename(path, aside).map_err(|e| e.to_string())?;
                let v = make()?;
                write_json(path, &v).map_err(|e| e.to_string())?;
                Ok(Loaded::Reset(v))
            }
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let v = make()?;
            write_json(path, &v).map_err(|e| e.to_string())?;
            Ok(Loaded::Fresh(v))
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Creates the data dirs and, when running elevated for real, restricts them to SYSTEM and Administrators.
/// Sandboxed runs and non-elevated debug runs skip the ACL change: an unelevated administrator token has the
/// Administrators group as deny-only, so locking the folder to it would lock the developer out.
pub fn prepare_data_dir(paths: &Paths, elevated: bool) -> Result<(), String> {
    fs::create_dir_all(paths.shots()).map_err(|e| e.to_string())?;
    if paths.sandboxed || !elevated {
        return Ok(());
    }
    lock_down(&paths.data)
}

/// Uses SIDs so it works in any Windows language. Best effort: the error is surfaced, not fatal.
fn lock_down(dir: &Path) -> Result<(), String> {
    let out = Command::new("icacls")
        .arg(dir)
        .args(["/inheritance:r", "/grant:r", "*S-1-5-18:(OI)(CI)F", "/grant:r", "*S-1-5-32-544:(OI)(CI)F", "/q"])
        .output()
        .map_err(|e| e.to_string())?;
    out.status.success().then_some(()).ok_or_else(|| String::from_utf8_lossy(&out.stderr).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_config_uses_the_default_password_hashed_and_all_services() {
        let c = Config::fresh().unwrap();
        assert!(c.default_password && auth::verify(&c.password_hash, DEFAULT_PASSWORD));
        assert!(!c.password_hash.contains(DEFAULT_PASSWORD));
        assert_eq!(c.enabled_services, blocklist::default_enabled());
    }

    #[test]
    fn load_creates_reads_and_recovers_from_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        assert!(matches!(load_or_create(&path, || Ok(State::default())), Ok(Loaded::Fresh(_))));
        write_json(&path, &State { unlocked_until: Some(5), failures: 2, ..State::default() }).unwrap();
        assert!(matches!(load_or_create(&path, || Ok(State::default())), Ok(Loaded::Read(s)) if s.failures == 2));
        fs::write(&path, b"{not json").unwrap();
        assert!(matches!(load_or_create(&path, || Ok(State::default())), Ok(Loaded::Reset(_))));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2, "the damaged file is kept aside, not deleted");
    }

    #[test]
    fn release_builds_ignore_the_sandbox_variables() {
        let env = |k: &str| match k {
            "PORTEIRO_DATA_DIR" => Some("D:\\attacker\\data".into()),
            "PORTEIRO_HOSTS_FILE" => Some("D:\\attacker\\hosts".into()),
            "ProgramData" => Some("C:\\ProgramData".into()),
            "SystemRoot" => Some("C:\\Windows".into()),
            _ => None,
        };
        let release = Paths::from_env(env, false);
        assert!(!release.sandboxed && !release.hosts_overridden);
        assert_eq!(release.data, PathBuf::from("C:\\ProgramData\\Porteiro"));
        assert!(release.hosts.ends_with("drivers\\etc\\hosts"));
        let debug = Paths::from_env(env, true);
        assert!(debug.sandboxed && debug.hosts_overridden && debug.data.ends_with("data") && debug.data.starts_with("D:\\attacker"));
    }

    #[test]
    fn writes_are_atomic_replacements() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.json");
        write_json(&path, &State::default()).unwrap();
        write_json(&path, &State { unlocked_until: Some(1), ..State::default() }).unwrap();
        assert!(!path.with_extension("tmp").exists());
    }
}
