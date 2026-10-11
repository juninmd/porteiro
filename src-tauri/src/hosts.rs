//! The managed block inside the hosts file. Everything outside the markers is preserved byte for byte.
use std::{fs, io, path::Path, thread, time::Duration};

pub const BEGIN: &str = "# >>> porteiro >>>";
pub const END: &str = "# <<< porteiro <<<";

fn is_our_entry(line: &str) -> bool {
    let mut parts = line.split_whitespace();
    matches!((parts.next(), parts.next(), parts.next()), (Some("0.0.0.0" | "::"), Some(_), None))
}

/// Removes every managed block. An unterminated block only loses its marker and the entry lines right
/// after it, so a user line that follows a damaged block is never swallowed.
fn strip_block(lines: Vec<&str>) -> Vec<&str> {
    let mut out = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        if line == BEGIN {
            i += 1;
            match lines[i..].iter().position(|l| l.trim() == END) {
                Some(end) => i += end + 1,
                None => {
                    while i < lines.len() && is_our_entry(lines[i].trim()) {
                        i += 1;
                    }
                }
            }
            continue;
        }
        if line != END {
            out.push(lines[i]);
        }
        i += 1;
    }
    out
}

/// Returns `original` with the managed block applied (`locked`) or removed. Idempotent.
pub fn render(original: &str, hosts: &[String], locked: bool) -> String {
    let eol = if original.contains('\n') && !original.contains("\r\n") { "\n" } else { "\r\n" };
    let lines: Vec<&str> = original.lines().collect();
    let mut kept: Vec<String> = strip_block(lines).into_iter().map(str::to_string).collect();
    while kept.last().is_some_and(|l| l.trim().is_empty()) {
        kept.pop();
    }
    if locked && !hosts.is_empty() {
        kept.push(BEGIN.to_string());
        for h in hosts {
            kept.push(format!("0.0.0.0 {h}"));
            kept.push(format!(":: {h}"));
        }
        kept.push(END.to_string());
    }
    if kept.is_empty() {
        return String::new();
    }
    kept.join(eol) + eol
}

pub fn in_state(content: &str, hosts: &[String], locked: bool) -> bool {
    let trimmed = |s: &str| s.lines().map(str::trim_end).collect::<Vec<_>>().join("\n").trim_end().to_string();
    trimmed(content) == trimmed(&render(content, hosts, locked))
}

fn write_with_retry(path: &Path, data: &[u8]) -> io::Result<()> {
    let mut last = None;
    for _ in 0..4 {
        match fs::write(path, data) {
            Ok(()) => return Ok(()),
            Err(e) => last = Some(e),
        }
        // Antivirus and the DNS client briefly hold the file open.
        thread::sleep(Duration::from_millis(150));
    }
    Err(last.expect("loop ran at least once"))
}

/// Brings the file to the desired state. Returns whether it had to change.
pub fn apply(path: &Path, hosts: &[String], locked: bool) -> io::Result<bool> {
    let raw = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e),
    };
    // Refuse to rewrite a file we cannot represent exactly: lossy conversion would corrupt the user's lines.
    let text = String::from_utf8(raw).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "hosts is not valid UTF-8"))?;
    let (bom, body) = match text.strip_prefix('\u{feff}') {
        Some(rest) => ("\u{feff}", rest),
        None => ("", text.as_str()),
    };
    if in_state(body, hosts, locked) {
        return Ok(false);
    }
    write_with_retry(path, format!("{bom}{}", render(body, hosts, locked)).as_bytes())?;
    Ok(true)
}

pub fn is_in_state(path: &Path, hosts: &[String], locked: bool) -> io::Result<bool> {
    let text = match fs::read(path) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    Ok(in_state(text.trim_start_matches('\u{feff}'), hosts, locked))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosts() -> Vec<String> {
        vec!["instagram.com".into(), "www.instagram.com".into()]
    }

    const BASE: &str = "# Copyright\r\n127.0.0.1 localhost\r\n::1 localhost\r\n";

    #[test]
    fn locking_appends_a_block_and_keeps_user_lines() {
        let out = render(BASE, &hosts(), true);
        assert!(out.starts_with(BASE));
        assert!(out.contains("0.0.0.0 www.instagram.com\r\n:: www.instagram.com\r\n"));
        assert!(out.ends_with(&format!("{END}\r\n")));
    }

    #[test]
    fn render_is_idempotent_and_unlock_restores_the_original() {
        let locked = render(BASE, &hosts(), true);
        assert_eq!(render(&locked, &hosts(), true), locked);
        assert_eq!(render(&locked, &hosts(), false), BASE);
    }

    #[test]
    fn the_domain_list_is_replaced_not_accumulated() {
        let locked = render(BASE, &hosts(), true);
        let again = render(&locked, &["only.com".to_string()], true);
        assert!(!again.contains("instagram") && again.contains("0.0.0.0 only.com"));
        assert_eq!(again.matches(BEGIN).count(), 1);
    }

    #[test]
    fn line_endings_of_the_original_are_kept() {
        assert!(!render("127.0.0.1 localhost\n", &hosts(), true).contains('\r'));
        assert!(render("", &hosts(), true).contains("\r\n"));
    }

    #[test]
    fn a_damaged_block_never_swallows_user_lines() {
        let damaged = format!("127.0.0.1 localhost\n{BEGIN}\n0.0.0.0 old.com\n10.0.0.5 mine.lan\n");
        let out = render(&damaged, &hosts(), false);
        assert_eq!(out, "127.0.0.1 localhost\n10.0.0.5 mine.lan\n");
        assert!(render(&format!("{END}\na b\n"), &[], false).starts_with("a b"));
    }

    #[test]
    fn drift_detection_sees_a_removed_or_edited_block() {
        let locked = render(BASE, &hosts(), true);
        assert!(in_state(&locked, &hosts(), true));
        assert!(!in_state(BASE, &hosts(), true), "block removed");
        assert!(!in_state(&locked.replace("0.0.0.0 instagram.com", "# 0.0.0.0 instagram.com"), &hosts(), true), "entry commented out");
        assert!(!in_state(&locked, &hosts(), false), "locked file while unlocked");
    }

    #[test]
    fn apply_writes_once_preserves_bom_and_refuses_binary_hosts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts");
        fs::write(&path, format!("\u{feff}{BASE}")).unwrap();
        assert!(apply(&path, &hosts(), true).unwrap());
        assert!(!apply(&path, &hosts(), true).unwrap(), "second apply is a no-op");
        assert!(fs::read_to_string(&path).unwrap().starts_with('\u{feff}'));
        assert!(is_in_state(&path, &hosts(), true).unwrap());
        fs::write(&path, [0x80, 0xff, b'\n']).unwrap();
        assert_eq!(apply(&path, &hosts(), true).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(fs::read(&path).unwrap(), [0x80, 0xff, b'\n'], "file left untouched");
    }
}
