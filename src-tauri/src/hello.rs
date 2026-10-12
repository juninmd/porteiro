//! Windows Hello as an unlock method: the OS asks for the face, fingerprint or PIN and says yes or no.
//! The prompt is modal and slow, so callers run it without holding the core lock.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Verified,
    /// The person closed the prompt; not an attempt and not logged as one.
    Cancelled,
    /// The OS rejected the person (wrong face, too many PIN tries).
    Failed(String),
    Unavailable(String),
}

pub trait Verifier: Send + Sync {
    fn available(&self) -> bool;
    /// `hwnd` is the app window the prompt attaches to.
    fn verify(&self, hwnd: isize, reason: &str) -> Verdict;
}

/// Used where Hello does not exist (other OS, tests that do not care).
pub struct NoHello;

impl Verifier for NoHello {
    fn available(&self) -> bool {
        false
    }
    fn verify(&self, _: isize, _: &str) -> Verdict {
        Verdict::Unavailable("Windows Hello não está disponível".into())
    }
}

#[cfg(windows)]
pub struct WindowsHello;

#[cfg(windows)]
mod win {
    use super::{Verdict, Verifier, WindowsHello};
    use windows_future::IAsyncOperation;
    use windows::{
        core::{factory, HSTRING},
        Security::Credentials::UI::{UserConsentVerificationResult as R, UserConsentVerifier, UserConsentVerifierAvailability as A},
        Win32::{Foundation::HWND, System::WinRT::IUserConsentVerifierInterop},
    };

    impl Verifier for WindowsHello {
        fn available(&self) -> bool {
            UserConsentVerifier::CheckAvailabilityAsync().and_then(|op| op.join()).is_ok_and(|a| a == A::Available)
        }

        fn verify(&self, hwnd: isize, reason: &str) -> Verdict {
            let run = || -> windows::core::Result<R> {
                let interop = factory::<UserConsentVerifier, IUserConsentVerifierInterop>()?;
                let op: IAsyncOperation<R> = unsafe { interop.RequestVerificationForWindowAsync(HWND(hwnd as *mut _), &HSTRING::from(reason))? };
                op.join()
            };
            match run() {
                Ok(R::Verified) => Verdict::Verified,
                Ok(R::Canceled) => Verdict::Cancelled,
                Ok(R::RetriesExhausted) => Verdict::Failed("tentativas esgotadas no Windows Hello".into()),
                Ok(R::DeviceBusy) => Verdict::Unavailable("o Windows Hello está ocupado".into()),
                Ok(_) => Verdict::Unavailable("o Windows Hello não está configurado neste PC".into()),
                Err(e) => Verdict::Unavailable(e.message()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_hello_nothing_verifies() {
        assert!(!NoHello.available());
        assert!(matches!(NoHello.verify(0, "x"), Verdict::Unavailable(_)));
    }

    /// Needs Windows Hello set up on the machine; only reports availability, never opens the prompt.
    #[cfg(windows)]
    #[test]
    fn availability_can_be_read_on_this_machine() {
        println!("windows hello available: {}", WindowsHello.available());
    }
}
