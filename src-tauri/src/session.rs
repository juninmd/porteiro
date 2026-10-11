//! Windows lock/unlock detection by polling the console session flags; no window procedure hook needed.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Locked,
    Unlocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    Locked,
    Unlocked,
}

/// Only real changes produce an event; the first observation just seeds the state.
pub fn transition(prev: Option<SessionState>, now: SessionState) -> Option<Transition> {
    match (prev?, now) {
        (SessionState::Unlocked, SessionState::Locked) => Some(Transition::Locked),
        (SessionState::Locked, SessionState::Unlocked) => Some(Transition::Unlocked),
        _ => None,
    }
}

/// `None` when the state cannot be read (no interactive session, API failure): never guess "unlocked".
#[cfg(windows)]
pub fn poll() -> Option<SessionState> {
    use windows_sys::Win32::System::RemoteDesktop::{
        WTSFreeMemory, WTSQuerySessionInformationW, WTSINFOEXW, WTSSessionInfoEx, WTS_CURRENT_SERVER_HANDLE,
        WTS_CURRENT_SESSION, WTS_SESSIONSTATE_LOCK, WTS_SESSIONSTATE_UNLOCK,
    };
    let mut buf: *mut u16 = std::ptr::null_mut();
    let mut len = 0u32;
    // SAFETY: WTS allocates `buf`; it is read only after a success return and always freed.
    unsafe {
        if WTSQuerySessionInformationW(WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, WTSSessionInfoEx, &mut buf, &mut len) == 0 {
            return None;
        }
        let state = if buf.is_null() || (len as usize) < std::mem::size_of::<WTSINFOEXW>() {
            None
        } else {
            let info = &*(buf as *const WTSINFOEXW);
            match info.Data.WTSInfoExLevel1.SessionFlags as u32 {
                f if f == WTS_SESSIONSTATE_LOCK => Some(SessionState::Locked),
                f if f == WTS_SESSIONSTATE_UNLOCK => Some(SessionState::Unlocked),
                _ => None,
            }
        };
        if !buf.is_null() {
            WTSFreeMemory(buf as *mut _);
        }
        state
    }
}

#[cfg(not(windows))]
pub fn poll() -> Option<SessionState> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use SessionState::*;

    #[test]
    fn only_real_changes_emit_and_the_first_sample_is_silent() {
        assert_eq!(transition(None, Unlocked), None);
        assert_eq!(transition(Some(Unlocked), Locked), Some(Transition::Locked));
        assert_eq!(transition(Some(Locked), Unlocked), Some(Transition::Unlocked));
        assert_eq!(transition(Some(Locked), Locked), None);
        assert_eq!(transition(Some(Unlocked), Unlocked), None);
    }

    #[cfg(windows)]
    #[test]
    fn polling_the_real_session_does_not_crash() {
        // The test runs inside an interactive, unlocked session in practice; both answers are acceptable elsewhere.
        println!("session state: {:?}", poll());
    }
}
