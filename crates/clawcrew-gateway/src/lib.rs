#![allow(
    clippy::to_string_in_format_args,
    clippy::useless_format,
    clippy::collapsible_if
)]

pub mod state;
pub mod gateway;
pub mod rate_limit;

#[cfg(feature = "a2a")]
pub mod a2a;
pub mod acp;
pub mod agent_owned_state;
pub mod api;
pub mod api_backup;
pub mod api_browse;
pub mod api_config;
pub mod api_logs;
pub mod api_pairing;
pub mod api_personality;
#[cfg(feature = "plugins-wasm")]
pub mod api_plugins;
pub mod api_quickstart;
pub mod api_sections;
pub mod api_skills;
pub mod api_sop;
pub mod api_sop_author;
mod api_sop_webhook;
pub mod api_tasks;
pub mod api_upload;
#[cfg(feature = "webauthn")]
pub mod api_webauthn;
#[cfg(any(
    feature = "channel-linq",
    feature = "channel-nextcloud",
    feature = "channel-whatsapp-cloud"
))]
pub mod api_webhook;
pub mod auth_rate_limit;
pub mod canvas;
pub mod hardware_context;
pub mod node_tool;
pub mod nodes;
pub mod openapi;
#[cfg(feature = "plugins-wasm")]
mod plugin_webhook;
pub mod security_headers;
pub mod session_queue;
pub mod sse;
pub mod static_files;
pub mod tls;
pub mod version;
pub mod grpc_system_gateway;
#[cfg(feature = "gateway-voice-duplex")]
pub mod voice_duplex;
#[cfg(any(
    feature = "channel-linq",
    feature = "channel-nextcloud",
    feature = "channel-whatsapp-cloud"
))]
mod webhook_ingress;
pub mod api_audit;
pub mod api_providers;
pub mod ws;
pub mod ws_approval;
pub mod ws_sop_runs;
pub mod handlers;

// Re-export key types from extracted modules
pub use state::{AppState, GatewaySupervision};
pub use gateway::{run_gateway, run_gateway_with_plugin_webhooks, start_grpc_server};
pub use rate_limit::{SlidingWindowRateLimiter, GatewayRateLimiter, IdempotencyStore};

// Constants from rate_limit module
pub use rate_limit::{RATE_LIMIT_WINDOW_SECS, RATE_LIMIT_MAX_KEYS_DEFAULT, IDEMPOTENCY_MAX_KEYS_DEFAULT};

// Types from other modules that extracted files need
pub use clawcrew_config::schema::Config;
pub use clawcrew_runtime::agent::memory_strategy::MemoryStrategy;
pub use clawcrew_runtime::tools::CanvasStore;

// Helper types/functions that extracted files reference
pub type ConfigWriteGuard<'a> = std::sync::MutexGuard<'a, Config>;
pub struct AdminReloadGate;
pub fn admin_reload_gate() -> AdminReloadGate {
    AdminReloadGate
}
