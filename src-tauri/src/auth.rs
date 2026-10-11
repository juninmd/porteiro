//! Password hashing and brute-force throttling. Time is a monotonic seconds counter passed in, so a
//! clock change cannot shorten a lockout.
use argon2::{
    password_hash::{phc::PasswordHash, PasswordHasher, PasswordVerifier},
    Argon2,
};

pub const FREE_ATTEMPTS: u32 = 5;
const BASE_LOCKOUT_SECS: u64 = 30;
const MAX_LOCKOUT_SECS: u64 = 3600;

pub fn hash(password: &str) -> Result<String, String> {
    Argon2::default().hash_password(password.as_bytes()).map(|h| h.to_string()).map_err(|e| e.to_string())
}

pub fn verify(phc: &str, password: &str) -> bool {
    PasswordHash::new(phc).is_ok_and(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
}

pub fn validate_new_password(password: &str) -> Result<(), &'static str> {
    match password.chars().count() {
        0..=3 => Err("password_too_short"),
        4..=128 => Ok(()),
        _ => Err("password_too_long"),
    }
}

/// Lockout after `failures` consecutive misses: free until FREE_ATTEMPTS, then 30 s doubling up to 1 h.
pub fn lockout_secs(failures: u32) -> u64 {
    if failures < FREE_ATTEMPTS {
        return 0;
    }
    BASE_LOCKOUT_SECS.saturating_mul(1u64 << (failures - FREE_ATTEMPTS).min(20)).min(MAX_LOCKOUT_SECS)
}

#[derive(Debug)]
pub struct Throttle {
    failures: u32,
    until: Option<u64>,
}

impl Throttle {
    /// Restoring a persisted failure count re-imposes the lockout, so restarting the app is no escape.
    pub fn restore(failures: u32, now: u64) -> Self {
        let wait = lockout_secs(failures);
        Throttle { failures, until: (wait > 0).then(|| now + wait) }
    }

    /// Ok to try, or the seconds left in the lockout.
    pub fn check(&self, now: u64) -> Result<(), u64> {
        match self.until {
            Some(until) if now < until => Err(until - now),
            _ => Ok(()),
        }
    }

    /// Registers a miss; returns the new lockout length when this miss started one.
    pub fn fail(&mut self, now: u64) -> Option<u64> {
        self.failures = self.failures.saturating_add(1);
        let wait = lockout_secs(self.failures);
        self.until = (wait > 0).then(|| now + wait);
        (wait > 0).then_some(wait)
    }

    pub fn succeed(&mut self) {
        *self = Throttle { failures: 0, until: None };
    }

    pub fn failures(&self) -> u32 {
        self.failures
    }

    pub fn attempts_left(&self) -> u32 {
        FREE_ATTEMPTS.saturating_sub(self.failures)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_round_trips_and_rejects_wrong_or_garbage() {
        let phc = hash("1406").unwrap();
        assert!(phc.starts_with("$argon2id$"));
        assert!(verify(&phc, "1406"));
        assert!(!verify(&phc, "1407") && !verify(&phc, "") && !verify("not-a-hash", "1406"));
        assert_ne!(phc, hash("1406").unwrap(), "salted");
    }

    #[test]
    fn password_length_bounds() {
        assert_eq!(validate_new_password("123"), Err("password_too_short"));
        assert!(validate_new_password("1234").is_ok());
        assert!(validate_new_password("ação1").is_ok(), "counted in characters, not bytes");
        assert_eq!(validate_new_password(&"x".repeat(129)), Err("password_too_long"));
    }

    #[test]
    fn lockout_schedule_doubles_and_caps() {
        let secs: Vec<u64> = (0..=12).map(lockout_secs).collect();
        assert_eq!(&secs[..8], [0, 0, 0, 0, 0, 30, 60, 120]);
        assert_eq!(secs[12], 3600);
        assert_eq!(lockout_secs(u32::MAX), 3600);
    }

    #[test]
    fn five_misses_lock_out_until_the_deadline_and_success_resets() {
        let mut t = Throttle::restore(0, 100);
        for _ in 0..4 {
            assert_eq!(t.fail(100), None);
            assert!(t.check(100).is_ok());
        }
        assert_eq!(t.attempts_left(), 1);
        assert_eq!(t.fail(100), Some(30));
        assert_eq!(t.check(110), Err(20));
        assert!(t.check(130).is_ok());
        t.succeed();
        assert_eq!((t.failures(), t.attempts_left()), (0, 5));
    }

    #[test]
    fn restarting_with_persisted_failures_keeps_the_lockout() {
        assert_eq!(Throttle::restore(6, 1000).check(1000), Err(60));
        assert!(Throttle::restore(2, 1000).check(1000).is_ok());
    }
}
