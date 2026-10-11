//! Writes a `policy::Plan` to the registry. The root key is injected so tests run against HKCU.
use crate::policy::{Data, Plan};
use std::io;
use winreg::{enums::KEY_READ, RegKey};

fn list_matches(root: &RegKey, key: &str, entries: &[String]) -> bool {
    let Ok(k) = root.open_subkey_with_flags(key, KEY_READ) else { return entries.is_empty() };
    let found: Vec<String> = (1..=entries.len() + 1).map_while(|i| k.get_value::<String, _>(i.to_string()).ok()).collect();
    found == entries
}

fn value_matches(root: &RegKey, key: &str, name: &str, want: &Data) -> bool {
    let Ok(k) = root.open_subkey_with_flags(key, KEY_READ) else { return false };
    match want {
        Data::Str(s) => k.get_value::<String, _>(name).is_ok_and(|v| &v == s),
        Data::Dword(n) => k.get_value::<u32, _>(name).is_ok_and(|v| v == *n),
    }
}

pub fn apply(root: &RegKey, plan: &Plan) -> io::Result<()> {
    for (key, entries) in &plan.lists {
        // Recreate so entries removed from the list do not linger as stale numbered values.
        if let Err(e) = root.delete_subkey_all(key) {
            if e.kind() != io::ErrorKind::NotFound {
                return Err(e);
            }
        }
        let (k, _) = root.create_subkey(key)?;
        for (i, entry) in entries.iter().enumerate() {
            k.set_value((i + 1).to_string(), entry)?;
        }
    }
    for (key, name, data) in &plan.values {
        let (k, _) = root.create_subkey(key)?;
        match data {
            Data::Str(s) => k.set_value(name, s)?,
            Data::Dword(n) => k.set_value(name, n)?,
        }
    }
    Ok(())
}

pub fn clear(root: &RegKey, plan: &Plan) -> io::Result<()> {
    let ignore_missing = |r: io::Result<()>| match r {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    };
    for (key, _) in &plan.lists {
        ignore_missing(root.delete_subkey_all(key))?;
    }
    for (key, name, _) in &plan.values {
        if let Ok(k) = root.open_subkey_with_flags(key, winreg::enums::KEY_SET_VALUE) {
            ignore_missing(k.delete_value(name))?;
        }
    }
    Ok(())
}

pub fn is_applied(root: &RegKey, plan: &Plan) -> bool {
    plan.lists.iter().all(|(k, e)| list_matches(root, k, e))
        && plan.values.iter().all(|(k, n, d)| value_matches(root, k, n, d))
}

pub fn is_cleared(root: &RegKey, plan: &Plan) -> bool {
    plan.lists.iter().all(|(k, _)| list_matches(root, k, &[]))
        && plan.values.iter().all(|(k, n, _)| {
            root.open_subkey_with_flags(k, KEY_READ).map_or(true, |key| key.get_raw_value(n).is_err())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::plan;
    use winreg::enums::HKEY_CURRENT_USER;

    /// Registry sandbox under HKCU that removes itself, so tests never touch real policies.
    struct Sandbox(RegKey, String);
    impl Sandbox {
        fn new() -> Self {
            let name = format!("Software\\PorteiroTest\\{}", uuid::Uuid::new_v4());
            let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(&name).unwrap();
            Sandbox(key, name)
        }
    }
    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&self.1);
        }
    }

    #[test]
    fn apply_then_clear_round_trips_and_detects_tampering() {
        let sb = Sandbox::new();
        let p = plan(&["instagram.com".to_string(), "x.com".to_string()]);
        assert!(is_cleared(&sb.0, &p));
        apply(&sb.0, &p).unwrap();
        assert!(is_applied(&sb.0, &p) && !is_cleared(&sb.0, &p));

        let chrome = sb.0.open_subkey_with_flags("SOFTWARE\\Policies\\Google\\Chrome\\URLBlocklist", winreg::enums::KEY_SET_VALUE).unwrap();
        chrome.delete_value("2").unwrap();
        assert!(!is_applied(&sb.0, &p), "dropped list entry is drift");

        apply(&sb.0, &p).unwrap();
        let smaller = plan(&["instagram.com".to_string()]);
        apply(&sb.0, &smaller).unwrap();
        assert!(is_applied(&sb.0, &smaller), "shrinking the list leaves no stale value");

        clear(&sb.0, &p).unwrap();
        assert!(is_cleared(&sb.0, &p));
        clear(&sb.0, &p).unwrap();
    }
}
