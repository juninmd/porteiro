//! Lets the poller reach the running `Core` without knowing it.
use super::{format, poller::Remote};
use crate::{commands::{lock, Shared}, events::Kind};

impl Remote for Shared {
    fn now(&self) -> u64 {
        lock(self).mono()
    }

    fn status_text(&self) -> String {
        format::status_text(&lock(self).status())
    }

    fn relock(&self) -> bool {
        lock(self).relock_via("pelo Telegram")
    }

    fn record(&self, kind: Kind, detail: String) {
        lock(self).log(kind, detail, false);
    }
}
