#![allow(dead_code)]
pub mod admin;
pub mod api;
mod app;
pub mod auth;
pub mod blocklist;
pub mod capture;
mod commands;
pub mod config;
pub mod core;
mod core_events;
mod core_telegram;
pub mod elevation;
pub mod enforce;
pub mod events;
pub mod evidence;
pub mod firewall;
pub mod hello;
pub mod hosts;
pub mod layers;
pub mod policy;
#[cfg(windows)]
pub mod policy_reg;
pub mod session;
pub mod telegram;
pub mod watchdog;

#[cfg(test)]
mod core_telegram_tests;
#[cfg(test)]
mod core_tests;
#[cfg(test)]
mod test_support;

use std::sync::atomic::AtomicBool;

/// Set only by the password-checked "disable and quit" command; every other exit request is refused.
pub static QUIT_AUTHORIZED: AtomicBool = AtomicBool::new(false);

pub use app::run;
