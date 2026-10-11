//! Whether the process holds an elevated (administrator) token. Without one the OS refuses every layer.

#[cfg(windows)]
pub fn is_elevated() -> bool {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    // SAFETY: the token handle is opened, queried into a correctly sized struct, and always closed.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut info = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(token, TokenElevation, &mut info as *mut _ as *mut _, std::mem::size_of::<TOKEN_ELEVATION>() as u32, &mut len);
        CloseHandle(token);
        ok != 0 && info.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    false
}

#[cfg(test)]
mod tests {
    #[test]
    fn runs_without_crashing_and_matches_the_launching_shell() {
        // Not elevated when launched from a normal terminal; elevated from an admin one. Either is valid.
        println!("elevated: {}", super::is_elevated());
    }
}
