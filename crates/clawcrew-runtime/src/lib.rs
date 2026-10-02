#![allow(
    clippy::to_string_in_format_args,
    clippy::useless_format,
    clippy::manual_inspect
)]
//! Agent runtime — orchestration, security, observability, cron, SOP, skills, hardware, and more.

pub mod cli_input;
pub mod identity;
pub mod migration;
pub mod util;

pub mod agent;
pub mod approval;
pub mod browse;
pub mod calendar;
pub mod control_plane;
pub mod cost;
pub mod cron;
pub mod daemon;
pub mod doctor;
pub mod enroll;
pub mod health;
pub mod heartbeat;
pub mod hooks;
// RF-B: the i18n body now lives in the standalone `galleon-i18n` crate (so
// channels/gateway can localize without depending on this 288k-LOC runtime).
// Re-export it under the historical `i18n` path so every existing
// `clawcrew_runtime::i18n::*` caller keeps resolving unchanged.
pub use galleon_i18n as i18n;
pub mod integrations;
pub mod observability;
pub mod peers;
pub mod platform;
pub mod plugin_runtime;
pub mod process_stats;
pub mod quickstart;
pub mod rag;
pub mod relay;
pub mod restart;
pub mod routines;
pub mod rpc;
pub mod security;
pub mod service;
pub mod session;
pub mod skills;
pub mod sop;
pub mod subagent;
pub mod tools;
pub mod trust;
pub mod tunnel;
pub mod verifiable_intent;
