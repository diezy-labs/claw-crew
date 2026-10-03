use std::collections::HashMap;
use std::sync::Arc;

use clawcrew_config::schema::Config;
use clawcrew_infra::session_backend::SessionBackend;
use clawcrew_memory::{self, Memory, MemoryCategory};
use clawcrew_providers::{self, ModelProvider};
use clawcrew_runtime::agent::memory_strategy::DefaultMemoryStrategy;
use clawcrew_runtime::cost::CostTracker;
use clawcrew_runtime::security::pairing::PairingGuard;
use clawcrew_runtime::tools::CanvasStore;
use parking_lot::RwLock;

/// Shared state for all axum handlers
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<Config>>,

    /// Serializes the read-mutate-save-swap critical section of every HTTP
    /// handler that mutates `config`
    pub config_write_lock: Arc<tokio::sync::Mutex<()>>,
    pub model_provider: Arc<dyn ModelProvider>,
    pub model: String,
    pub temperature: Option<f64>,
    pub mem: Arc<dyn Memory>,
    pub memory_strategy: Arc<dyn MemoryStrategy>,
    pub auto_save: bool,
    pub pairing: Arc<PairingGuard>,
    pub trust_forwarded_headers: bool,
    pub rate_limiter: Arc<crate::GatewayRateLimiter>,
    pub auth_limiter: Arc<crate::auth_rate_limit::AuthRateLimiter>,
    pub idempotency_store: Arc<crate::IdempotencyStore>,
    #[cfg(feature = "channel-whatsapp-cloud")]
    pub whatsapp: HashMap<String, Arc<clawcrew_channels::whatsapp::WhatsAppChannel>>,
    #[cfg(feature = "channel-whatsapp-cloud")]
    pub whatsapp_app_secret: HashMap<String, Arc<str>>,
    #[cfg(feature = "channel-linq")]
    pub linq: HashMap<String, Arc<clawcrew_channels::linq::LinqChannel>>,
    #[cfg(feature = "channel-linq")]
    pub linq_signing_secrets: HashMap<String, Arc<str>>,
    #[cfg(feature = "channel-nextcloud")]
    pub nextcloud_talk: HashMap<String, Arc<clawcrew_channels::nextcloud_talk::NextcloudTalkChannel>>,
    #[cfg(feature = "channel-nextcloud")]
    pub nextcloud_talk_webhook_secret: HashMap<String, Arc<str>>,
    #[cfg(feature = "channel-email")]
    pub gmail_push: Option<Arc<clawcrew_channels::gmail_push::GmailPushChannel>>,
    pub observer: Arc<dyn clawcrew_runtime::observability::Observer>,
    pub tools_registry: Arc<Vec<clawcrew_api::tool::ToolSpec>>,
    pub tools_registry_by_agent: Arc<HashMap<String, Arc<Vec<clawcrew_api::tool::ToolSpec>>>>,
    pub cost_tracker: Option<Arc<CostTracker>>,
    pub event_tx: tokio::sync::broadcast::Sender<serde_json::Value>,
    pub event_buffer: Arc<crate::sse::EventBuffer>,
    pub shutdown_tx: tokio::sync::watch::Sender<bool>,
    pub reload_tx: Option<tokio::sync::watch::Sender<bool>>,
    pub node_registry: Arc<crate::nodes::NodeRegistry>,
    pub mdns_peer_registry: crate::nodes::mdns::MdnsPeerRegistry,
    pub path_prefix: String,
    pub web_dist_dir: Option<std::path::PathBuf>,
    pub session_backend: Option<Arc<dyn SessionBackend>>,
    pub session_queue: Arc<crate::session_queue::SessionActorQueue>,
    pub device_registry: Option<Arc<crate::api_pairing::DeviceRegistry>>,
    pub pending_pairings: Option<Arc<crate::api_pairing::PairingStore>>,
    pub canvas_store: CanvasStore,
    #[cfg(feature = "webauthn")]
    pub webauthn: Option<Arc<crate::api_webauthn::WebAuthnState>>,
    pub cancel_tokens: Arc<std::sync::Mutex<std::collections::HashMap<String, Arc<tokio_util::sync::CancellationToken>>>>,
    pub pending_reload: Arc<std::sync::atomic::AtomicBool>,
    pub tui_registry: Option<Arc<clawcrew_runtime::rpc::tui_identity::TuiRegistry>>,
    pub sop_engine: Option<Arc<std::sync::Mutex<clawcrew_runtime::sop::SopEngine>>>,
    pub sop_audit: Option<Arc<clawcrew_runtime::sop::SopAuditLogger>>,
}

/// Daemon-owned services whose lifecycle matches one supervised gateway run.
pub struct GatewaySupervision {
    readiness: Option<clawcrew_runtime::daemon::GatewayReadinessReporter>,
    plugin_webhooks: Arc<clawcrew_api::webhook::PluginWebhookRegistry>,
}

impl GatewaySupervision {
    #[must_use]
    pub fn new(
        readiness: Option<clawcrew_runtime::daemon::GatewayReadinessReporter>,
        plugin_webhooks: Arc<clawcrew_api::webhook::PluginWebhookRegistry>,
    ) -> Self {
        Self {
            readiness,
            plugin_webhooks,
        }
    }
}
