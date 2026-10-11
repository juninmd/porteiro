//! Scheduled task that starts Porteiro at logon and every minute afterwards, elevated and without a UAC prompt.
//! If the process is killed it is back within a minute and re-applies the block from disk state.
use std::{fs, path::Path, process::Command};

pub const TASK_NAME: &str = "Porteiro";

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// `run_level` is `HighestAvailable` in production and `LeastPrivilege` in the tests that register a real task.
pub fn task_xml(exe: &str, user: &str, run_level: &str) -> String {
    let (exe, user) = (xml_escape(exe), xml_escape(user));
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>Porteiro: keeps the site block running</Description></RegistrationInfo>
  <Triggers>
    <LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger>
    <TimeTrigger>
      <StartBoundary>2020-01-01T00:00:00</StartBoundary>
      <Enabled>true</Enabled>
      <Repetition><Interval>PT1M</Interval></Repetition>
    </TimeTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>{run_level}</RunLevel></Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>false</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Enabled>true</Enabled>
  </Settings>
  <Actions Context="Author"><Exec><Command>{exe}</Command><Arguments>--watchdog</Arguments></Exec></Actions>
</Task>
"#
    )
}

/// schtasks only accepts UTF-16 LE with a BOM for the XML it imports.
fn utf16_le_bom(s: &str) -> Vec<u8> {
    std::iter::once(0xFEFFu16).chain(s.encode_utf16()).flat_map(u16::to_le_bytes).collect()
}

fn user() -> String {
    let name = std::env::var("USERNAME").unwrap_or_default();
    match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => format!("{domain}\\{name}"),
        _ => name,
    }
}

fn run(args: &[&str]) -> Result<String, String> {
    let out = Command::new("schtasks").args(args).output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() {
        Ok(text)
    } else {
        Err(format!("{} {}", text, String::from_utf8_lossy(&out.stderr).trim()).trim().to_string())
    }
}

pub fn install_named(name: &str, exe: &Path, run_level: &str, work_dir: &Path) -> Result<(), String> {
    let xml = work_dir.join(format!("{name}.task.xml"));
    fs::write(&xml, utf16_le_bom(&task_xml(&exe.to_string_lossy(), &user(), run_level))).map_err(|e| e.to_string())?;
    let result = run(&["/Create", "/TN", name, "/XML", &xml.to_string_lossy(), "/F"]).map(|_| ());
    let _ = fs::remove_file(&xml);
    result
}

pub fn install(exe: &Path, work_dir: &Path) -> Result<(), String> {
    install_named(TASK_NAME, exe, "HighestAvailable", work_dir)
}

pub fn remove_named(name: &str) -> Result<(), String> {
    if !exists_named(name) {
        return Ok(());
    }
    run(&["/Delete", "/TN", name, "/F"]).map(|_| ())
}

pub fn remove() -> Result<(), String> {
    remove_named(TASK_NAME)
}

pub fn exists_named(name: &str) -> bool {
    run(&["/Query", "/TN", name]).is_ok()
}

pub fn exists() -> bool {
    exists_named(TASK_NAME)
}

/// What the core needs from the restart task; faked in tests so the quit path can be exercised.
pub trait Restarter: Send {
    fn install(&mut self, work_dir: &Path) -> Result<(), String>;
    fn remove(&mut self) -> Result<(), String>;
}

pub struct TaskRestarter;

impl Restarter for TaskRestarter {
    fn install(&mut self, work_dir: &Path) -> Result<(), String> {
        install(&std::env::current_exe().map_err(|e| e.to_string())?, work_dir)
    }
    fn remove(&mut self) -> Result<(), String> {
        remove()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_runs_elevated_without_overlap_and_flags_the_watchdog_launch() {
        let xml = task_xml("C:\\Program Files\\Porteiro\\porteiro.exe", "PC\\jr", "HighestAvailable");
        for needle in [
            "<RunLevel>HighestAvailable</RunLevel>", "<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>",
            "<Interval>PT1M</Interval>", "<Arguments>--watchdog</Arguments>", "<LogonTrigger>", "<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>",
            "<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>",
        ] {
            assert!(xml.contains(needle), "missing {needle}");
        }
    }

    #[test]
    fn paths_and_users_cannot_break_out_of_the_xml() {
        let xml = task_xml("C:\\A&B <x>\\p.exe", "PC\\o'brien\"", "HighestAvailable");
        assert!(xml.contains("C:\\A&amp;B &lt;x&gt;\\p.exe") && xml.contains("PC\\o&apos;brien&quot;"));
        assert!(!xml.contains("<x>"));
    }

    #[test]
    fn utf16_has_a_bom_and_round_trips() {
        let bytes = utf16_le_bom("<a>é</a>");
        assert_eq!(&bytes[..2], [0xFF, 0xFE]);
        let units: Vec<u16> = bytes[2..].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(String::from_utf16(&units).unwrap(), "<a>é</a>");
    }

    /// Registers a real, harmless task for the current user (no admin needed at LeastPrivilege) to prove schtasks accepts our XML.
    #[cfg(windows)]
    #[test]
    fn schtasks_accepts_the_generated_xml() {
        let name = format!("PorteiroTest-{}", std::process::id());
        let dir = tempfile::tempdir().unwrap();
        let exe = Path::new("C:\\Windows\\System32\\cmd.exe");
        install_named(&name, exe, "LeastPrivilege", dir.path()).expect("schtasks rejected the XML");
        let registered = exists_named(&name);
        let _ = remove_named(&name);
        assert!(registered && !exists_named(&name));
        assert!(remove_named(&name).is_ok(), "removing a missing task is not an error");
    }
}
