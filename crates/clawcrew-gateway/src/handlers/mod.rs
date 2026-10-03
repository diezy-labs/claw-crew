//! Dynamic route handlers for plugin self-registration.
//!
//! Each handler implements the `RouteHandler` trait from `clawcrew-routing`.

pub mod config;
pub mod backup;
pub mod browse;
pub mod logs;
pub mod pairing;
pub mod personality;
#[cfg(feature = "plugins-wasm")]
pub mod plugins;
pub mod quickstart;
pub mod sections;
pub mod skills;
pub mod sop;
pub mod sop_author;
pub mod sop_webhook;
pub mod tasks;
pub mod upload;
#[cfg(feature = "webauthn")]
pub mod webauthn;
#[cfg(any(
    feature = "channel-linq",
    feature = "channel-nextcloud",
    feature = "channel-whatsapp-cloud"
))]
pub mod webhook;
pub mod rate_limit;
pub mod canvas;
pub mod hardware_context;
pub mod node_tool;
pub mod nodes;
pub mod openapi;
#[cfg(feature = "plugins-wasm")]
pub mod plugin_webhook;
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
pub mod webhook_ingress;
pub mod audit;
pub mod providers;
pub mod ws;
pub mod ws_approval;
pub mod ws_sop_runs;
