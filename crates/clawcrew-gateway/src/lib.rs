#![allow(
    clippy::to_string_in_format_args,
    clippy::useless_format,
    clippy::collapsible_if
)]

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

use anyhow::{Context, Result};
#[cfg(any(
    feature = "channel-email",
    feature = "channel-linq",
    feature = "channel-nextcloud",
    feature = "channel-whatsapp-cloud"
))]
use axum::body::Bytes;
#[cfg(any(
    feature = "channel-linq",
    feature = "channel-nextcloud",
    feature = "channel-whatsapp-cloud"
))]
use axum::extract::Path;
use axum::{
    Router,
    extract::{ConnectInfo, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{
        IntoResponse, Json, Response,
        sse::{Event as SseWireEvent, KeepAlive as SseKeepAlive, Sse as SseBody},
    },
    routing::{delete, get, post, put},
};
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Gateway session key prefix to avoid collisions with channel sessions.
pub(crate) const GW_SESSION_PREFIX: &str = "gw_";

/// Return the canonical persistence key for a gateway session.
///
/// Persistence backends apply the shared filesystem-safe normalization so
/// their in-memory and on-disk keys remain consistent.
pub(crate) fn gateway_session_key(session_id: &str) -> String {
    format!(
        "{GW_SESSION_PREFIX}{}",
        clawcrew_api::session_keys::sanitize_session_key(session_id)
    )
}

/// Return the process-local cancellation key for a gateway session.
///
/// Unlike persistence keys, cancellation keys must preserve the accepted
/// session id verbatim: filesystem-safe normalization is lossy and would make
/// distinct live sessions such as `team.alpha` and `team_alpha` cancel one
/// another.
pub(crate) fn gateway_cancel_key(session_id: &str) -> String {
    format!("{GW_SESSION_PREFIX}{session_id}")
}

/// Backoff after a transient `accept()` error so the serve loop does not
/// hot-spin while the condition (e.g. fd exhaustion) clears.
const ACCEPT_ERROR_BACKOFF_MS: u64 = 50;

/// File-descriptor exhaustion errno values, stable across the Unix targets
/// we support (Linux, macOS, BSD).
#[cfg(unix)]
const EMFILE: i32 = 24; // too many open files (this process)
#[cfg(unix)]
const ENFILE: i32 = 23; // too many open files (system-wide)

fn is_recoverable_accept_error(e: &std::io::Error) -> bool {
    use std::io::ErrorKind;
    if matches!(
        e.kind(),
        ErrorKind::ConnectionAborted | ErrorKind::Interrupted | ErrorKind::WouldBlock
    ) {
        return true;
    }
    #[cfg(unix)]
    if matches!(e.raw_os_error(), Some(EMFILE) | Some(ENFILE)) {
        return true;
    }
    false
}
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;
use uuid::Uuid;
#[cfg(any(
    feature = "channel-linq",
    feature = "channel-nextcloud",
    feature = "channel-whatsapp-cloud"
))]
use clawcrew_api::channel::Channel;
use clawcrew_api::memory_traits::MemoryStrategy;
use clawcrew_api::tool::ToolSpec;
#[cfg(feature = "channel-email")]
use clawcrew_channels::gmail_push::GmailPushChannel;
#[cfg(feature = "channel-linq")]
use clawcrew_channels::linq::LinqChannel;
#[cfg(feature = "channel-nextcloud")]
use clawcrew_channels::nextcloud_talk::NextcloudTalkChannel;
#[cfg(feature = "channel-whatsapp-cloud")]
use clawcrew_channels::whatsapp::WhatsAppChannel;
use clawcrew_config::policy::SecurityPolicy;
use clawcrew_config::schema::Config;
use clawcrew_infra::session_backend::SessionBackend;
use clawcrew_memory::{self, Memory, MemoryCategory};
use clawcrew_providers::{self, ModelProvider};
use clawcrew_runtime::agent::memory_strategy::DefaultMemoryStrategy;
use clawcrew_runtime::cost::CostTracker;
use clawcrew_runtime::i18n;
use clawcrew_runtime::platform;
use clawcrew_runtime::security::pairing::{
    PairingCodePolicy, PairingGuard, constant_time_eq, is_public_bind,
};
use clawcrew_runtime::tools;
use clawcrew_runtime::tools::CanvasStore;
use clawcrew_runtime::tools::scoped;

/// Maximum request body size (64KB) — prevents memory exhaustion
pub const MAX_BODY_SIZE: usize = 65_536;
/// Default request timeout (30s) — prevents slow-loris attacks.
pub const REQUEST_TIMEOUT_SECS: u64 = 30;

pub const LONG_RUNNING_REQUEST_TIMEOUT_SECS: u64 = 600;

/// Gateway request timeout (seconds) for routes other than the long-running
/// cron-trigger endpoint. Reads from typed config.
pub fn gateway_request_timeout_secs(cfg: &clawcrew_config::schema::GatewayConfig) -> u64 {
    cfg.request_timeout_secs
}

/// Manual cron-trigger request timeout (seconds), exempt from the
/// gateway-wide [`gateway_request_timeout_secs`] limit so synchronous agent
/// jobs can run to completion. Reads from typed config.
pub fn gateway_long_running_request_timeout_secs(
    cfg: &clawcrew_config::schema::GatewayConfig,
) -> u64 {
    cfg.long_running_request_timeout_secs
}
/// Sliding window used by gateway rate limiting.
pub const RATE_LIMIT_WINDOW_SECS: u64 = 60;

/// Fallback max distinct client keys tracked in gateway rate limiter.
pub const RATE_LIMIT_MAX_KEYS_DEFAULT: usize = 10_000;
/// Fallback max distinct idempotency keys retained in gateway memory.
pub const IDEMPOTENCY_MAX_KEYS_DEFAULT: usize = 10_000;

fn webhook_memory_key() -> String {
    format!("webhook_msg_{}", Uuid::new_v4())
}

#[cfg(feature = "channel-whatsapp-cloud")]
fn whatsapp_memory_key(msg: &clawcrew_api::channel::ChannelMessage) -> String {
    format!("whatsapp_{}_{}", msg.sender, msg.id)
}

#[cfg(feature = "channel-linq")]
fn linq_memory_key(msg: &clawcrew_api::channel::ChannelMessage) -> String {
    format!("linq_{}_{}", msg.sender, msg.id)
}

#[cfg(feature = "channel-nextcloud")]
fn nextcloud_talk_memory_key(msg: &clawcrew_api::channel::ChannelMessage) -> String {
    format!("nextcloud_talk_{}_{}", msg.sender, msg.id)
}

#[cfg(any(
    feature = "channel-linq",
    feature = "channel-nextcloud",
    feature = "channel-whatsapp-cloud"
))]
fn sender_session_id(channel: &str, msg: &clawcrew_api::channel::ChannelMessage) -> String {
    match &msg.thread_ts {
        Some(thread_id) => format!("{channel}_{thread_id}_{}", msg.sender),
        None => format!("{channel}_{}", msg.sender),
    }
}

#[cfg(feature = "channel-linq")]
fn linq_channel_ref(alias: &str) -> String {
    format!("linq.{alias}")
}

fn webhook_session_id(headers: &HeaderMap) -> Option<String> {
    const MAX_SESSION_ID_LEN: usize = 128;
    headers
        .get("X-Session-Id")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|value| value.len() <= MAX_SESSION_ID_LEN)
        .filter(|value| {
            value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        })
        .map(str::to_owned)
}

fn hash_webhook_secret(value: &str) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(value.as_bytes());
    hex::encode(digest)
}

/// How often the rate limiter sweeps stale IP entries from its map.
const RATE_LIMITER_SWEEP_INTERVAL_SECS: u64 = 300; // 5 minutes

#[derive(Debug)]
struct SlidingWindowRateLimiter {
    limit_per_window: u32,
    window: Duration,
    max_keys: usize,
    requests: Mutex<(HashMap<String, Vec<Instant>>, Instant)>,
}

impl SlidingWindowRateLimiter {
    fn new(limit_per_window: u32, window: Duration, max_keys: usize) -> Self {
        Self {
            limit_per_window,
            window,
            max_keys: max_keys.max(1),
            requests: Mutex::new((HashMap::new(), Instant::now())),
        }
    }

    fn prune_stale(requests: &mut HashMap<String, Vec<Instant>>, cutoff: Instant) {
        requests.retain(|_, timestamps| {
            timestamps.retain(|t| *t > cutoff);
            !timestamps.is_empty()
        });
    }

    fn allow(&self, key: &str) -> bool {
        if self.limit_per_window == 0 {
            return true;
        }

        let now = Instant::now();
        let cutoff = now.checked_sub(self.window).unwrap_or_else(Instant::now);

        let mut guard = self.requests.lock();
        let (requests, last_sweep) = &mut *guard;

        // Periodic sweep: remove keys with no recent requests
        if last_sweep.elapsed() >= Duration::from_secs(RATE_LIMITER_SWEEP_INTERVAL_SECS) {
            Self::prune_stale(requests, cutoff);
            *last_sweep = now;
        }

        if !requests.contains_key(key) && requests.len() >= self.max_keys {
            // Opportunistic stale cleanup before eviction under cardinality pressure.
            Self::prune_stale(requests, cutoff);
            *last_sweep = now;

            if requests.len() >= self.max_keys {
                let evict_key = requests
                    .iter()
                    .min_by_key(|(_, timestamps)| timestamps.last().copied().unwrap_or(cutoff))
                    .map(|(k, _)| k.clone());
                if let Some(evict_key) = evict_key {
                    requests.remove(&evict_key);
                }
            }
        }

        let entry = requests.entry(key.to_owned()).or_default();
        entry.retain(|instant| *instant > cutoff);

        if entry.len() >= self.limit_per_window as usize {
            return false;
        }

        entry.push(now);
        true
    }
}

#[derive(Debug)]
pub struct GatewayRateLimiter {
    pair: SlidingWindowRateLimiter,
    webhook: SlidingWindowRateLimiter,
}

impl GatewayRateLimiter {
    pub fn new(pair_per_minute: u32, webhook_per_minute: u32, max_keys: usize) -> Self {
        let window = Duration::from_secs(RATE_LIMIT_WINDOW_SECS);
        Self {
            pair: SlidingWindowRateLimiter::new(pair_per_minute, window, max_keys),
            webhook: SlidingWindowRateLimiter::new(webhook_per_minute, window, max_keys),
        }
    }

    fn allow_pair(&self, key: &str) -> bool {
        self.pair.allow(key)
    }

    fn allow_webhook(&self, key: &str) -> bool {
        self.webhook.allow(key)
    }
}

#[derive(Debug)]
pub struct IdempotencyStore {
    ttl: Duration,
    max_keys: usize,
    entries: Mutex<IdempotencyEntries>,
    #[cfg(feature = "plugins-wasm")]
    next_generation: std::sync::atomic::AtomicU64,
}

#[derive(Debug, Default)]
struct IdempotencyEntries {
    committed: HashMap<String, Instant>,
    /// Temporary plugin-delivery owners, bounded independently by `max_keys`.
    /// Keeping this separate prevents one slow plugin request from making the
    /// legacy boolean `record_if_new` path misclassify store pressure as a
    /// committed duplicate.
    #[cfg(feature = "plugins-wasm")]
    pending: HashMap<String, PendingIdempotencyReservation>,
}

#[cfg(feature = "plugins-wasm")]
#[derive(Debug)]
struct PendingIdempotencyReservation {
    generation: u64,
    status: tokio::sync::watch::Sender<clawcrew_api::webhook::WebhookReservationStatus>,
}

impl IdempotencyStore {
    pub fn new(ttl: Duration, max_keys: usize) -> Self {
        Self {
            ttl,
            max_keys: max_keys.max(1),
            entries: Mutex::new(IdempotencyEntries::default()),
            #[cfg(feature = "plugins-wasm")]
            next_generation: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// Returns true if this key is new and is now recorded.
    fn record_if_new(&self, key: &str) -> bool {
        let now = Instant::now();
        let mut entries = self.entries.lock();

        entries
            .committed
            .retain(|_, seen_at| now.duration_since(*seen_at) < self.ttl);

        let pending_contains = {
            #[cfg(feature = "plugins-wasm")]
            {
                entries.pending.contains_key(key)
            }
            #[cfg(not(feature = "plugins-wasm"))]
            {
                false
            }
        };
        if entries.committed.contains_key(key) || pending_contains {
            return false;
        }

        if entries.committed.len() >= self.max_keys {
            let evict_key = entries
                .committed
                .iter()
                .min_by_key(|(_, seen_at)| *seen_at)
                .map(|(k, _)| k.clone());
            if let Some(evict_key) = evict_key {
                entries.committed.remove(&evict_key);
            } else {
                return false;
            }
        }

        entries.committed.insert(key.to_owned(), now);
        true
    }

    #[cfg(feature = "plugins-wasm")]
    fn begin_reservation(&self, key: &str) -> clawcrew_api::webhook::WebhookReservation {
        use clawcrew_api::webhook::{
            WebhookReservation, WebhookReservationStatus, WebhookReservationToken,
            WebhookReservationWaiter,
        };

        let now = Instant::now();
        let mut entries = self.entries.lock();
        entries
            .committed
            .retain(|_, seen_at| now.duration_since(*seen_at) < self.ttl);
        if entries.committed.contains_key(key) {
            return WebhookReservation::Committed;
        }
        if let Some(pending) = entries.pending.get(key) {
            return WebhookReservation::InFlight(WebhookReservationWaiter::new(
                pending.status.subscribe(),
            ));
        }

        if entries.pending.len() >= self.max_keys {
            return WebhookReservation::Unavailable;
        }

        let generation = self
            .next_generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (status, _) = tokio::sync::watch::channel(WebhookReservationStatus::InFlight);
        entries.pending.insert(
            key.to_string(),
            PendingIdempotencyReservation { generation, status },
        );
        WebhookReservation::Owner(WebhookReservationToken::new(key.to_string(), generation))
    }

    #[cfg(feature = "plugins-wasm")]
    fn commit_reservation(&self, token: &clawcrew_api::webhook::WebhookReservationToken) -> bool {
        let mut entries = self.entries.lock();
        if entries
            .pending
            .get(token.key())
            .is_none_or(|pending| pending.generation != token.generation())
        {
            return false;
        }
        let Some(pending) = entries.pending.remove(token.key()) else {
            return false;
        };
        pending
            .status
            .send_replace(clawcrew_api::webhook::WebhookReservationStatus::Committed);
        let now = Instant::now();
        entries
            .committed
            .retain(|_, seen_at| now.duration_since(*seen_at) < self.ttl);
        if entries.committed.len() >= self.max_keys {
            let evict_key = entries
                .committed
                .iter()
                .min_by_key(|(_, seen_at)| *seen_at)
                .map(|(key, _)| key.clone());
            if let Some(evict_key) = evict_key {
                entries.committed.remove(&evict_key);
            }
        }
        entries.committed.insert(token.key().to_string(), now);
        true
    }

    #[cfg(feature = "plugins-wasm")]
    fn rollback_reservation(&self, token: &clawcrew_api::webhook::WebhookReservationToken) -> bool {
        let mut entries = self.entries.lock();
        if entries
            .pending
            .get(token.key())
            .is_none_or(|pending| pending.generation != token.generation())
        {
            return false;
        }
        let Some(pending) = entries.pending.remove(token.key()) else {
            return false;
        };
        pending
            .status
            .send_replace(clawcrew_api::webhook::WebhookReservationStatus::RolledBack);
        true
    }
}

fn parse_client_ip(value: &str) -> Option<IpAddr> {
    let value = value.trim().trim_matches('"').trim();
    if value.is_empty() {
        return None;
    }

    if let Ok(ip) = value.parse::<IpAddr>() {
        return Some(ip);
    }

    if let Ok(addr) = value.parse::<SocketAddr>() {
        return Some(addr.ip());
    }

    let value = value.trim_matches(['[', ']']);
    value.parse::<IpAddr>().ok()
}

fn dirs_data_local() -> Option<std::path::PathBuf> {
    directories::BaseDirs::new().map(|d| d.data_local_dir().to_path_buf())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebDashboardAvailability {
    Embedded,
    Filesystem(std::path::PathBuf),
}

pub fn resolve_web_dashboard_availability(config: &Config) -> Option<WebDashboardAvailability> {
    #[cfg(feature = "embedded-web")]
    {
        let _ = config;
        Some(WebDashboardAvailability::Embedded)
    }
    #[cfg(not(feature = "embedded-web"))]
    {
        resolve_web_dist_dir(config).map(WebDashboardAvailability::Filesystem)
    }
}

fn has_servable_dashboard_index(dir: &std::path::Path) -> bool {
    let Ok(canonical_root) = std::fs::canonicalize(dir) else {
        return false;
    };
    let Ok(canonical_index) = std::fs::canonicalize(canonical_root.join("index.html")) else {
        return false;
    };

    canonical_index.starts_with(&canonical_root) && canonical_index.is_file()
}

pub fn resolve_web_dist_dir(config: &Config) -> Option<std::path::PathBuf> {
    match config
        .gateway
        .web_dist_dir
        .as_ref()
        .map(std::path::PathBuf::from)
    {
        Some(explicit) if has_servable_dashboard_index(&explicit) => Some(explicit),
        Some(_) | None => auto_detect_web_dist_dir(),
    }
}

fn auto_detect_web_dist_dir() -> Option<std::path::PathBuf> {
    let mut candidates = vec![
        std::path::PathBuf::from("web/dist"),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("web/dist")))
            .unwrap_or_default(),
        std::path::PathBuf::from("/clawcrew-data/web/dist"),
        std::path::PathBuf::from("/usr/share/clawcrewlabs/web/dist"),
    ];
    if let Some(data_dir) = dirs_data_local() {
        candidates.push(data_dir.join("clawcrew/web/dist"));
    }
    candidates
        .into_iter()
        .find(|p| !p.as_os_str().is_empty() && has_servable_dashboard_index(p))
}

fn forwarded_client_ip(headers: &HeaderMap) -> Option<IpAddr> {
    if let Some(xff) = headers.get("X-Forwarded-For").and_then(|v| v.to_str().ok()) {
        for candidate in xff.split(',') {
            if let Some(ip) = parse_client_ip(candidate) {
                return Some(ip);
            }
        }
    }

    headers
        .get("X-Real-IP")
        .and_then(|v| v.to_str().ok())
        .and_then(parse_client_ip)
}

fn client_key_from_request(
    peer_addr: Option<SocketAddr>,
    headers: &HeaderMap,
    trust_forwarded_headers: bool,
) -> String {
    if trust_forwarded_headers && let Some(ip) = forwarded_client_ip(headers) {
        return ip.to_string();
    }

    peer_addr
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn normalize_max_keys(configured: usize, fallback: usize) -> usize {
    if configured == 0 {
        fallback.max(1)
    } else {
        configured
    }
}

fn default_agent_alias(config: &Config) -> Option<String> {
    config
        .agents
        .iter()
        .filter(|(_, a)| a.enabled)
        .map(|(alias, _)| alias.clone())
        .min()
}

/// Owned guard for [`AppState::config_write_lock`]. Owned (not borrowed) so
/// a handler can release it explicitly at its commit point, or pass it by
/// value into a delegated helper without lifetime coupling.
pub(crate) type ConfigWriteGuard = tokio::sync::OwnedMutexGuard<()>;

/// Shared state for all axum handlers
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<Config>>,

    /// Serializes the read-mutate-save-swap critical section of every HTTP
    /// handler that mutates `config` (per-property PUT/DELETE/PATCH, map-key
    /// create/delete/rename, channel bind, config migrate, section select,
    /// quickstart apply, cron settings patch, pairing-token persistence). A
    /// tokio mutex, not `parking_lot`, because the guard must survive the
    /// `.await` on config-save I/O. Mirrors
    /// `RpcContext::config_write_lock` in the RPC path.
    ///
    /// Invariant: every mutation of `config` must happen while holding this
    /// mutex, acquired before the first `config` read-for-modify and held
    /// through the swap that installs the mutated snapshot. Never acquire it
    /// while holding a `config` guard — lock order is this mutex first,
    /// `config` second, always. A writer that bypasses this lock and swaps
    /// the live config while a concurrent writer's save is in flight loses
    /// that writer's change — clobbered in memory and, if its save hadn't
    /// landed yet, on disk too.
    pub config_write_lock: Arc<tokio::sync::Mutex<()>>,
    pub model_provider: Arc<dyn ModelProvider>,
    pub model: String,
    /// `None` means "let the provider decide" — required for models
    /// (e.g. claude-opus-4-7) that reject the field. Always preserve
    /// `Option<f64>` end-to-end; never substitute a hardcoded default.
    pub temperature: Option<f64>,
    pub mem: Arc<dyn Memory>,
    pub memory_strategy: Arc<dyn MemoryStrategy>,
    pub auto_save: bool,
    pub pairing: Arc<PairingGuard>,
    pub trust_forwarded_headers: bool,
    pub rate_limiter: Arc<GatewayRateLimiter>,
    pub auth_limiter: Arc<auth_rate_limit::AuthRateLimiter>,
    pub idempotency_store: Arc<IdempotencyStore>,
    /// `WhatsApp` channel instances keyed by config alias. Webhooks route by
    /// `/whatsapp/{alias}`; the bare `/whatsapp` path falls back to the first
    /// instance (see [`api_webhook`]).
    #[cfg(feature = "channel-whatsapp-cloud")]
    pub whatsapp: HashMap<String, Arc<WhatsAppChannel>>,
    /// `WhatsApp` app secrets keyed by alias for webhook signature verification
    /// (`X-Hub-Signature-256`).
    #[cfg(feature = "channel-whatsapp-cloud")]
    pub whatsapp_app_secret: HashMap<String, Arc<str>>,
    #[cfg(feature = "channel-linq")]
    pub linq: HashMap<String, Arc<LinqChannel>>,
    /// Linq webhook signing secrets per alias
    #[cfg(feature = "channel-linq")]
    pub linq_signing_secrets: HashMap<String, Arc<str>>,
    /// Nextcloud Talk channel instances keyed by config alias.
    #[cfg(feature = "channel-nextcloud")]
    pub nextcloud_talk: HashMap<String, Arc<NextcloudTalkChannel>>,
    /// Nextcloud Talk webhook secrets keyed by alias for signature verification.
    #[cfg(feature = "channel-nextcloud")]
    pub nextcloud_talk_webhook_secret: HashMap<String, Arc<str>>,
    /// Gmail Pub/Sub push notification channel
    #[cfg(feature = "channel-email")]
    pub gmail_push: Option<Arc<GmailPushChannel>>,
    /// Observability backend for metrics scraping
    pub observer: Arc<dyn clawcrew_runtime::observability::Observer>,
    /// Registered tool specs (for web dashboard tools page). This is the
    /// default (no `?agent=`) listing, seeded from the deterministically
    /// smallest enabled agent alias.
    pub tools_registry: Arc<Vec<ToolSpec>>,
    /// Per-agent tool-spec listings keyed by agent alias, powering the
    /// agent-aware `GET /api/tools?agent=<alias>` view so the WebUI Tools
    /// page can show each agent's scoped tool set. Falls back to
    /// `tools_registry` for an unknown or omitted alias.
    pub tools_registry_by_agent: Arc<HashMap<String, Arc<Vec<ToolSpec>>>>,
    /// Cost tracker (optional, for web dashboard cost page)
    pub cost_tracker: Option<Arc<CostTracker>>,
    /// SSE broadcast channel for real-time events
    pub event_tx: tokio::sync::broadcast::Sender<serde_json::Value>,
    /// Ring buffer of recent events for history replay
    pub event_buffer: Arc<sse::EventBuffer>,
    /// Shutdown signal sender for graceful shutdown
    pub shutdown_tx: tokio::sync::watch::Sender<bool>,
    /// Reload signal sender owned by the daemon. /admin/reload writes `true`
    /// here; the daemon's wait loop reacts and re-instantiates every
    /// subsystem in place. `None` when running standalone (`clawcrew gateway start`)
    /// — reload then degrades to a 503 with a clear message.
    pub reload_tx: Option<tokio::sync::watch::Sender<bool>>,
    /// Registry of dynamically connected nodes
    pub node_registry: Arc<nodes::NodeRegistry>,
    /// LAN-local peer hints discovered by multicast. These are informational
    /// only; they never authorize or connect a peer.
    pub mdns_peer_registry: nodes::mdns::MdnsPeerRegistry,
    /// Path prefix for reverse-proxy deployments (empty string = no prefix)
    pub path_prefix: String,
    /// Filesystem path to `web/dist/` for serving the dashboard (None = API-only)
    pub web_dist_dir: Option<std::path::PathBuf>,
    /// Session backend for persisting gateway WS chat sessions
    pub session_backend: Option<Arc<dyn SessionBackend>>,
    /// Per-session actor queue for serializing concurrent turns
    pub session_queue: Arc<session_queue::SessionActorQueue>,
    /// Device registry for paired device management
    pub device_registry: Option<Arc<api_pairing::DeviceRegistry>>,
    /// Pending pairing request store
    pub pending_pairings: Option<Arc<api_pairing::PairingStore>>,
    /// Shared canvas store for Live Canvas (A2UI) system
    pub canvas_store: CanvasStore,
    /// WebAuthn state for hardware key authentication (optional, requires `webauthn` feature)
    #[cfg(feature = "webauthn")]
    pub webauthn: Option<Arc<api_webauthn::WebAuthnState>>,
    /// Per-session cancellation tokens for aborting in-flight agent responses.
    /// Key is session_key (e.g. `gw_<session_id>`), value is the token for the
    /// current turn. Entries are inserted before each turn and removed after
    /// completion (normal or cancelled). The outer `Arc` provides turn identity
    /// so late cleanup cannot remove a replacement turn's token.
    pub cancel_tokens: Arc<
        std::sync::Mutex<
            std::collections::HashMap<String, Arc<tokio_util::sync::CancellationToken>>,
        >,
    >,
    pub pending_reload: Arc<std::sync::atomic::AtomicBool>,
    /// TUI session registry from the daemon (for /api/tuis endpoint).
    /// `None` when the gateway runs standalone without a daemon.
    pub tui_registry: Option<Arc<clawcrew_runtime::rpc::tui_identity::TuiRegistry>>,
    /// Shared SOP engine from the daemon (for WS agent sessions).
    /// `None` when the gateway runs standalone — sessions build their own.
    pub sop_engine: Option<Arc<std::sync::Mutex<clawcrew_runtime::sop::SopEngine>>>,
    /// Shared SOP audit logger from the daemon (for WS agent sessions).
    pub sop_audit: Option<Arc<clawcrew_runtime::sop::SopAuditLogger>>,
}

/// Daemon-owned services whose lifecycle matches one supervised gateway run.
pub struct GatewaySupervision {
    readiness: Option<clawcrew_runtime::daemon::GatewayReadinessReporter>,
    plugin_webhooks: Arc<clawcrew_api::webhook::PluginWebhookRegistry>,
}

impl GatewaySupervision {
    /// Pair startup readiness with the channel supervisor's route generation.
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

/// Binds the `SystemGateway` gRPC server on `127.0.0.1:50052` and serves it
/// until the process exits. D1: lets the Go engine call back into Rust for
/// native-tool execution (bash/git/file I/O). Loopback-only — this is an
/// internal engine<->gateway channel, never exposed on a public interface.
pub async fn start_grpc_server() -> Result<()> {
    use grpc_system_gateway::system_gateway_server::SystemGatewayServer;

    let addr: SocketAddr = "127.0.0.1:50052".parse().expect("valid loopback addr");
    ::clawcrew_log::record!(
        INFO,
        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
        "ClawCrew SystemGateway gRPC server listening on {addr}"
    );

    tonic::transport::Server::builder()
        .add_service(SystemGatewayServer::new(
            grpc_system_gateway::SystemGatewayService::new(None),
        ))
        .serve(addr)
        .await?;
    Ok(())
}

/// Run the HTTP gateway using axum with proper HTTP/1.1 compliance.
pub async fn run_gateway(
    host: &str,
    port: u16,
    config: Config,
    external_event_tx: Option<tokio::sync::broadcast::Sender<serde_json::Value>>,
    // Reload controls owned by the daemon for supervised runs. RPC reloads
    // write to `shutdown_tx` before signalling daemon reload so the listener
    // releases its socket before the replacement gateway binds. /admin/reload
    // writes to both controls directly. Standalone gateway passes `None`.
    reload_controls: Option<clawcrew_runtime::daemon::GatewayReloadControls>,
    // TUI session registry from the daemon for the /api/tuis endpoint.
    tui_registry: Option<Arc<clawcrew_runtime::rpc::tui_identity::TuiRegistry>>,
    canvas_store: Option<CanvasStore>,
    // Shared SOP engine from the daemon. `None` when standalone — sessions build their own.
    sop_engine: Option<Arc<std::sync::Mutex<clawcrew_runtime::sop::SopEngine>>>,
    sop_audit: Option<Arc<clawcrew_runtime::sop::SopAuditLogger>>,
    readiness: Option<clawcrew_runtime::daemon::GatewayReadinessReporter>,
) -> Result<()> {
    Box::pin(run_gateway_with_plugin_webhooks(
        host,
        port,
        config,
        external_event_tx,
        reload_controls,
        tui_registry,
        canvas_store,
        sop_engine,
        sop_audit,
        GatewaySupervision::new(
            readiness,
            Arc::new(clawcrew_api::webhook::PluginWebhookRegistry::new()),
        ),
    ))
    .await
}

/// Run the supervised gateway with the daemon generation's channel-plugin
/// webhook registry. Standalone callers use [`run_gateway`], because no channel
/// supervisor exists there to publish live routes.
#[allow(clippy::too_many_lines)]
pub async fn run_gateway_with_plugin_webhooks(
    host: &str,
    port: u16,
    config: Config,
    external_event_tx: Option<tokio::sync::broadcast::Sender<serde_json::Value>>,
    reload_controls: Option<clawcrew_runtime::daemon::GatewayReloadControls>,
    tui_registry: Option<Arc<clawcrew_runtime::rpc::tui_identity::TuiRegistry>>,
    canvas_store: Option<CanvasStore>,
    sop_engine: Option<Arc<std::sync::Mutex<clawcrew_runtime::sop::SopEngine>>>,
    sop_audit: Option<Arc<clawcrew_runtime::sop::SopAuditLogger>>,
    supervision: GatewaySupervision,
) -> Result<()> {
    let GatewaySupervision {
        readiness,
        plugin_webhooks,
    } = supervision;
    // ── Security: warn on public bind without tunnel or explicit opt-in ──
    if is_public_bind(host)
        && config.tunnel.tunnel_provider == "none"
        && !config.gateway.allow_public_bind
    {
        ::clawcrew_log::record!(
            WARN,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_outcome(::clawcrew_log::EventOutcome::Unknown),
            "⚠️  Binding to {host} — gateway will be exposed to all network interfaces.\n\
             Suggestion: use --host 127.0.0.1 (default), configure a tunnel, or set\n\
             [gateway] allow_public_bind = true in config.toml to silence this warning.\n\n\
             Docker/VM: if you are running inside a container or VM, this is expected."
        );
    }
    let config_state = Arc::new(RwLock::new(config.clone()));

    // ── Hooks ──────────────────────────────────────────────────────
    let hooks: Option<std::sync::Arc<clawcrew_runtime::hooks::HookRunner>> = if config.hooks.enabled
    {
        Some(std::sync::Arc::new(
            clawcrew_runtime::hooks::HookRunner::new(),
        ))
    } else {
        None
    };

    let addr: SocketAddr = match clawcrew_infra::parse_gateway_bind_socket_addr(host, port) {
        Ok(a) => a,
        Err(e) => {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({
                        "host": host,
                        "port": port,
                        "error": format!("{e}"),
                    })),
                "Gateway: host:port did not parse as a SocketAddr; falling back to \
                 127.0.0.1 so the gateway can still boot. Fix [gateway] host and \
                 POST /admin/reload."
            );
            clawcrew_infra::fallback_gateway_bind_socket_addr(port)
        }
    };
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let actual_addr = listener.local_addr()?;
    let actual_port = actual_addr.port();
    let display_addr = format!("{host}:{actual_port}");

    // D1: SystemGateway gRPC server so the Go engine can call back into Rust
    // for native-tool execution (bash/git/file I/O). Independent listener,
    // independent lifecycle from the HTTP gateway above — a failure here
    // must not block the dashboard/API from booting.
    tokio::spawn(start_grpc_server());

    // Seed the install-wide default provider from the first entry that
    // actually declares a `model`. Entries without one cannot serve as the
    // default (there is no model string to pair with the provider), so
    // skipping them keeps the boot family, credentials, runtime options, and
    // model coherent — the previous "first entry, whatever it is" pick could
    // build the provider from one entry while `resolve_default_model` sourced
    // the model from another.
    let (boot_family, boot_alias, boot_entry) = config
        .providers
        .models
        .first_entry_with_model()
        .map(|(f, a, e)| (f.to_string(), a.to_string(), Some(e)))
        .unwrap_or_else(|| ("openrouter".to_string(), "default".to_string(), None));
    let fallback = boot_entry;
    let model_provider_name = boot_family.as_str();
    let (model_provider, boot_provider_failed): (Arc<dyn ModelProvider>, bool) =
        match clawcrew_providers::create_resilient_model_provider_from_ref(
            &config,
            model_provider_name,
            fallback.and_then(|e| e.api_key.as_deref()),
            fallback.and_then(|e| e.uri.as_deref()),
            &config.reliability,
            &clawcrew_providers::provider_runtime_options_for_alias(
                &config,
                &boot_family,
                &boot_alias,
            ),
        ) {
            Ok(p) => (Arc::from(p), false),
            Err(e) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note,)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({
                            "model_provider": model_provider_name,
                            "alias": boot_alias,
                            "error": format!("{e}"),
                        })),
                    "Gateway: seed model_provider failed to construct; booting in \
                     needs_quickstart mode so /quickstart and /admin/reload stay \
                     reachable. Fix the [providers.models.<type>.<alias>] entry \
                     and POST /admin/reload."
                );
                (
                    Arc::new(UnconfiguredModelProvider) as Arc<dyn ModelProvider>,
                    true,
                )
            }
        };
    let model = if boot_provider_failed {
        String::new()
    } else {
        // `first_entry_with_model` guarantees a non-empty model for the boot
        // entry, so reaching the empty fallback means no entry declares a
        // model at all — the needs_quickstart onboarding path.
        let model = fallback
            .and_then(|e| e.model.as_deref())
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(ToString::to_string);
        if model.is_none() {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({"display_addr": display_addr})),
                &format!(
                    "Gateway booting without a configured model. Visit http://{display_addr}/quickstart to complete browser quickstart. Chat endpoints will return 503 needs_quickstart until at least one [providers.models.<type>.<alias>] model = \"...\" is set."
                )
            );
        }
        model.unwrap_or_default()
    };
    // Preserve `Option<f64>` end-to-end. Substituting a hardcoded default
    // here would clobber the "let the provider decide" intent for models
    // (e.g. claude-opus-4-7) that reject `temperature`.
    let temperature: Option<f64> = fallback.and_then(|e| e.temperature);
    let mem: Arc<dyn Memory> = if config.agents.is_empty() {
        Arc::new(clawcrew_memory::NoneMemory::new("none"))
    } else {
        match clawcrew_memory::create_memory_from_config(
            &config,
            fallback.and_then(|e| e.api_key.as_deref()),
        ) {
            Ok(m) => Arc::from(m),
            Err(e) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note,)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"error": format!("{e}")})),
                    "Gateway: memory backend failed to construct; falling back to \
                     NoneMemory so the gateway can still boot. Fix [memory] and \
                     POST /admin/reload."
                );
                Arc::new(clawcrew_memory::NoneMemory::new("none"))
            }
        }
    };
    let runtime: Arc<dyn platform::RuntimeAdapter> = match platform::create_runtime(&config.runtime)
    {
        Ok(r) => Arc::from(r),
        Err(e) => {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note,)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({
                        "runtime_kind": config.runtime.kind,
                        "error": format!("{e}"),
                    })),
                "Gateway: runtime adapter failed to construct; falling back to \
                     NativeRuntime so the gateway can still boot. Fix [runtime] and \
                     POST /admin/reload."
            );
            Arc::new(platform::NativeRuntime::new())
        }
    };
    let memory_strategy: Arc<dyn MemoryStrategy> = Arc::new(DefaultMemoryStrategy::with_config(
        mem.clone(),
        config.memory.clone(),
        config.data_dir.clone(),
    ));
    let canvas_store = canvas_store.unwrap_or_default();
    let agent_alias_opt = default_agent_alias(&config);

    let (composio_key, composio_entity_id) = if config.composio.enabled {
        (
            config.composio.api_key.as_deref(),
            Some(config.composio.entity_id.as_str()),
        )
    } else {
        (None, None)
    };

    let agent_setup: Option<(
        clawcrew_config::schema::RiskProfileConfig,
        Arc<SecurityPolicy>,
    )> = agent_alias_opt.as_ref().and_then(|agent_alias| {
        let Some(risk_profile) = config.risk_profile_for_agent(agent_alias) else {
            ::clawcrew_log::record!(WARN, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_outcome(::clawcrew_log::EventOutcome::Unknown).with_attrs(::serde_json::json!({"agent": agent_alias, "agent_alias": agent_alias})), "Gateway: agents..risk_profile does not name a configured risk_profiles entry; booting with empty tools registry. Fix via /admin/reload or /quickstart.");
            return None;
        };
        let risk_profile = risk_profile.clone();
        let security = match SecurityPolicy::for_agent(&config, agent_alias) {
            Ok(s) => Arc::new(s),
            Err(e) => {
                ::clawcrew_log::record!(WARN, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_outcome(::clawcrew_log::EventOutcome::Unknown).with_attrs(::serde_json::json!({"agent": agent_alias, "error": format!("{}", e), "agent_alias": agent_alias})), "Gateway: agent SecurityPolicy failed to build; booting with empty tools registry. Fix [agents.] via /admin/reload or /quickstart.");
                return None;
            }
        };
        Some((risk_profile, security))
    });

    let (tools_registry_raw, _delegate_handle_gw) = match (&agent_alias_opt, agent_setup) {
        (Some(agent_alias), Some((risk_profile, security))) => {
            let all_tools_result = tools::all_tools_with_runtime(
                Arc::new(config.clone()),
                &security,
                &risk_profile,
                agent_alias,
                Arc::clone(&runtime),
                Arc::clone(&mem),
                composio_key,
                composio_entity_id,
                &config.browser,
                &config.http_request,
                &config.web_fetch,
                &config.data_dir,
                &config.agents,
                config
                    .model_provider_for_agent(agent_alias)
                    .and_then(|e| e.api_key.as_deref()),
                &config,
                Some(canvas_store.clone()),
                false,
                None,
                sop_engine.clone(),
                sop_audit.clone(),
                None,
            )?;
            let assembled = scoped::ScopedToolRegistry::assemble(scoped::ScopedAssembly {
                config: &config,
                agent_alias,
                security: &security,
                built: all_tools_result,
                // The gateway registers no skills today; unifying the two
                // skill loaders through this seam is the Epic F follow-up.
                skills: &[],
                runtime: Arc::clone(&runtime),
                caller_allowed: None,
                connect_mcp: true,
                // Gateway tool-listing path: short-lived, no cross-turn reuse
                // contract, so the per-call connect is correct.
                mcp_registry: None,
                // Listing-only registry: loading peripherals physically opens
                // hardware (exclusive serial holds) that the live turn paths
                // need. Never connect them for a registry no turn runs against.
                connect_peripherals: false,
                emit_assembly_logs: false,
                exclude_memory: false,
                acp_delivery: false,
                list_deferred_mcp_specs: true,
            })
            .await;
            let reaction_handle_gw_opt = Some(assembled.reaction_handle.clone());
            let channel_names = clawcrew_channels::orchestrator::register_channels_for_tools(
                &config,
                &assembled.ask_user_handle,
                &assembled.channel_room_handle,
                &reaction_handle_gw_opt,
                &assembled.poll_handle,
                &assembled.escalate_handle,
            );
            if !channel_names.is_empty() {
                ::clawcrew_log::record!(
                    INFO,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_attrs(::serde_json::json!({"count": channel_names.len()})),
                    &format!(
                        "Registered {} channel(s) for dashboard agent",
                        channel_names.len()
                    ),
                );
            }
            // Listing-only registry: no turn runs against it, so the
            // deferred-MCP prompt section and activation handle returned by
            // `assemble` have no consumer here (live gateway chat resolves
            // its tools inside process_message).
            (assembled.registry.into_inner(), assembled.delegate_handle)
        }
        (Some(_), None) => {
            // Agent existed but its config failed to resolve. Warned
            // above; fall through to the empty-registry shape.
            (Vec::new(), None)
        }
        (None, _) => {
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_attrs(::serde_json::json!({"display_addr": display_addr})),
                &format!(
                    "Gateway: no [agents.<alias>] configured — booting with empty tools registry. Visit http://{display_addr}/quickstart to add an agent."
                )
            );
            (Vec::new(), None)
        }
    };

    let tools_registry: Arc<Vec<ToolSpec>> =
        Arc::new(tools_registry_raw.iter().map(|t| t.spec()).collect());

    let mut tools_registry_by_agent: HashMap<String, Arc<Vec<ToolSpec>>> = HashMap::new();
    if let Some(default_alias) = agent_alias_opt.as_ref() {
        tools_registry_by_agent.insert(default_alias.clone(), Arc::clone(&tools_registry));
    }
    let mut other_aliases: Vec<String> = config
        .agents
        .iter()
        .filter(|(alias, a)| a.enabled && Some(*alias) != agent_alias_opt.as_ref())
        .map(|(alias, _)| alias.clone())
        .collect();
    other_aliases.sort();
    for alias in other_aliases {
        let Some(risk_profile) = config.risk_profile_for_agent(&alias) else {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({"agent_alias": alias})),
                "Gateway: agent risk_profile does not resolve; skipping its /api/tools listing."
            );
            continue;
        };
        let risk_profile = risk_profile.clone();
        let security = match SecurityPolicy::for_agent(&config, &alias) {
            Ok(s) => Arc::new(s),
            Err(e) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(
                            ::serde_json::json!({"agent_alias": alias, "error": format!("{e}")})
                        ),
                    "Gateway: agent SecurityPolicy failed to build; skipping its /api/tools listing."
                );
                continue;
            }
        };
        let agent_tools_result = tools::all_tools_with_runtime(
            Arc::new(config.clone()),
            &security,
            &risk_profile,
            &alias,
            Arc::clone(&runtime),
            Arc::clone(&mem),
            composio_key,
            composio_entity_id,
            &config.browser,
            &config.http_request,
            &config.web_fetch,
            &config.data_dir,
            &config.agents,
            config
                .model_provider_for_agent(&alias)
                .and_then(|e| e.api_key.as_deref()),
            &config,
            Some(canvas_store.clone()),
            false,
            None,
            sop_engine.clone(),
            sop_audit.clone(),
            None,
        )?;
        // Same gated seam as the dashboard seed above, so this listing shows
        // the agent's policy-filtered set (filter + MCP). The tools are only
        // enumerated for their specs, never invoked, so the returned channel
        // handles, deferred section, and activation handle are unused.
        let assembled = scoped::ScopedToolRegistry::assemble(scoped::ScopedAssembly {
            config: &config,
            agent_alias: &alias,
            security: &security,
            built: agent_tools_result,
            // Same divergence note as the dashboard seed: no skills on the
            // gateway until Epic F unifies the loaders.
            skills: &[],
            runtime: Arc::clone(&runtime),
            caller_allowed: None,
            connect_mcp: true,
            // Gateway tool-listing path: short-lived, no cross-turn reuse
            // contract, so the per-call connect is correct.
            mcp_registry: None,
            // Same as the seed: never open hardware for a listing (and
            // `config.peripherals` is global - N per-agent opens of the same
            // boards would fail against the first holder anyway).
            connect_peripherals: false,
            emit_assembly_logs: false,
            exclude_memory: false,
            acp_delivery: false,
            list_deferred_mcp_specs: true,
        })
        .await;
        let specs: Vec<ToolSpec> = assembled.registry.iter().map(|t| t.spec()).collect();
        tools_registry_by_agent.insert(alias, Arc::new(specs));
    }
    let tools_registry_by_agent: Arc<HashMap<String, Arc<Vec<ToolSpec>>>> =
        Arc::new(tools_registry_by_agent);

    // Cost tracker — process-global singleton so channels share the same instance
    let cost_tracker = CostTracker::get_or_init_global(config.cost.clone(), &config.data_dir);

    // Live model-pricing refresher (once per process; idempotent, no-op unless a
    // provider sets `live_pricing = true`). Each call re-binds the refresher's
    // config handle, so reloads that re-instantiate the config Arc are honored
    // without a restart; shares the global price snapshot the cost path reads.
    clawcrew_providers::pricing::spawn_refresher(config_state.clone());

    // SSE broadcast channel for real-time events.
    // Use an externally provided sender (e.g. from the daemon) so that other
    // components (cron, heartbeat) can publish events to the same bus.
    let event_tx = external_event_tx.unwrap_or_else(|| {
        let (tx, _rx) = tokio::sync::broadcast::channel::<serde_json::Value>(256);
        tx
    });
    let event_buffer = Arc::new(sse::EventBuffer::new(500));
    // WhatsApp channel instances (one per cloud-configured alias), keyed by
    // alias so `/whatsapp/{alias}` webhooks reach the matching instance
    #[cfg(feature = "channel-whatsapp-cloud")]
    let whatsapp_channel: HashMap<String, Arc<WhatsAppChannel>> = config
        .channels
        .whatsapp
        .iter()
        .filter(|(_, wa)| wa.is_cloud_config())
        .map(|(alias, wa)| {
            let peer_resolver: Arc<dyn Fn() -> Vec<String> + Send + Sync> = {
                let cfg_arc = config_state.clone();
                let alias = alias.clone();
                Arc::new(move || cfg_arc.read().channel_external_peers("whatsapp", &alias))
            };
            (
                alias.clone(),
                Arc::new(WhatsAppChannel::new(
                    wa.access_token.clone().unwrap_or_default(),
                    wa.phone_number_id.clone().unwrap_or_default(),
                    wa.verify_token.clone().unwrap_or_default(),
                    alias.clone(),
                    peer_resolver,
                )),
            )
        })
        .collect();

    // WhatsApp app secrets keyed by alias for webhook signature verification.
    #[cfg(feature = "channel-whatsapp-cloud")]
    let whatsapp_app_secret: HashMap<String, Arc<str>> = config
        .channels
        .whatsapp
        .iter()
        .filter_map(|(alias, wa)| {
            let secret = wa
                .app_secret
                .as_deref()
                .map(str::trim)
                .filter(|secret| !secret.is_empty())
                .map(ToOwned::to_owned)?;
            Some((alias.clone(), Arc::from(secret)))
        })
        .collect();

    // Linq channel instances (multi-tenant: one per alias)
    #[cfg(feature = "channel-linq")]
    let linq_channels: HashMap<String, Arc<LinqChannel>> = config
        .channels
        .linq
        .iter()
        .filter(|(_, lq)| lq.enabled)
        .map(|(alias, lq)| {
            let peer_resolver: Arc<dyn Fn() -> Vec<String> + Send + Sync> = {
                let cfg_arc = config_state.clone();
                let alias = alias.clone();
                Arc::new(move || cfg_arc.read().channel_external_peers("linq", &alias))
            };
            (
                alias.clone(),
                Arc::new(LinqChannel::new(
                    lq.api_token.clone(),
                    lq.from_phone.clone(),
                    alias.clone(),
                    peer_resolver,
                )),
            )
        })
        .collect();

    // Linq signing secrets per alias.
    #[cfg(feature = "channel-linq")]
    let linq_signing_secrets: HashMap<String, Arc<str>> = config
        .channels
        .linq
        .iter()
        .filter_map(|(alias, lq)| {
            let secret = lq
                .signing_secret
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)?;
            Some((alias.clone(), Arc::from(secret)))
        })
        .collect();

    // Nextcloud Talk channel instances keyed by alias.
    #[cfg(feature = "channel-nextcloud")]
    let nextcloud_talk_channel: HashMap<String, Arc<NextcloudTalkChannel>> = config
        .channels
        .nextcloud_talk
        .iter()
        .map(|(alias, nc)| {
            let peer_resolver: Arc<dyn Fn() -> Vec<String> + Send + Sync> = {
                let cfg_arc = config_state.clone();
                let alias = alias.clone();
                Arc::new(move || {
                    cfg_arc
                        .read()
                        .channel_external_peers("nextcloud_talk", &alias)
                })
            };
            (
                alias.clone(),
                Arc::new(NextcloudTalkChannel::new(
                    nc.base_url.clone(),
                    // Resolve the same installed-bot secret used by inbound
                    // verification. `webhook_secret` is canonical and
                    // `bot_token` is a deprecated alias for the same value.
                    nc.resolve_bot_secret().unwrap_or_else(|e| {
                        ::clawcrew_log::record!(
                            WARN,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Note
                            )
                            .with_outcome(::clawcrew_log::EventOutcome::Failure),
                            &e.to_string()
                        );
                        None
                    }),
                    nc.bot_name.clone().unwrap_or_default(),
                    alias.clone(),
                    peer_resolver,
                )),
            )
        })
        .collect();

    // Nextcloud Talk webhook secrets keyed by alias for signature verification.
    #[cfg(feature = "channel-nextcloud")]
    let nextcloud_talk_webhook_secret: HashMap<String, Arc<str>> = config
        .channels
        .nextcloud_talk
        .iter()
        .filter_map(|(alias, nc)| {
            // Same resolver as the outbound signing path: Nextcloud installs one
            // secret per bot, so inbound verification and outbound signing must
            // agree. A conflict or an unresolved secret yields no entry, and the
            // handler rejects rather than skipping verification.
            let secret = match nc.resolve_bot_secret() {
                Ok(Some(secret)) => secret,
                Ok(None) => return None,
                Err(e) => {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_outcome(::clawcrew_log::EventOutcome::Failure),
                        &e.to_string()
                    );
                    return None;
                }
            };
            Some((alias.clone(), Arc::from(secret)))
        })
        .collect();

    // Gmail Push channel (if configured and referenced by an enabled agent)
    #[cfg(feature = "channel-email")]
    let gmail_push_channel: Option<Arc<GmailPushChannel>> = {
        let active: std::collections::HashSet<String> = config
            .agents
            .values()
            .filter(|a| a.enabled)
            .flat_map(|a| a.channels.iter().map(|c| c.as_str().to_string()))
            .collect();
        config
            .channels
            .gmail_push
            .iter()
            .find(|(alias, _)| active.contains(&format!("gmail_push.{alias}")))
            .map(|(alias, gp)| {
                let alias = alias.clone();
                let peer_resolver: Arc<dyn Fn() -> Vec<String> + Send + Sync> = {
                    let cfg_arc = config_state.clone();
                    let alias = alias.clone();
                    Arc::new(move || cfg_arc.read().channel_external_peers("gmail_push", &alias))
                };
                Arc::new(GmailPushChannel::new(gp.clone(), alias, peer_resolver))
            })
    };

    let session_backend: Option<Arc<dyn SessionBackend>> = if config.gateway.session_persistence {
        match clawcrew_infra::make_session_backend(
            &config.data_dir,
            &config.channels.session_backend,
        ) {
            Ok(backend) => {
                ::clawcrew_log::record!(
                    INFO,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                    &format!(
                        "Gateway session persistence enabled (backend={})",
                        config.channels.session_backend
                    )
                );
                if config.gateway.session_ttl_hours > 0
                    && let Ok(cleaned) = backend.cleanup_stale(config.gateway.session_ttl_hours)
                    && cleaned > 0
                {
                    ::clawcrew_log::record!(
                        INFO,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_attrs(::serde_json::json!({"cleaned": cleaned})),
                        "Cleaned up stale gateway sessions"
                    );
                }
                Some(backend)
            }
            Err(e) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
                    "Session persistence disabled"
                );
                None
            }
        }
    } else {
        None
    };

    // ── Pairing guard ──────────────────────────────────────
    // The pairing-code policy is resolved from config here and nowhere
    // else: startup pairing, `gateway get-paircode --new`, the dashboard
    // pairing flow, and rotate-device all issue through this guard.
    let pairing = Arc::new(PairingGuard::new(
        config.gateway.require_pairing,
        &config.gateway.paired_tokens,
        config.gateway.pairing_code,
    ));
    let rate_limit_max_keys = normalize_max_keys(
        config.gateway.rate_limit_max_keys,
        RATE_LIMIT_MAX_KEYS_DEFAULT,
    );
    let rate_limiter = Arc::new(GatewayRateLimiter::new(
        config.gateway.pair_rate_limit_per_minute,
        config.gateway.webhook_rate_limit_per_minute,
        rate_limit_max_keys,
    ));
    let idempotency_max_keys = normalize_max_keys(
        config.gateway.idempotency_max_keys,
        IDEMPOTENCY_MAX_KEYS_DEFAULT,
    );
    let idempotency_store = Arc::new(IdempotencyStore::new(
        Duration::from_secs(config.gateway.idempotency_ttl_secs.max(1)),
        idempotency_max_keys,
    ));

    // Resolve optional path prefix for reverse-proxy deployments.
    let path_prefix: Option<&str> = config
        .gateway
        .path_prefix
        .as_deref()
        .filter(|p| !p.is_empty());

    // ── Tunnel ────────────────────────────────────────────────
    let tunnel = match clawcrew_runtime::tunnel::create_tunnel(&config.tunnel) {
        Ok(t) => t,
        Err(e) => {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({
                        "tunnel_provider": config.tunnel.tunnel_provider,
                        "error": format!("{e}"),
                    })),
                "Gateway: tunnel adapter failed to construct; booting without a \
                 tunnel. Fix [tunnel] and POST /admin/reload."
            );
            None
        }
    };
    let mut tunnel_url: Option<String> = None;

    if let Some(ref tun) = tunnel {
        println!("🔗 Starting {} tunnel...", tun.name());
        match tun.start(host, actual_port).await {
            Ok(url) => {
                println!("🌐 Tunnel active: {url}");
                tunnel_url = Some(url);
            }
            Err(e) => {
                println!("⚠️  Tunnel failed to start: {e}");
                println!("   Falling back to local-only mode.");
            }
        }
    }

    let web_dist_dir = resolve_web_dist_dir(&config);
    if let Some(stale) = config
        .gateway
        .web_dist_dir
        .as_ref()
        .map(std::path::PathBuf::from)
        && !has_servable_dashboard_index(&stale)
    {
        ::clawcrew_log::record!(
            WARN,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                .with_attrs(::serde_json::json!({"configured": stale.display().to_string()})),
            "gateway.web_dist_dir points at a path without a usable index.html on \
             this machine; falling back to auto-detect. Update or remove the setting in \
             config.toml to silence this warning."
        );
    }

    // Embedded assets take serving priority when compiled in. Otherwise, use
    // the single resolved `web_dist_dir` snapshot stored in AppState.
    let availability: Option<WebDashboardAvailability> = {
        #[cfg(feature = "embedded-web")]
        {
            Some(WebDashboardAvailability::Embedded)
        }
        #[cfg(not(feature = "embedded-web"))]
        {
            web_dist_dir
                .clone()
                .map(WebDashboardAvailability::Filesystem)
        }
    };

    match availability {
        Some(WebDashboardAvailability::Embedded) => {
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                "Web dashboard: serving embedded assets"
            );
        }
        Some(WebDashboardAvailability::Filesystem(ref dir)) => {
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_attrs(::serde_json::json!({"path": dir.display().to_string()})),
                "Web dashboard: serving filesystem assets"
            );
        }
        None if config.gateway.web_dist_dir.is_some() => {
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                "Web dashboard: not available — configured gateway.web_dist_dir is missing on \
                 this machine and no fallback location was found. Reinstall with the supported \
                 installer (`./install.sh --source` on Linux/macOS, `setup.bat` on Windows) to \
                 build and place the dashboard where the gateway looks for it."
            );
        }
        None => {
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                "Web dashboard: not available — no web/dist found. Reinstall with the supported \
                 installer (`./install.sh --source` on Linux/macOS, `setup.bat` on Windows) to \
                 build and place the dashboard where the gateway looks for it."
            );
        }
    }

    let pfx = path_prefix.unwrap_or("");
    println!("🦀 ClawCrew Gateway listening on http://{display_addr}{pfx}");
    if let Some(ref url) = tunnel_url {
        println!("  🌐 Public URL: {url}");
    }
    if availability.is_some() {
        println!("  🌐 Web Dashboard: http://{display_addr}{pfx}/");
    } else {
        println!(
            "  ⚠️  Web Dashboard: not available — reinstall with the supported installer \
             (`./install.sh --source` on Linux/macOS, `setup.bat` on Windows) to build it"
        );
    }
    if let Some(code) = pairing.pairing_code() {
        // The box is sized from the code, not from a literal: since the policy became config-driven,
        // the code length is operator-configurable (6..=128 chars).
        let rule = "─".repeat(code.chars().count() + 4);
        println!();
        println!("  🔐 PAIRING REQUIRED — use this one-time code:");
        println!("     ┌{rule}┐");
        println!("     │  {code}  │");
        println!("     └{rule}┘");
        println!("     Send: POST {pfx}/pair with header X-Pairing-Code: {code}");
    } else if pairing.require_pairing() {
        for line in already_paired_pairing_notice(host, actual_port, pfx) {
            println!("{line}");
        }
        println!();
    } else {
        println!("  ⚠️  Pairing: DISABLED (all requests accepted)");
        println!();
    }
    println!("  POST {pfx}/pair      — pair a new client (X-Pairing-Code header)");
    println!("  POST {pfx}/webhook   — {{\"message\": \"your prompt\"}}");
    #[cfg(feature = "channel-whatsapp-cloud")]
    if !whatsapp_channel.is_empty() {
        println!("  GET  {pfx}/whatsapp[/<alias>]  — Meta webhook verification");
        println!("  POST {pfx}/whatsapp[/<alias>]  — WhatsApp message webhook");
    }
    #[cfg(feature = "channel-linq")]
    if !linq_channels.is_empty() {
        println!("  POST {pfx}/linq[/<alias>]      — Linq message webhook (iMessage/RCS/SMS)");
    }
    #[cfg(feature = "channel-nextcloud")]
    if !nextcloud_talk_channel.is_empty() {
        println!("  POST {pfx}/nextcloud-talk[/<alias>] — Nextcloud Talk bot webhook");
    }
    println!("  GET  {pfx}/api/*     — REST API (bearer token required)");
    println!("  GET  {pfx}/ws/chat   — WebSocket agent chat");
    if config.nodes.enabled {
        println!("  GET  {pfx}/ws/nodes  — WebSocket node discovery");
    }
    println!("  GET  {pfx}/health    — health check");
    println!("  GET  {pfx}/metrics   — Prometheus metrics");
    println!("  Press Ctrl+C to stop.\n");

    clawcrew_runtime::health::mark_component_ok("gateway");

    // Fire gateway start hook
    if let Some(ref hooks) = hooks {
        hooks.fire_gateway_start(host, actual_port).await;
    }

    let broadcast_layer: Arc<dyn clawcrew_runtime::observability::Observer> = Arc::new(
        sse::BroadcastObserver::new(event_tx.clone(), event_buffer.clone()),
    );
    let broadcast_hook_guard =
        clawcrew_runtime::observability::set_scoped_broadcast_hook(broadcast_layer);

    clawcrew_log::set_broadcast_hook(event_tx.clone());

    // Bound into AppState. Not a broadcaster — the broadcaster is the
    // `broadcast_layer` installed above as the global hook. This is the
    // configured backend (Log/Prometheus/...) wrapped by `TeeObserver`,
    // which tees events into the hook on every record.
    let state_observer: Arc<dyn clawcrew_runtime::observability::Observer> = Arc::from(
        clawcrew_runtime::observability::create_observer(&config.observability),
    );

    let (owned_shutdown_tx, _) = tokio::sync::watch::channel(false);
    let (shutdown_tx, reload_tx) = reload_controls
        .map(|controls| (controls.shutdown_tx, Some(controls.reload_tx)))
        .unwrap_or((owned_shutdown_tx, None));
    let mut shutdown_rx = shutdown_tx.subscribe();

    // Node registry for dynamic node discovery
    let node_registry = Arc::new(nodes::NodeRegistry::new(config.nodes.max_nodes));
    let mdns_config_state = Arc::clone(&config_state);
    let mdns_peer_registry =
        nodes::mdns::MdnsPeerRegistry::new(move || mdns_config_state.read().nodes.mdns.max_peers);
    let mdns_task = if config.nodes.mdns.enabled
        && nodes::mdns::is_advertisable_gateway_addr(&actual_addr)
    {
        let mdns_config = config.nodes.mdns.clone();
        let advertised_gateway = nodes::mdns::MdnsAdvertisedGateway::new(actual_port, path_prefix);
        let mdns_registry = mdns_peer_registry.clone();
        let mdns_shutdown_rx = shutdown_tx.subscribe();
        Some(clawcrew_spawn::spawn!(async move {
            if let Err(err) = nodes::mdns::run_peer_discovery(
                mdns_config,
                advertised_gateway,
                mdns_registry,
                mdns_shutdown_rx,
            )
            .await
            {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"error": format!("{err}")})),
                    "mDNS local peer discovery stopped"
                );
            }
        }))
    } else if config.nodes.mdns.enabled {
        ::clawcrew_log::record!(
            WARN,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                .with_outcome(::clawcrew_log::EventOutcome::Failure)
                .with_attrs(::serde_json::json!({"bind_addr": actual_addr.to_string()})),
            "mDNS local peer discovery skipped because the gateway is bound to a loopback-only host"
        );
        None
    } else {
        None
    };

    // Device registry and pairing store (only when pairing is required)
    let device_registry = if config.gateway.require_pairing {
        let registry = Arc::new(api_pairing::DeviceRegistry::new(&config.data_dir));
        // Reconcile the registry against the canonical paired-token set so that
        // tokens paired via the legacy `/pair` route (and any other historical
        // orphans) become visible and revocable in the management UI. The token
        // set itself stays owned by `PairingGuard`/`gateway.paired_tokens`.
        match registry.reconcile_from_token_hashes(&pairing.tokens()) {
            Ok(0) => {}
            Ok(n) => ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_attrs(::serde_json::json!({ "backfilled": n })),
                "backfilled legacy paired token(s) into the device registry"
            ),
            Err(e) => ::clawcrew_log::record!(
                ERROR,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({ "error": format!("{e}") })),
                "device registry backfill from paired_tokens failed"
            ),
        }
        Some(registry)
    } else {
        None
    };
    let pending_pairings = if config.gateway.require_pairing {
        Some(Arc::new(api_pairing::PairingStore::new()))
    } else {
        None
    };

    let state = AppState {
        config: config_state,
        config_write_lock: clawcrew_config::write_lock::shared_config_write_lock(),
        model_provider,
        model,
        temperature,
        mem,
        memory_strategy,
        auto_save: config.memory.auto_save,
        pairing,
        trust_forwarded_headers: config.gateway.trust_forwarded_headers,
        rate_limiter,
        auth_limiter: Arc::new(auth_rate_limit::AuthRateLimiter::new()),
        idempotency_store,
        #[cfg(feature = "channel-whatsapp-cloud")]
        whatsapp: whatsapp_channel,
        #[cfg(feature = "channel-whatsapp-cloud")]
        whatsapp_app_secret,
        #[cfg(feature = "channel-linq")]
        linq: linq_channels,
        #[cfg(feature = "channel-linq")]
        linq_signing_secrets,
        #[cfg(feature = "channel-nextcloud")]
        nextcloud_talk: nextcloud_talk_channel,
        #[cfg(feature = "channel-nextcloud")]
        nextcloud_talk_webhook_secret,
        #[cfg(feature = "channel-email")]
        gmail_push: gmail_push_channel,
        observer: state_observer,
        tools_registry,
        tools_registry_by_agent,
        cost_tracker,
        event_tx,
        event_buffer,
        shutdown_tx,
        reload_tx,
        node_registry,
        mdns_peer_registry,
        session_backend,
        session_queue: Arc::new(session_queue::SessionActorQueue::new(8, 30, 600)),
        device_registry,
        pending_pairings,
        path_prefix: path_prefix.unwrap_or("").to_string(),
        web_dist_dir,
        canvas_store,
        cancel_tokens: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        pending_reload: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        tui_registry,
        sop_engine,
        sop_audit,
        #[cfg(feature = "webauthn")]
        webauthn: if config.security.webauthn.enabled {
            let secret_store = Arc::new(clawcrew_runtime::security::SecretStore::new(
                &config.data_dir,
                true,
            ));
            let wa_config = clawcrew_runtime::security::webauthn::WebAuthnConfig {
                enabled: true,
                rp_id: config.security.webauthn.rp_id.clone(),
                rp_origin: config.security.webauthn.rp_origin.clone(),
                rp_name: config.security.webauthn.rp_name.clone(),
            };
            Some(Arc::new(api_webauthn::WebAuthnState {
                manager: clawcrew_runtime::security::webauthn::WebAuthnManager::new(
                    wa_config,
                    secret_store,
                    &config.data_dir,
                ),
                pending_registrations: parking_lot::Mutex::new(std::collections::HashMap::new()),
                pending_authentications: parking_lot::Mutex::new(std::collections::HashMap::new()),
            }))
        } else {
            None
        },
    };

    // Build router with middleware
    let inner = Router::new()
        // ── Admin routes (for CLI management) ──
        .route("/admin/shutdown", post(handle_admin_shutdown))
        .route("/admin/reload", post(handle_admin_reload))
        .route("/admin/sop/pending", get(api_sop::handle_sop_pending))
        .route("/admin/sop/approve", post(api_sop::handle_sop_approve))
        .route("/admin/sop/deny", post(api_sop::handle_sop_deny))
        .route("/admin/paircode", get(handle_admin_paircode))
        .route("/admin/paircode/new", post(handle_admin_paircode_new))
        // ── Existing routes ──
        .route("/health", get(handle_health))
        .route("/metrics", get(handle_metrics))
        .route("/pair", post(handle_pair))
        .route("/pair/code", get(handle_pair_code))
        .route("/webhook", post(handle_webhook))
        .merge(sop_webhook_routes())
        .merge(optional_channel_routes())
        // ── Claude Code runner hooks ──
        .route("/hooks/claude-code", post(api::handle_claude_code_hook))
        // ── Web Dashboard API routes ──
        .route("/api/status", get(api::handle_api_status))
        .route("/api/version/check", get(version::handle_version_check))
        .route("/api/version/upgrade", post(version::handle_version_upgrade))
        .route(
            "/api/version/upgrade/status",
            get(version::handle_version_upgrade_status),
        )
        .route("/api/logs", get(api_logs::handle_api_logs))
        .route("/api/dashboard/tasks", get(api_tasks::handle_tasks))
        .route("/api/dashboard/tasks/stats", get(api_tasks::handle_task_stats))
        .route("/api/dashboard/tasks/{task_id}", get(api_tasks::handle_task))
        .route("/api/dashboard/tasks/{task_id}/children", get(api_tasks::handle_task_children))
        .route("/api/dashboard/tasks/{task_id}/tree", get(api_tasks::handle_task_tree))
        .route("/api/dashboard/tasks/{task_id}/timeline", get(api_tasks::handle_task_timeline))
        .route("/api/dashboard/tasks/{task_id}/cancel", post(api_tasks::handle_task_cancel))
        .route("/api/dashboard/tasks/{task_id}/reopen", post(api_tasks::handle_task_reopen))
        .route("/api/dashboard/tasks/{task_id}/events", get(api_tasks::handle_task_events))
        .route("/api/dashboard/tasks/{task_id}/stream", get(api_tasks::handle_task_events_stream))
        .route(
            "/api/config",
            get(api_config::handle_config_get)
                .patch(api_config::handle_patch)
                .options(api_config::handle_options_config),
        )
        .route(
            "/api/config/prop",
            get(api_config::handle_prop_get)
                .put(api_config::handle_prop_put)
                .delete(api_config::handle_prop_delete)
                .options(api_config::handle_options_prop),
        )
        .route("/api/config/list", get(api_config::handle_list))
        .route(
            "/api/sops",
            get(api_sop_author::handle_sops_list).post(api_sop_author::handle_sop_create),
        )
        .route(
            "/api/sops/{name}",
            put(api_sop_author::handle_sop_save).delete(api_sop_author::handle_sop_delete),
        )
        .route(
            "/api/sops/{name}/graph",
            get(api_sop_author::handle_sop_graph),
        )
        .route(
            "/api/sops/{name}/run",
            post(api_sop_author::handle_sop_run),
        )
        .route("/api/sops/runs", get(api_sop_author::handle_sop_runs))
        .route(
            "/api/sops/{name}/full",
            get(api_sop_author::handle_sop_full),
        )
        .route(
            "/api/sops/wire-draft",
            post(api_sop_author::handle_sop_wire_draft),
        )
        .route(
            "/api/sops/graph-draft",
            post(api_sop_author::handle_sop_graph_draft),
        )
        .route(
            "/api/sops/trigger-sources",
            get(api_sop_author::handle_sop_trigger_sources),
        )
        .route(
            "/api/sops/graph-legend",
            get(api_sop_author::handle_sop_graph_legend),
        )
        .route(
            "/api/tools/param-options",
            post(api_sop_author::handle_tools_param_options),
        )
        .route(
            "/api/sops/{name}/runs/{run_id}/overlay",
            get(api_sop_author::handle_sop_run_overlay),
        )
        .route(
            "/api/sops/{name}/runs/{run_id}/decide",
            post(api_sop_author::handle_sop_decide),
        )
        .route(
            "/api/sops/{name}/runs/{run_id}/cancel",
            post(api_sop_author::handle_sop_cancel),
        )
        .route("/api/config/drift", get(api_config::handle_drift))
        .route(
            "/api/config/reload-status",
            get(api_config::handle_reload_status),
        )
        .route("/api/config/templates", get(api_config::handle_templates))
        .route("/api/config/map-keys", get(api_config::handle_get_map_keys))
        .route(
            "/api/config/resolve-alias-source",
            get(api_config::handle_resolve_alias_source),
        )
        .route(
            "/api/config/map-key",
            post(api_config::handle_map_key).delete(api_config::handle_delete_map_key),
        )
        .route("/api/config/rename-map-key", post(api_config::handle_rename_map_key))
        .route(
            "/api/config/model-providers/{type}/{alias}/refresh-context-window",
            post(api_config::handle_refresh_context_window),
        )
        .route("/api/config/delete-plan", get(api_config::handle_delete_plan))
        .route("/api/config/catalog", get(api_sections::handle_catalog))
        .route(
            "/api/config/catalog/models",
            get(api_sections::handle_catalog_models),
        )
        .route("/api/config/status", get(api_sections::handle_section_status))
        .route(
            "/api/config/agent-options",
            get(api_sections::handle_agent_options),
        )
        .route("/api/config/sections", get(api_sections::handle_sections))
        .route(
            "/api/config/sections/{section}",
            get(api_sections::handle_section_picker),
        )
        .route(
            "/api/config/sections/{section}/items/{key}",
            post(api_sections::handle_section_select),
        )
        .route("/api/personality", get(api_personality::handle_index))
        .route(
            "/api/quickstart/state",
            get(api_quickstart::handle_state),
        )
        .route(
            "/api/quickstart/fields",
            post(api_quickstart::handle_fields),
        )
        .route(
            "/api/quickstart/validate",
            post(api_quickstart::handle_validate),
        )
        .route(
            "/api/quickstart/apply",
            post(api_quickstart::handle_apply),
        )
        .route(
            "/api/quickstart/dismiss",
            post(api_quickstart::handle_dismiss),
        )
        .route(
            "/api/personality/templates",
            get(api_personality::handle_templates),
        )
        .route(
            "/api/personality/{filename}",
            get(api_personality::handle_get).put(api_personality::handle_put),
        )
        .route("/api/browse", get(api_browse::handle_browse))
        .route("/api/browse/mkdir", post(api_browse::handle_browse_mkdir))
        .route("/api/browse/rmdir", delete(api_browse::handle_browse_rmdir))
        .route(
            "/api/agents/{alias}/workspace/list",
            get(api_browse::handle_agent_workspace_list),
        )
        .route(
            "/api/agents/{alias}/workspace/read",
            get(api_browse::handle_agent_workspace_read),
        )
        .route(
            "/api/agents/{alias}/workspace/path",
            delete(api_browse::handle_agent_workspace_delete),
        )
        .route(
            "/api/agents/{alias}/workspace/move",
            post(api_browse::handle_agent_workspace_move),
        )
        .route(
            "/api/agents/{alias}/workspace/mkdir",
            post(api_browse::handle_agent_workspace_mkdir),
        )
        .route(
            "/api/agents/{alias}/skills",
            get(api_skills::handle_agent_skills),
        )
        .route("/api/skills/bundles", get(api_skills::handle_list_bundles))
        .route(
            "/api/skills/slash-option-kinds",
            get(api_skills::handle_slash_option_kinds),
        )
        .route(
            "/api/skills/bundles/{alias}/skills",
            get(api_skills::handle_list_skills).post(api_skills::handle_create_skill),
        )
        .route(
            "/api/skills/bundles/{alias}/skills/{name}",
            get(api_skills::handle_read_skill)
                .put(api_skills::handle_write_skill)
                .delete(api_skills::handle_delete_skill),
        )
        .route("/api/config/init", post(api_config::handle_init))
        .route("/api/config/migrate", post(api_config::handle_migrate))
        .route("/api/openapi.json", get(openapi::handle_openapi_json))
        .route("/api/docs", get(openapi::handle_docs))
        .route("/api/tools", get(api::handle_api_tools))
        .route("/api/cron", get(api::handle_api_cron_list))
        .route("/api/cron", post(api::handle_api_cron_add))
        .route(
            "/api/cron/settings",
            get(api::handle_api_cron_settings_get).patch(api::handle_api_cron_settings_patch),
        )
        .route(
            "/api/cron/{id}",
            delete(api::handle_api_cron_delete).patch(api::handle_api_cron_patch),
        )
        .route("/api/cron/{id}/runs", get(api::handle_api_cron_runs))
        // Note: `/api/cron/{id}/run` is registered on a separate router below
        // with a longer TimeoutLayer — manual cron triggers run the job
        // synchronously and routinely exceed the 30s gateway-wide default.
        .route("/api/integrations", get(api::handle_api_integrations))
        .route(
            "/api/integrations/settings",
            get(api::handle_api_integrations_settings),
        )
        .route(
            "/api/doctor",
            get(api::handle_api_doctor).post(api::handle_api_doctor),
        )
        .route("/api/memory", get(api::handle_api_memory_list))
        .route("/api/memory", post(api::handle_api_memory_store))
        .route("/api/memory/{key}", delete(api::handle_api_memory_delete))
        .route("/api/cost", get(api::handle_api_cost))
        .route("/api/cli-tools", get(api::handle_api_cli_tools))
        .route("/api/channels", get(api::handle_api_channels))
        .route(
            "/api/channels/bind",
            post(api_config::handle_api_channel_bind),
        )
        .route(
            "/api/channels/{channel}/relink",
            post(api::handle_api_channel_relink),
        )
        .route("/api/health", get(api::handle_api_health))
        .route(
            "/api/providers/health",
            get(api_providers::handle_providers_health),
        )
        .route(
            "/api/providers/capabilities.md",
            get(api_providers::handle_provider_capabilities),
        )
        .route("/api/audit", get(api_audit::handle_api_audit))
        .route("/api/backup/schema-versions", get(api_backup::handle_schema_versions))
        .route("/api/backup/create", post(api_backup::handle_create_backup))
        .route("/api/backup/plan-restore", post(api_backup::handle_plan_restore))
        .route("/api/backup/restore", post(api_backup::handle_restore))
        .route("/api/tuis", get(api::handle_api_tuis))
        .route("/api/sessions", get(api::handle_api_sessions_list))
        .route("/api/sessions/running", get(api::handle_api_sessions_running))
        .route("/api/sessions/stream", get(api::handle_api_sessions_stream))
        .route(
            "/api/sessions/{id}/messages",
            get(api::handle_api_session_messages).post(api::handle_api_session_message_post),
        )
        .route("/api/sessions/{id}", delete(api::handle_api_session_delete).put(api::handle_api_session_rename))
        .route("/api/sessions/{id}/state", get(api::handle_api_session_state))
        .route("/api/sessions/{id}/abort", post(api::handle_api_session_abort))
        // ── Pairing + Device management API ──
        .route("/api/pairing/initiate", post(api_pairing::initiate_pairing))
        .route("/api/pair", post(api_pairing::submit_pairing_enhanced))
        .route("/api/devices", get(api_pairing::list_devices))
        .route(
            "/api/devices/me/capabilities",
            post(api_pairing::update_my_capabilities),
        )
        .route("/api/devices/{id}", delete(api_pairing::revoke_device))
        .route(
            "/api/devices/{id}/token/rotate",
            post(api_pairing::rotate_token),
        )
        // ── Live Canvas (A2UI) routes ──
        .route("/api/canvas", get(canvas::handle_canvas_list))
        .route(
            "/api/canvas/{id}",
            get(canvas::handle_canvas_get)
                .post(canvas::handle_canvas_post)
                .delete(canvas::handle_canvas_clear),
        )
        .route(
            "/api/canvas/{id}/history",
            get(canvas::handle_canvas_history),
        );

    #[cfg(feature = "plugins-wasm")]
    let inner = inner.merge(plugin_webhook::routes(plugin_webhooks));
    #[cfg(not(feature = "plugins-wasm"))]
    let _ = plugin_webhooks;

    #[cfg(feature = "a2a")]
    let inner = inner.merge(a2a::a2a_routes_with_endpoint(Some(
        a2a::AdvertisedGatewayEndpoint::new(host, actual_port),
    )));

    // ── WebAuthn hardware key authentication API (requires webauthn feature) ──
    #[cfg(feature = "webauthn")]
    let inner = inner
        .route(
            "/api/webauthn/register/start",
            post(api_webauthn::handle_register_start),
        )
        .route(
            "/api/webauthn/register/finish",
            post(api_webauthn::handle_register_finish),
        )
        .route(
            "/api/webauthn/auth/start",
            post(api_webauthn::handle_auth_start),
        )
        .route(
            "/api/webauthn/auth/finish",
            post(api_webauthn::handle_auth_finish),
        )
        .route(
            "/api/webauthn/credentials",
            get(api_webauthn::handle_list_credentials),
        )
        .route(
            "/api/webauthn/credentials/{id}",
            delete(api_webauthn::handle_delete_credential),
        );

    // ── Plugin management API (requires plugins-wasm feature) ──
    #[cfg(feature = "plugins-wasm")]
    let inner = inner.route(
        "/api/plugins",
        get(api_plugins::plugin_routes::list_plugins),
    );

    let inner = inner
        // ── SSE event stream ──
        .route("/api/events", get(sse::handle_sse_events))
        .route("/api/events/history", get(sse::handle_events_history))
        // ── ACP client bridge ──
        .route("/acp", get(acp::handle_ws_acp))
        // ── WebSocket agent chat ──
        .route("/ws/chat", get(ws::handle_ws_chat))
        // ── WebSocket SOP runs feed ──
        .route("/ws/sops/runs", get(ws_sop_runs::handle_ws_sop_runs))
        // ── WebSocket canvas updates ──
        .route("/ws/canvas/{id}", get(canvas::handle_ws_canvas))
        // ── WebSocket node discovery ──
        .route("/ws/nodes", get(nodes::handle_ws_nodes))
        // ── Static assets (web dashboard) ──
        .merge(static_file_routes())
        // ── SPA fallback: non-API GET requests serve index.html ──
        .fallback(get(static_files::handle_spa_fallback))
        .with_state(state.clone())
        .layer(RequestBodyLimitLayer::new(MAX_BODY_SIZE))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(gateway_request_timeout_secs(&config.gateway)),
        ));

    // The dashboard image upload lives on its own sub-router so it can opt out
    // of the 64 KB gateway-wide RequestBodyLimitLayer, which is sized for JSON
    // control-plane bodies and would otherwise reject any real image before the
    // route's own ceiling runs. The route keeps the extractor-level
    // DefaultBodyLimit at the same ceiling; the per-request size check against
    // live `multimodal.max_image_size_mb` happens inside the handler.
    let upload_router: Router = Router::new()
        .route(
            "/api/upload",
            post(api_upload::handle_upload).layer(axum::extract::DefaultBodyLimit::max(
                api_upload::UPLOAD_BODY_CEILING_BYTES,
            )),
        )
        .with_state(state.clone())
        .layer(RequestBodyLimitLayer::new(
            api_upload::UPLOAD_BODY_CEILING_BYTES,
        ))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(gateway_request_timeout_secs(&config.gateway)),
        ));
    let inner = inner.merge(upload_router);

    // Manual cron-trigger and A2A task routes live on their own sub-router so
    // they can opt out of the 30s gateway-wide TimeoutLayer. Both run a
    // synchronous agent turn inline. Layers attached here travel with the
    // route through `merge`, so only these endpoints see the longer timeout.
    let long_running_router: Router<AppState> =
        Router::new().route("/api/cron/{id}/run", post(api::handle_api_cron_run));
    #[cfg(feature = "a2a")]
    let long_running_router = long_running_router.merge(a2a::a2a_task_route());
    let long_running_router: Router = long_running_router
        .with_state(state)
        .layer(RequestBodyLimitLayer::new(MAX_BODY_SIZE))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(gateway_long_running_request_timeout_secs(&config.gateway)),
        ));

    let inner = inner.merge(long_running_router);

    // Nest under path prefix when configured (axum strips prefix before routing).
    // nest() at "/prefix" handles both "/prefix" and "/prefix/*" but not "/prefix/"
    // with a trailing slash, so we add a fallback redirect for that case.
    let app = if let Some(prefix) = path_prefix {
        let redirect_target = prefix.to_string();
        Router::new().nest(prefix, inner).route(
            &format!("{prefix}/"),
            get(|| async move { axum::response::Redirect::permanent(&redirect_target) }),
        )
    } else {
        inner
    };

    let tls_enabled = config
        .gateway
        .tls
        .as_ref()
        .is_some_and(|tls_cfg| tls_cfg.enabled);
    let app = if tls_enabled {
        app.layer(axum::middleware::from_fn(security_headers::apply_with_hsts))
    } else {
        app.layer(axum::middleware::from_fn(security_headers::apply))
    };

    // ── TLS / mTLS setup ───────────────────────────────────────────
    let tls_acceptor = match &config.gateway.tls {
        Some(tls_cfg) if tls_cfg.enabled => {
            let has_mtls = tls_cfg.client_auth.as_ref().is_some_and(|ca| ca.enabled);
            if has_mtls {
                ::clawcrew_log::record!(
                    INFO,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                    "TLS enabled with mutual TLS (mTLS) client verification"
                );
            } else {
                ::clawcrew_log::record!(
                    INFO,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                    "TLS enabled (no client certificate requirement)"
                );
            }
            Some(tls::build_tls_acceptor(tls_cfg)?)
        }
        _ => None,
    };

    if let Some(readiness) = readiness {
        readiness.report_ready(actual_addr);
    }

    if let Some(tls_acceptor) = tls_acceptor {
        // Manual TLS accept loop — serves each connection via hyper.
        let app = app.into_make_service_with_connect_info::<SocketAddr>();
        let mut app = app;

        let mut shutdown_signal = shutdown_rx;
        loop {
            tokio::select! {
                conn = listener.accept() => {
                    let (tcp_stream, remote_addr) = match conn {
                        Ok(pair) => pair,
                        Err(e) => {
                            if is_recoverable_accept_error(&e) {
                                // Transient (e.g. EMFILE under fd pressure):
                                // the listener is still valid. Back off
                                // briefly to avoid hot-spinning, then keep
                                // serving rather than killing the daemon
                                ::clawcrew_log::record!(WARN, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_outcome(::clawcrew_log::EventOutcome::Unknown).with_attrs(::serde_json::json!({"error": format!("{}", e)})), "gateway accept() failed with a transient error; backing off and continuing");
                                tokio::time::sleep(Duration::from_millis(ACCEPT_ERROR_BACKOFF_MS)).await;
                                continue;
                            }
                            return Err(e.into());
                        }
                    };
                    let tls_acceptor = tls_acceptor.clone();
                    let svc = tower::MakeService::<
                        SocketAddr,
                        hyper::Request<hyper::body::Incoming>,
                    >::make_service(&mut app, remote_addr)
                    .await
                    .expect("infallible make_service");

                    clawcrew_spawn::spawn!(async move {
                        let tls_stream = match tls_acceptor.accept(tcp_stream).await {
                            Ok(s) => s,
                            Err(e) => {
                                ::clawcrew_log::record!(DEBUG, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_attrs(::serde_json::json!({"error": format!("{}", e), "remote_addr": remote_addr})), "TLS handshake failed from");
                                return;
                            }
                        };
                        let io = hyper_util::rt::TokioIo::new(tls_stream);
                        let hyper_svc = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                            let mut svc = svc.clone();
                            async move {
                                tower::Service::call(&mut svc, req).await
                            }
                        });
                        if let Err(e) = hyper_util::server::conn::auto::Builder::new(
                            hyper_util::rt::TokioExecutor::new(),
                        )
                        .serve_connection(io, hyper_svc)
                        .await
                        {
                            ::clawcrew_log::record!(DEBUG, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_attrs(::serde_json::json!({"error": format!("{}", e), "remote_addr": remote_addr})), "connection error from");
                        }
                    });
                }
                _ = shutdown_signal.changed() => {
                    ::clawcrew_log::record!(INFO, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note), "ClawCrew Gateway shutting down");
                    break;
                }
            }
        }
    } else {
        // Plain TCP — use axum's built-in serve.
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.changed().await;
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                "ClawCrew Gateway shutting down"
            );
        })
        .await?;
    }

    if let Some(task) = mdns_task {
        let mut task = task;
        tokio::select! {
            result = &mut task => {
                if let Err(err) = result {
                    ::clawcrew_log::record!(
                        DEBUG,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({"error": format!("{err}")})),
                        "LAN peer discovery task join failed"
                    );
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(2)) => {
                task.abort();
            }
        }
    }

    drop(broadcast_hook_guard);
    Ok(())
}

fn static_file_routes() -> Router<AppState> {
    Router::new()
        .route("/_app/", get(static_files::handle_static))
        .route("/_app/{*path}", get(static_files::handle_static))
}

fn format_paircode_recovery_command(_host: &str, port: u16) -> String {
    format!("clawcrew gateway get-paircode --new --port {port}")
}

fn already_paired_pairing_notice(host: &str, port: u16, path_prefix: &str) -> Vec<String> {
    vec![
        "  🔒 Pairing: ACTIVE — this gateway is already paired, so no new \
         one-time code was generated on this start."
            .to_string(),
        format!(
            "     To pair another device, run: {}",
            format_paircode_recovery_command(host, port)
        ),
        format!(
            "     Fallback (localhost only): {}",
            format_paircode_recovery_curl(host, port, path_prefix)
        ),
    ]
}

fn format_paircode_recovery_curl(host: &str, port: u16, path_prefix: &str) -> String {
    // Admin paircode routes are localhost-only, so the curl fallback must point
    // at loopback. Bind-only hosts and non-loopback advertised hosts are
    // normalized to `127.0.0.1`; explicit loopback hosts are preserved.
    let recovery_host = paircode_recovery_curl_host(host);
    format!("curl -s -X POST http://{recovery_host}:{port}{path_prefix}/admin/paircode/new")
}

fn paircode_recovery_curl_host(host: &str) -> &str {
    match host {
        "127.0.0.1" | "localhost" => host,
        "::1" => "[::1]",
        _ => "127.0.0.1",
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// AXUM HANDLERS
// ══════════════════════════════════════════════════════════════════════════════

fn public_health_snapshot() -> serde_json::Value {
    let snapshot = clawcrew_runtime::health::snapshot();
    let components = snapshot
        .components
        .into_iter()
        .map(|(name, component)| {
            (
                name,
                serde_json::json!({
                    "status": component.status,
                    "updated_at": component.updated_at,
                    "last_ok": component.last_ok,
                    "restart_count": component.restart_count,
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();

    serde_json::json!({
        "pid": snapshot.pid,
        "updated_at": snapshot.updated_at,
        "uptime_seconds": snapshot.uptime_seconds,
        "components": components,
    })
}

/// GET /health — always public (no secrets leaked)
async fn handle_health(State(state): State<AppState>) -> impl IntoResponse {
    let body = serde_json::json!({
        "status": "ok",
        "paired": state.pairing.is_paired(),
        "require_pairing": state.pairing.require_pairing(),
        "runtime": public_health_snapshot(),
    });
    Json(body)
}

/// Prometheus content type for text exposition format.
const PROMETHEUS_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

fn prometheus_disabled_hint() -> String {
    String::from(
        "# Prometheus backend not enabled. Set [observability] backend = \"prometheus\" in config.\n",
    )
}

#[cfg(feature = "observability-prometheus")]
fn prometheus_observer_from_state(
    observer: &dyn clawcrew_runtime::observability::Observer,
) -> Option<&clawcrew_runtime::observability::PrometheusObserver> {
    // `TeeObserver::as_any` returns the primary observer, so a single direct
    // downcast finds the PrometheusObserver whether the state observer is the
    // raw backend or wrapped by the factory tee.
    observer
        .as_any()
        .downcast_ref::<clawcrew_runtime::observability::PrometheusObserver>()
}

/// GET /metrics — Prometheus text exposition format
async fn handle_metrics(State(state): State<AppState>) -> impl IntoResponse {
    let body = {
        #[cfg(feature = "observability-prometheus")]
        {
            if let Some(prom) = prometheus_observer_from_state(state.observer.as_ref()) {
                prom.encode()
            } else {
                prometheus_disabled_hint()
            }
        }
        #[cfg(not(feature = "observability-prometheus"))]
        {
            let _ = &state;
            prometheus_disabled_hint()
        }
    };

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, PROMETHEUS_CONTENT_TYPE)],
        body,
    )
}

/// POST /pair — exchange one-time code for bearer token
#[axum::debug_handler]
async fn handle_pair(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let rate_key =
        client_key_from_request(Some(peer_addr), &headers, state.trust_forwarded_headers);
    if !state.rate_limiter.allow_pair(&rate_key) {
        ::clawcrew_log::record!(
            WARN,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_outcome(::clawcrew_log::EventOutcome::Unknown),
            "/pair rate limit exceeded"
        );
        let err = serde_json::json!({
            "error": "Too many pairing requests. Please retry later.",
            "retry_after": RATE_LIMIT_WINDOW_SECS,
        });
        return (StatusCode::TOO_MANY_REQUESTS, Json(err));
    }

    // ── Auth rate limiting (brute-force protection) ──
    if let Err(e) = state.auth_limiter.check_rate_limit(&rate_key) {
        ::clawcrew_log::record!(
            WARN,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                .with_attrs(::serde_json::json!({"rate_key": rate_key})),
            "pairing auth rate limit exceeded"
        );
        let err = serde_json::json!({
            "error": format!("Too many auth attempts. Try again in {}s.", e.retry_after_secs),
            "retry_after": e.retry_after_secs,
        });
        return (StatusCode::TOO_MANY_REQUESTS, Json(err));
    }

    let code = headers
        .get("X-Pairing-Code")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    match state.pairing.try_pair(code, &rate_key).await {
        Ok(Some(token)) => {
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                "new client paired successfully"
            );
            let token_hash = PairingGuard::token_hash(&token);
            if let Some(ref registry) = state.device_registry {
                if let Err(e) = registry.register(
                    token_hash.clone(),
                    api_pairing::DeviceInfo {
                        id: uuid::Uuid::new_v4().to_string(),
                        name: None,
                        device_type: None,
                        paired_at: chrono::Utc::now(),
                        last_seen: chrono::Utc::now(),
                        ip_address: Some(rate_key.clone()),
                        capabilities: None,
                    },
                ) {
                    ::clawcrew_log::record!(
                        ERROR,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({"error": format!("{e}")})),
                        "device registry insert failed after successful legacy /pair; rolling back in-process token"
                    );
                    state.pairing.revoke_token_hash(&token_hash);
                    let body = serde_json::json!({
                        "paired": false,
                        "persisted": false,
                        "error": format!("Device registry error: {e}"),
                        "message": "Pairing failed; the in-process token was not retained.",
                    });
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(body));
                }
            }
            if let Err(err) = Box::pin(persist_pairing_tokens(
                state.config.clone(),
                &state.pairing,
                state.config_write_lock.clone(),
            ))
            .await
            {
                ::clawcrew_log::record!(
                    ERROR,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"error": format!("{}", err)})),
                    "pairing token persistence failed; rolling back in-process token"
                );
                state.pairing.revoke_token_hash(&token_hash);
                let body = serde_json::json!({
                    "paired": false,
                    "persisted": false,
                    "error": format!("Token persistence error: {err}"),
                    "message": "Pairing failed; the in-process token was not retained.",
                });
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(body));
            }

            let body = serde_json::json!({
                "paired": true,
                "persisted": true,
                "token": token,
                "message": "Save this token — use it as Authorization: Bearer <token>"
            });
            (StatusCode::OK, Json(body))
        }
        Ok(None) => {
            state.auth_limiter.record_attempt(&rate_key);
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                "pairing attempt with invalid code"
            );
            let err = serde_json::json!({"error": "Invalid pairing code"});
            (StatusCode::FORBIDDEN, Json(err))
        }
        Err(lockout_secs) => {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({"lockout_secs": lockout_secs})),
                "pairing locked out; too many failed attempts"
            );
            let err = serde_json::json!({
                "error": format!("Too many failed attempts. Try again in {lockout_secs}s."),
                "retry_after": lockout_secs
            });
            (StatusCode::TOO_MANY_REQUESTS, Json(err))
        }
    }
}

pub(crate) async fn persist_pairing_tokens(
    config: Arc<RwLock<Config>>,
    pairing: &PairingGuard,
    config_write_lock: Arc<tokio::sync::Mutex<()>>,
) -> Result<()> {
    // Self-contained: no caller pre-reads config for modify, so this
    // acquires the witness itself rather than taking it as a param. Held
    // across the whole read-modify-save-swap below.
    let _guard = Arc::clone(&config_write_lock).lock_owned().await;
    debug_assert!(
        config_write_lock.try_lock().is_err(),
        "persist_pairing_tokens must hold config_write_lock across its read-modify-save-swap"
    );
    let paired_tokens = pairing.tokens();
    // This is needed because parking_lot's guard is not Send so we clone the inner
    // this should be removed once async mutexes are used everywhere
    let mut updated_cfg = { config.read().clone() };
    updated_cfg.gateway.paired_tokens = paired_tokens;
    updated_cfg.mark_dirty("gateway.paired_tokens");
    updated_cfg
        .save_dirty()
        .await
        .context("Failed to persist paired tokens to config.toml")?;

    // Keep shared runtime config in sync with persisted tokens.
    *config.write() = updated_cfg;
    Ok(())
}

/// Result of a gateway chat turn.
struct GatewayChatOutcome {
    response: String,
}

struct UnconfiguredModelProvider;

#[async_trait::async_trait]
impl ModelProvider for UnconfiguredModelProvider {
    async fn chat_with_system(
        &self,
        _system_prompt: Option<&str>,
        _message: &str,
        _model: &str,
        _temperature: Option<f64>,
    ) -> anyhow::Result<String> {
        anyhow::bail!(
            "needs_quickstart: gateway booted without a working model_provider. \
             Complete browser quickstart at /quickstart, or fix \
             [providers.models.<type>.<alias>] and POST /admin/reload."
        )
    }
}

impl ::clawcrew_api::attribution::Attributable for UnconfiguredModelProvider {
    fn role(&self) -> ::clawcrew_api::attribution::Role {
        ::clawcrew_api::attribution::Role::Provider(
            ::clawcrew_api::attribution::ProviderKind::Model(
                ::clawcrew_api::attribution::ModelProviderKind::Custom,
            ),
        )
    }
    fn alias(&self) -> &str {
        "unconfigured"
    }
}

fn needs_quickstart_for(model: &str) -> Option<anyhow::Error> {
    if model.trim().is_empty() {
        ::clawcrew_log::record!(
            WARN,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                .with_outcome(::clawcrew_log::EventOutcome::Failure),
            "gateway dispatch refused: no model configured (browser quickstart incomplete)"
        );
        Some(anyhow::Error::msg(
            "needs_quickstart: gateway has no model configured. Complete \
             browser quickstart at /quickstart, or set [providers.models.<type>.<alias>] \
             model = \"...\" before sending messages.",
        ))
    } else {
        None
    }
}

/// True when `e` carries the marker produced by `needs_quickstart_for`.
/// Used by chat-dispatch error paths to map the marker to a 503
/// `needs_quickstart` HTTP response or a more accurate channel-side
/// reply, instead of the generic 500 / "sorry" catch-all.
fn is_needs_quickstart_err(e: &anyhow::Error) -> bool {
    e.to_string().contains("needs_quickstart")
}

fn needs_quickstart_channel_reply() -> String {
    i18n::get_required_cli_string("channel-needs-quickstart-reply")
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct GatewayChatDispatchCapture {
    message: String,
    session_id: Option<String>,
    agent_override: Option<String>,
}

#[cfg(test)]
static GATEWAY_CHAT_DISPATCH_CAPTURES: std::sync::Mutex<Vec<GatewayChatDispatchCapture>> =
    std::sync::Mutex::new(Vec::new());

// Tests that read or clear the shared capture buffer hold this lock so
// parallel webhook tests cannot invalidate one another's evidence.
#[cfg(test)]
static GATEWAY_CHAT_DISPATCH_CAPTURE_TEST_LOCK: tokio::sync::Mutex<()> =
    tokio::sync::Mutex::const_new(());

#[cfg(test)]
async fn lock_gateway_chat_dispatch_capture_for_test() -> tokio::sync::MutexGuard<'static, ()> {
    GATEWAY_CHAT_DISPATCH_CAPTURE_TEST_LOCK.lock().await
}

#[cfg(all(test, feature = "channel-linq"))]
fn clear_gateway_chat_dispatch_captures_for_test() {
    GATEWAY_CHAT_DISPATCH_CAPTURES
        .lock()
        .expect("gateway chat dispatch capture mutex poisoned")
        .clear();
}

#[cfg(test)]
fn gateway_chat_dispatch_captures_for_test() -> Vec<GatewayChatDispatchCapture> {
    GATEWAY_CHAT_DISPATCH_CAPTURES
        .lock()
        .expect("gateway chat dispatch capture mutex poisoned")
        .clone()
}

#[cfg(test)]
fn record_gateway_chat_dispatch_for_test(
    message: &str,
    session_id: Option<&str>,
    agent_override: Option<&str>,
) {
    GATEWAY_CHAT_DISPATCH_CAPTURES
        .lock()
        .expect("gateway chat dispatch capture mutex poisoned")
        .push(GatewayChatDispatchCapture {
            message: message.to_string(),
            session_id: session_id.map(ToString::to_string),
            agent_override: agent_override.map(ToString::to_string),
        });
}

pub(crate) async fn run_gateway_chat_with_tools(
    state: &AppState,
    message: &str,
    session_id: Option<&str>,
    agent_override: Option<&str>,
) -> anyhow::Result<GatewayChatOutcome> {
    if let Some(err) = needs_quickstart_for(&state.model) {
        return Err(err);
    }

    // Tests exercise webhook infrastructure (idempotency, auth, autosave)
    // through handle_webhook, so dispatch to the mock model_provider directly
    // instead of bootstrapping the full agent runtime. The mock path
    // doesn't go through the cost-tracking scope.
    #[cfg(test)]
    {
        record_gateway_chat_dispatch_for_test(message, session_id, agent_override);
        let response = state
            .model_provider
            .chat_with_system(None, message, &state.model, state.temperature)
            .await?;
        Ok(GatewayChatOutcome { response })
    }

    #[cfg(not(test))]
    {
        let config = state.config.read().clone();
        let agent_alias = require_gateway_chat_agent_alias(&config, agent_override)?;

        // Scope the cost tracking context so per-LLM-call usage flows into
        // the gateway's cost tracker and costs.jsonl. A separate
        // `TOOL_LOOP_TURN_USAGE` task-local accumulates this turn's totals so
        // the runtime-owned lifecycle guard can annotate its `AgentEnd`
        // without racing concurrent requests sharing the same tracker.
        // Pricing is built from the
        // unified `build_model_provider_pricing` (alias-keyed, `cost.rates`
        // wins over legacy per-alias pricing).
        let cost_tracking_context = state.cost_tracker.as_ref().map(|tracker| {
            let pricing = clawcrew_runtime::agent::cost::build_model_provider_pricing(&config);
            clawcrew_runtime::agent::cost::ToolLoopCostTrackingContext::new(
                tracker.clone(),
                std::sync::Arc::new(pricing),
            )
            .with_agent_alias(&agent_alias)
        });
        let turn_usage = state.cost_tracker.as_ref().map(|_| {
            std::sync::Arc::new(parking_lot::Mutex::new(
                clawcrew_runtime::agent::cost::TurnUsage::default(),
            ))
        });
        let response = Box::pin(clawcrew_runtime::agent::cost::TOOL_LOOP_TURN_USAGE.scope(
            turn_usage.clone(),
            clawcrew_runtime::agent::cost::TOOL_LOOP_COST_TRACKING_CONTEXT.scope(
                cost_tracking_context,
                clawcrew_runtime::agent::process_message(
                    config,
                    &agent_alias,
                    message,
                    session_id,
                    clawcrew_api::ingress::TurnOrigin::Interactive,
                ),
            ),
        ))
        .await?;
        Ok(GatewayChatOutcome { response })
    }
}

fn resolve_gateway_chat_agent_alias(
    config: &Config,
    agent_override: Option<&str>,
) -> Option<String> {
    agent_override
        .map(ToString::to_string)
        .or_else(|| config.resolved_runtime_agent_alias().map(str::to_owned))
}

fn require_gateway_chat_agent_alias(
    config: &Config,
    agent_override: Option<&str>,
) -> anyhow::Result<String> {
    resolve_gateway_chat_agent_alias(config, agent_override).ok_or_else(|| {
        ::clawcrew_log::record!(
            WARN,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                .with_outcome(::clawcrew_log::EventOutcome::Failure),
            "webhook chat rejected: no configured [agents.<alias>] entry"
        );
        anyhow::Error::msg("webhook chat requires at least one configured [agents.<alias>] entry")
    })
}

fn optional_channel_routes() -> Router<AppState> {
    let router: Router<AppState> = Router::new();
    #[cfg(feature = "channel-whatsapp-cloud")]
    let router = router
        .route("/whatsapp", get(handle_whatsapp_verify))
        .route("/whatsapp", post(handle_whatsapp_message))
        .route("/whatsapp/{alias}", get(handle_whatsapp_verify_alias))
        .route("/whatsapp/{alias}", post(handle_whatsapp_message_alias));
    #[cfg(feature = "channel-linq")]
    let router = router
        .route("/linq", post(handle_linq_webhook))
        .route("/linq/{alias}", post(handle_linq_webhook_alias));
    #[cfg(feature = "channel-nextcloud")]
    let router = router
        .route("/nextcloud-talk", post(handle_nextcloud_talk_webhook))
        .route(
            "/nextcloud-talk/{alias}",
            post(handle_nextcloud_talk_webhook_alias),
        );
    #[cfg(feature = "channel-email")]
    let router = router.route("/webhook/gmail", post(handle_gmail_push_webhook));
    router
}

fn sop_webhook_routes() -> Router<AppState> {
    Router::new().route("/sop/{*rest}", post(api_sop_webhook::handle_sop_webhook))
}

/// Webhook request body
#[derive(serde::Deserialize)]
pub struct WebhookBody {
    pub message: String,
    /// Opt in to Server-Sent Events streaming for this turn. Callers must also
    /// send an `Accept: text/event-stream` header.
    #[serde(default)]
    pub stream: bool,
}

/// Webhook query parameters
#[derive(Default, serde::Deserialize)]
pub struct WebhookQuery {
    /// Configured agent alias to dispatch to. Optional — when omitted, the
    /// legacy pick applies (migration-synthesized "default" agent, else the
    /// first enabled one). Aliases mirror `WsQuery` so `/ws/chat` callers
    /// can reuse their query string verbatim.
    #[serde(default, alias = "agentAlias", alias = "agent_alias")]
    pub agent: Option<String>,
}

type WebhookJsonResponse = (StatusCode, Json<serde_json::Value>);

/// Resolve the gateway webhook credential from its canonical live config.
/// Channel webhook aliases own separate HMAC listeners and never participate
/// in authorization for the gateway's `/webhook` or `/sop/*` routes.
fn configured_gateway_webhook_secret_hash(state: &AppState) -> Option<String> {
    state
        .config
        .read()
        .gateway
        .webhook_secret
        .as_deref()
        .map(str::trim)
        .filter(|secret| !secret.is_empty())
        .map(hash_webhook_secret)
}

/// Immutable snapshot of the credential policy applied to ONE request.
///
/// `AppState::config` is a live `Arc<RwLock<Config>>` that the config
/// PUT/PATCH handlers and `POST /admin/reload` legitimately mutate while a
/// request is in flight. Reading the policy twice therefore lets a single
/// request straddle two security states: a headerless request can pass an
/// "unconfigured" read and then satisfy a "configured" read after an operator
/// inserts a secret, and a request bearing a revoked secret can pass a read
/// against the old secret and then be admitted merely because a replacement is
/// present.
///
/// This verdict is produced by exactly one policy read inside
/// [`authorize_webhook_request`] and is then threaded through dispatch. No
/// downstream code re-reads `state.config` for an authorization decision, so
/// live rotation takes effect at *next*-request granularity instead of mixing
/// two security states inside one request.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WebhookAuthVerdict {
    /// `gateway.require_pairing` was on in this snapshot AND the bearer token verified.
    pairing_verified: bool,
    /// `gateway.webhook_secret` was set in this snapshot AND `X-Webhook-Secret` matched it.
    secret_verified: bool,
    /// At least one control was configured in this same snapshot.
    any_control_configured: bool,
}

impl WebhookAuthVerdict {
    /// The composition rule the docs state: at least one control must be
    /// configured, and every configured control must have passed. A verdict is
    /// only ever constructed after every configured control passed, so both
    /// facts come from the SAME snapshot and insertion/rotation cannot straddle
    /// them.
    fn may_dispatch_sop(self) -> bool {
        self.any_control_configured && (self.pairing_verified || self.secret_verified)
    }
}

fn authorize_webhook_request(
    state: &AppState,
    peer_addr: SocketAddr,
    headers: &HeaderMap,
) -> Result<WebhookAuthVerdict, WebhookJsonResponse> {
    let rate_key = client_key_from_request(Some(peer_addr), headers, state.trust_forwarded_headers);
    if !state.rate_limiter.allow_webhook(&rate_key) {
        ::clawcrew_log::record!(
            WARN,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_outcome(::clawcrew_log::EventOutcome::Unknown),
            "/webhook rate limit exceeded"
        );
        let err = serde_json::json!({
            "error": "Too many webhook requests. Please retry later.",
            "retry_after": RATE_LIMIT_WINDOW_SECS,
        });
        return Err((StatusCode::TOO_MANY_REQUESTS, Json(err)));
    }

    // ── The single authorization policy read for this request ──
    // Everything below decides from `snapshot_secret_hash` / `require_pairing`
    // captured here; `state.config` is never consulted again for an
    // authorization decision on this request.
    let snapshot_secret_hash = configured_gateway_webhook_secret_hash(state);
    let require_pairing = state.pairing.require_pairing();
    let any_control_configured = require_pairing || snapshot_secret_hash.is_some();
    let mut pairing_verified = false;
    let mut secret_verified = false;

    if require_pairing {
        if let Err(e) = state.auth_limiter.check_rate_limit(&rate_key) {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({"rate_key": rate_key})),
                "webhook: auth rate limit exceeded for"
            );
            let err = serde_json::json!({
                "error": format!("Too many auth attempts. Try again in {}s.", e.retry_after_secs),
                "retry_after": e.retry_after_secs,
            });
            return Err((StatusCode::TOO_MANY_REQUESTS, Json(err)));
        }
        let auth = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let token = auth.strip_prefix("Bearer ").unwrap_or("");
        if !state.pairing.is_authenticated(token) {
            state.auth_limiter.record_attempt(&rate_key);
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                "webhook: rejected — not paired / invalid bearer token"
            );
            let err = serde_json::json!({
                "error": "Unauthorized — pair first via POST /pair, then send Authorization: Bearer <token>"
            });
            return Err((StatusCode::UNAUTHORIZED, Json(err)));
        }
        pairing_verified = true;
    }

    if let Some(secret_hash) = snapshot_secret_hash.as_deref() {
        let header_hash = headers
            .get("X-Webhook-Secret")
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(hash_webhook_secret);
        match header_hash {
            Some(val) if constant_time_eq(&val, secret_hash) => {
                secret_verified = true;
            }
            _ => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                    "webhook: rejected request — invalid or missing X-Webhook-Secret"
                );
                let err = serde_json::json!({"error": "Unauthorized — invalid or missing X-Webhook-Secret header"});
                return Err((StatusCode::UNAUTHORIZED, Json(err)));
            }
        }
    }

    Ok(WebhookAuthVerdict {
        pairing_verified,
        secret_verified,
        any_control_configured,
    })
}

/// Build an injective storage key for the replay/idempotency store.
///
/// Both the namespace (derived from a caller-controlled SOP path) and the
/// caller's `X-Idempotency-Key` are attacker-controlled strings, so a bare
/// `"{namespace}:{key}"` join is ambiguous: `/sop/a` with key `b:c` produced
/// the same string as `/sop/a:b` with key `c`, and a `/webhook` caller could
/// forge the key `sop:/sop/deploy:k` to collide with `/sop/deploy` key `k`.
/// An authenticated caller could therefore suppress a *different* attempt
/// despite the documented per-path and per-endpoint isolation.
///
/// This encoding is injective because every component is length-prefixed:
/// a reader consumes exactly the declared number of bytes, so no component
/// boundary can be forged by embedding the separator inside a component. The
/// endpoint domain tag is carried as its own component, so `/webhook` keys can
/// never alias `/sop/*` keys.
fn idempotency_storage_key(namespace: Option<&str>, idempotency_key: &str) -> String {
    fn push_component(out: &mut String, value: &str) {
        // `<byte-len>:<value>` — the length prefix makes the split point
        // unambiguous regardless of what `value` contains.
        out.push_str(&value.len().to_string());
        out.push(':');
        out.push_str(value);
    }

    let mut key = String::new();
    match namespace {
        Some(namespace) => {
            push_component(&mut key, "ns");
            push_component(&mut key, namespace);
        }
        None => push_component(&mut key, "global"),
    }
    push_component(&mut key, idempotency_key);
    key
}

/// Emit duplicate telemetry without copying caller-controlled key material
/// into the log pipeline. The key remains available to the replay store; only
/// its presence is useful and safe at this observability boundary.
fn record_duplicate_idempotency_log() {
    ::clawcrew_log::record!(
        INFO,
        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
            .with_attrs(::serde_json::json!({"idempotency_key_present": true})),
        "webhook duplicate ignored"
    );
}

fn check_webhook_idempotency(
    state: &AppState,
    headers: &HeaderMap,
    namespace: Option<&str>,
) -> Option<WebhookJsonResponse> {
    let idempotency_key = headers
        .get("X-Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let storage_key = idempotency_storage_key(namespace, idempotency_key);
    if state.idempotency_store.record_if_new(&storage_key) {
        return None;
    }

    record_duplicate_idempotency_log();
    Some((
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "duplicate",
            "idempotent": true,
            "message": "A prior request already reserved this idempotency key; no new dispatch was started"
        })),
    ))
}

/// Fail closed before a SOP run starts. Starting a SOP run authorizes real
/// side effects, so — unlike the chat-only `/webhook` fallback, which keeps
/// its existing default-open policy — dispatch must not proceed when both
/// `gateway.require_pairing` and the webhook secret are unset (the official
/// container's default configuration). Repo policy is "new external surfaces
/// default closed".
///
/// This is a PURE function over the request-scoped [`WebhookAuthVerdict`]
/// produced by the single policy read in [`authorize_webhook_request`]. It
/// deliberately takes no `&AppState`: re-reading the live config here is what
/// let a request straddle two security states across a concurrent config
/// mutation.
fn require_sop_dispatch_credentials(
    verdict: WebhookAuthVerdict,
) -> Result<(), WebhookJsonResponse> {
    if verdict.may_dispatch_sop() {
        return Ok(());
    }
    ::clawcrew_log::record!(
        WARN,
        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
            .with_outcome(::clawcrew_log::EventOutcome::Failure),
        "sop webhook dispatch rejected — no credential configured"
    );
    let err = serde_json::json!({
        "error": "SOP webhook dispatch requires a configured credential: set \
                  `gateway.require_pairing = true` and authenticate with \
                  `Authorization: Bearer <paired-token>` (pair first via POST /pair), or set \
                  `gateway.webhook_secret` and send X-Webhook-Secret."
    });
    Err((StatusCode::UNAUTHORIZED, Json(err)))
}

/// POST /webhook — main webhook endpoint
async fn handle_webhook(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    Query(query): Query<WebhookQuery>,
    headers: HeaderMap,
    body: Result<Json<WebhookBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let auth_verdict = match authorize_webhook_request(&state, peer_addr, &headers) {
        Ok(verdict) => verdict,
        Err(response) => return response.into_response(),
    };
    let Json(webhook_body) = match body {
        Ok(b) => b,
        Err(e) => {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
                "webhook JSON parse error"
            );
            let err = serde_json::json!({
                "error": "Invalid JSON body. Expected: {\"message\": \"...\"}"
            });
            return (StatusCode::BAD_REQUEST, Json(err)).into_response();
        }
    };

    // ── SOP dispatch first ──
    // A matching `/webhook` SOP trigger takes the request before any
    // chat-only routing (query params, agent aliases) is even inspected —
    // the advertised contract is SOP dispatch first, chat fallback second,
    // so a chat-only param must never block a matching SOP.
    let sop_payload = serde_json::json!({ "message": &webhook_body.message }).to_string();
    let has_matching_sop = match api_sop_webhook::has_matching_webhook_sop(&state, "/webhook") {
        Ok(matches) => matches,
        Err(response) => return response.into_response(),
    };

    if has_matching_sop {
        if let Err(response) = require_sop_dispatch_credentials(auth_verdict) {
            return response.into_response();
        }
        if let Some(response) = check_webhook_idempotency(&state, &headers, None) {
            return response.into_response();
        }
        if let api_sop_webhook::SopWebhookOutcome::Handled(response) =
            api_sop_webhook::dispatch_webhook_sop(&state, "/webhook", Some(&sop_payload)).await
        {
            return response.into_response();
        }
        // The engine reported no match after all (e.g. a trigger was
        // unloaded between the pre-check above and dispatch); fall through
        // to chat. The idempotency key above is already consumed for this
        // attempt.
    }

    // ── Per-request agent dispatch (optional `?agent=` query param) ──
    // Chat-only routing: only reached when no SOP trigger matched
    // `/webhook`, so a bogus `?agent=` can never block a matching SOP
    // dispatch. Validated before idempotency / autosave so a typo'd alias
    // doesn't consume the caller's idempotency key. Mirrors the `/ws/chat`
    // unknown-agent rejection.
    let agent_override = query
        .agent
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(alias) = agent_override {
        let cfg = state.config.read();
        if cfg.agent(alias).is_none() {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({"agent": alias})),
                "webhook: rejected — unknown agent alias"
            );
            let err = serde_json::json!({
                "error": format!(
                    "Unknown agent `{alias}` — no [agents.{alias}] entry configured."
                )
            });
            return (StatusCode::BAD_REQUEST, Json(err)).into_response();
        }
    }

    if !has_matching_sop && let Some(response) = check_webhook_idempotency(&state, &headers, None) {
        return response.into_response();
    }

    let message = &webhook_body.message;
    let session_id = webhook_session_id(&headers);

    if state.auto_save && !clawcrew_memory::should_skip_autosave_content(message) {
        let key = webhook_memory_key();
        let _ = state
            .mem
            .store(
                &key,
                message,
                MemoryCategory::Conversation,
                session_id.as_deref(),
            )
            .await;
    }

    let model_label = {
        let cfg = state.config.read();
        let resolved_agent_alias = resolve_gateway_chat_agent_alias(&cfg, agent_override);
        let resolved_provider = resolved_agent_alias
            .as_deref()
            .and_then(|alias| cfg.resolved_model_provider_for_agent(alias));
        resolved_provider
            .and_then(|(_, _, entry)| {
                entry
                    .model
                    .as_deref()
                    .map(str::trim)
                    .filter(|model| !model.is_empty())
                    .map(ToString::to_string)
            })
            .or_else(|| cfg.resolve_default_model())
            .unwrap_or_else(|| "<unresolved>".to_string())
    };
    // HTTP transport owns request latency and response mapping. The production
    // dispatch below enters `process_message`, whose runtime turn guard is the
    // sole owner of lifecycle and LLM events. Emitting another bracket here
    // gives one webhook prompt two unrelated turn IDs.
    let started_at = Instant::now();

    // ── Optional SSE streaming ──
    // Opt in with `stream: true` plus an `Accept: text/event-stream` header:
    // the turn then streams cumulative assistant tokens as `event: token`
    // frames, ends with `event: done` (or `event: error`), and honours
    // client disconnects through the shared cancellation registry. Every
    // other combination keeps the JSON `{ response }` path below untouched.
    if webhook_body.stream && accepts_sse(&headers) {
        return run_gateway_chat_streaming_response(
            &state,
            message,
            session_id.as_deref(),
            agent_override,
            started_at,
        )
        .await;
    }

    match run_gateway_chat_with_tools(&state, message, session_id.as_deref(), agent_override).await
    {
        Ok(GatewayChatOutcome { response, .. }) => {
            let duration = started_at.elapsed();
            state.observer.record_metric(
                &clawcrew_runtime::observability::traits::ObserverMetric::RequestLatency(duration),
            );

            let body = serde_json::json!({"response": response, "model": model_label});
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(e) => {
            let duration = started_at.elapsed();
            let sanitized = clawcrew_providers::sanitize_api_error(&e.to_string());
            state.observer.record_metric(
                &clawcrew_runtime::observability::traits::ObserverMetric::RequestLatency(duration),
            );
            state
                .observer
                .record_event(&clawcrew_runtime::observability::ObserverEvent::Error {
                    component: "gateway".to_string(),
                    message: sanitized.clone(),
                });
            if is_needs_quickstart_err(&e) {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                    "Webhook chat refused: gateway has no model configured; \
                     visit /quickstart"
                );
                let body = serde_json::json!({
                    "error": "needs_quickstart",
                    "url": "/quickstart"
                });
                (StatusCode::SERVICE_UNAVAILABLE, Json(body)).into_response()
            } else {
                ::clawcrew_log::record!(
                    ERROR,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"error": sanitized})),
                    "webhook model_provider error"
                );
                let err = serde_json::json!({"error": "LLM request failed"});
                (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response()
            }
        }
    }
}

/// True when the request's `Accept` header negotiates Server-Sent Events.
fn accepts_sse(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_ascii_lowercase().contains("text/event-stream"))
        .unwrap_or(false)
}

/// Build one cumulative `event: token` SSE frame.
fn sse_token_frame(cumulative: &str) -> Result<SseWireEvent, std::convert::Infallible> {
    let data = serde_json::json!({ "text": cumulative }).to_string();
    Ok(SseWireEvent::default().event("token").data(data))
}

/// Build the terminating `event: error` SSE frame.
fn sse_error_frame(message: &str) -> Result<SseWireEvent, std::convert::Infallible> {
    let data = serde_json::json!({ "message": message }).to_string();
    Ok(SseWireEvent::default().event("error").data(data))
}

fn send_sse_terminal_error(
    terminal_tx: &mut Option<tokio::sync::oneshot::Sender<SseFrame>>,
    message: &str,
) {
    if let Some(sender) = terminal_tx.take() {
        let _ = sender.send(sse_error_frame(message));
    }
}

/// Reconcile the runtime's authoritative final response with text already
/// emitted from streamed chunks. The runtime may return a cached response with
/// no chunks, or append a receipt/fallback suffix after the last chunk. Prefix
/// reconciliation emits only the missing suffix; a non-prefix conflict uses a
/// replacement cumulative frame so clients never concatenate duplicate text.
/// An empty final response is authoritative too: when earlier chunks exist it
/// emits an empty cumulative replacement instead of leaving stale text visible.
fn reconcile_sse_final_response(cumulative: &mut String, final_response: &str) -> Option<SseFrame> {
    if final_response == cumulative {
        return None;
    }

    if let Some(suffix) = final_response.strip_prefix(cumulative.as_str()) {
        cumulative.push_str(suffix);
    } else {
        cumulative.clear();
        cumulative.push_str(final_response);
    }
    Some(sse_token_frame(cumulative))
}

type SseFrame = Result<SseWireEvent, std::convert::Infallible>;

async fn send_sse_frame_or_cancel(
    frame_tx: &tokio::sync::mpsc::Sender<SseFrame>,
    frame: SseFrame,
    cancel_token: &tokio_util::sync::CancellationToken,
) -> bool {
    tokio::select! {
        result = frame_tx.send(frame) => result.is_ok(),
        _ = cancel_token.cancelled() => false,
    }
}

/// Register the current turn for a gateway session and cancel any replaced
/// turn before returning. Both HTTP/SSE and WebSocket transports share this
/// registry, so replacement ownership must be identical at both edges.
pub(crate) fn register_cancel_token(
    cancel_tokens: &Arc<
        std::sync::Mutex<
            std::collections::HashMap<String, Arc<tokio_util::sync::CancellationToken>>,
        >,
    >,
    session_key: &str,
    cancel_token: Arc<tokio_util::sync::CancellationToken>,
) {
    let previous_token = cancel_tokens
        .lock()
        .expect("cancel_tokens lock poisoned")
        .insert(session_key.to_owned(), cancel_token);
    if let Some(previous_token) = previous_token {
        previous_token.cancel();
    }
}

/// Remove a turn's registry entry only while it still owns the session key.
/// A late completion from a replaced WS/SSE turn must never remove the newer
/// turn's cancellation handle.
pub(crate) fn remove_cancel_token_if_current(
    cancel_tokens: &Arc<
        std::sync::Mutex<
            std::collections::HashMap<String, Arc<tokio_util::sync::CancellationToken>>,
        >,
    >,
    session_key: &str,
    cancel_token: &Arc<tokio_util::sync::CancellationToken>,
) {
    let mut tokens = cancel_tokens.lock().expect("cancel_tokens lock poisoned");
    if tokens
        .get(session_key)
        .is_some_and(|current| Arc::ptr_eq(current, cancel_token))
    {
        tokens.remove(session_key);
    }
}

struct SseClientStream {
    receiver: tokio::sync::mpsc::Receiver<SseFrame>,
    terminal_receiver: Option<tokio::sync::oneshot::Receiver<SseFrame>>,
    terminal_delivered: bool,
    cancel_token: Arc<tokio_util::sync::CancellationToken>,
    cancel_tokens: Arc<
        std::sync::Mutex<
            std::collections::HashMap<String, Arc<tokio_util::sync::CancellationToken>>,
        >,
    >,
    cancel_key: String,
}

impl futures_util::Stream for SseClientStream {
    type Item = SseFrame;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.terminal_delivered {
            return std::task::Poll::Ready(None);
        }

        if let Some(terminal_receiver) = this.terminal_receiver.as_mut() {
            match std::pin::Pin::new(terminal_receiver).poll(cx) {
                std::task::Poll::Ready(Ok(frame)) => {
                    // A terminal error is deliberately independent of the
                    // bounded token queue. Once it is delivered, discard any
                    // queued token frames so the client observes one terminal
                    // error rather than a truncated stream followed by stale
                    // data.
                    this.terminal_receiver = None;
                    this.terminal_delivered = true;
                    this.receiver.close();
                    return std::task::Poll::Ready(Some(frame));
                }
                std::task::Poll::Ready(Err(_)) => {
                    // Normal completion drops the sender after enqueueing the
                    // regular done frame. Continue draining that frame queue.
                    this.terminal_receiver = None;
                }
                std::task::Poll::Pending => {}
            }
        }

        this.receiver.poll_recv(cx)
    }
}

impl Drop for SseClientStream {
    fn drop(&mut self) {
        self.cancel_token.cancel();
        remove_cancel_token_if_current(&self.cancel_tokens, &self.cancel_key, &self.cancel_token);
    }
}

/// Stream one gateway chat turn over Server-Sent Events.
///
/// Drives the turn with the same streamed agent API the `/ws/chat` path uses
/// ([`clawcrew_runtime::agent::Agent::turn_streamed`]), relaying cumulative
/// `TurnEvent::Chunk` text as `event: token` frames. The turn's cancellation
/// token is registered under the same `gw_{session}` key the abort endpoint
/// uses, so an in-flight streamed turn is cancellable exactly like a
/// WebSocket turn; a client disconnect cancels the token too.
async fn run_gateway_chat_streaming_response(
    state: &AppState,
    message: &str,
    session_id: Option<&str>,
    agent_override: Option<&str>,
    started_at: Instant,
) -> Response {
    if let Some(err) = needs_quickstart_for(&state.model) {
        if is_needs_quickstart_err(&err) {
            let body = serde_json::json!({
                "error": "needs_quickstart",
                "url": "/quickstart"
            });
            return (StatusCode::SERVICE_UNAVAILABLE, Json(body)).into_response();
        }
    }

    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<clawcrew_api::agent::TurnEvent>(64);
    let cancel_token = Arc::new(tokio_util::sync::CancellationToken::new());

    // Register under the process-local cancellation key so the existing abort
    // endpoint (and the shared cancellation registry) can cancel the
    // in-flight turn without collapsing distinct display ids.
    let (session_key, cancel_key) = match session_id {
        Some(session_id) => (
            gateway_session_key(session_id),
            gateway_cancel_key(session_id),
        ),
        None => {
            let generated_session_id = uuid::Uuid::new_v4().to_string();
            (
                gateway_session_key(&generated_session_id),
                gateway_cancel_key(&generated_session_id),
            )
        }
    };
    register_cancel_token(&state.cancel_tokens, &cancel_key, Arc::clone(&cancel_token));

    let (frame_tx, frame_rx) = tokio::sync::mpsc::channel::<SseFrame>(16);
    let (terminal_tx, terminal_rx) = tokio::sync::oneshot::channel::<SseFrame>();
    let (turn_tx, turn_rx) = tokio::sync::oneshot::channel::<anyhow::Result<String>>();

    let state_for_turn = state.clone();
    let message_owned = message.to_string();
    let session_for_turn = session_id.map(str::to_string);
    let alias_for_turn = agent_override.map(str::to_string);
    let session_key_for_turn = session_key.clone();
    let token_for_turn = Arc::clone(&cancel_token);
    clawcrew_spawn::spawn!(async move {
        let outcome = dispatch_gateway_turn_streaming(
            &state_for_turn,
            &message_owned,
            event_tx,
            token_for_turn.as_ref(),
            session_for_turn.as_deref(),
            alias_for_turn.as_deref(),
            &session_key_for_turn,
        )
        .await;
        let _ = turn_tx.send(outcome);
    });

    let state_for_frames = state.clone();
    let cancel_key_for_frames = cancel_key.clone();
    let token_for_stream = Arc::clone(&cancel_token);
    clawcrew_spawn::spawn!(async move {
        let mut terminal_tx = Some(terminal_tx);
        let mut cumulative = String::new();
        loop {
            tokio::select! {
                maybe = event_rx.recv() => match maybe {
                    Some(clawcrew_api::agent::TurnEvent::Chunk { delta }) => {
                        cumulative.push_str(&delta);
                        if !send_sse_frame_or_cancel(
                            &frame_tx,
                            sse_token_frame(&cumulative),
                            &cancel_token,
                        )
                        .await
                        {
                            // The bounded token queue may be full, so a
                            // cancellation-aware send can return without
                            // delivering the terminal frame through it.
                            cancel_token.cancel();
                            let message = clawcrew_providers::sanitize_api_error(
                                &clawcrew_runtime::i18n::get_required_cli_string(
                                    "turn-interrupted-by-user",
                                ),
                            );
                            send_sse_terminal_error(&mut terminal_tx, &message);
                            remove_cancel_token_if_current(
                                &state_for_frames.cancel_tokens,
                                &cancel_key_for_frames,
                                &cancel_token,
                            );
                            return;
                        }
                    }
                    Some(_) => {}
                    None => break,
                },
                _ = cancel_token.cancelled() => {
                    // A server-side abort can leave the HTTP connection open.
                    // Emit one sanitized terminal frame before ending the
                    // stream; a disconnected client makes this send fail and
                    // is handled by the same path without additional work.
                    let message = clawcrew_providers::sanitize_api_error(
                        &clawcrew_runtime::i18n::get_required_cli_string(
                            "turn-interrupted-by-user",
                        ),
                    );
                    send_sse_terminal_error(&mut terminal_tx, &message);
                    remove_cancel_token_if_current(
                        &state_for_frames.cancel_tokens,
                        &cancel_key_for_frames,
                        &cancel_token,
                    );
                    return;
                }
            }
        }
        let result = turn_rx
            .await
            .unwrap_or_else(|e| Err(anyhow::Error::msg(e.to_string())));
        remove_cancel_token_if_current(
            &state_for_frames.cancel_tokens,
            &cancel_key_for_frames,
            &cancel_token,
        );
        state_for_frames.observer.record_metric(
            &clawcrew_runtime::observability::traits::ObserverMetric::RequestLatency(
                started_at.elapsed(),
            ),
        );
        match result {
            Ok(final_response) => {
                if let Some(frame) = reconcile_sse_final_response(&mut cumulative, &final_response)
                {
                    if !send_sse_frame_or_cancel(&frame_tx, frame, &cancel_token).await {
                        cancel_token.cancel();
                        let message = clawcrew_providers::sanitize_api_error(
                            &clawcrew_runtime::i18n::get_required_cli_string(
                                "turn-interrupted-by-user",
                            ),
                        );
                        send_sse_terminal_error(&mut terminal_tx, &message);
                        return;
                    }
                }
                if !send_sse_frame_or_cancel(
                    &frame_tx,
                    Ok(SseWireEvent::default().event("done").data("{}")),
                    &cancel_token,
                )
                .await
                {
                    cancel_token.cancel();
                    let message = clawcrew_providers::sanitize_api_error(
                        &clawcrew_runtime::i18n::get_required_cli_string(
                            "turn-interrupted-by-user",
                        ),
                    );
                    send_sse_terminal_error(&mut terminal_tx, &message);
                }
            }
            Err(e) => {
                if cancel_token.is_cancelled() {
                    let message = clawcrew_providers::sanitize_api_error(
                        &clawcrew_runtime::i18n::get_required_cli_string(
                            "turn-interrupted-by-user",
                        ),
                    );
                    send_sse_terminal_error(&mut terminal_tx, &message);
                } else {
                    let sanitized = clawcrew_providers::sanitize_api_error(&e.to_string());
                    if !send_sse_frame_or_cancel(
                        &frame_tx,
                        sse_error_frame(&sanitized),
                        &cancel_token,
                    )
                    .await
                    {
                        cancel_token.cancel();
                        let message = clawcrew_providers::sanitize_api_error(
                            &clawcrew_runtime::i18n::get_required_cli_string(
                                "turn-interrupted-by-user",
                            ),
                        );
                        send_sse_terminal_error(&mut terminal_tx, &message);
                    }
                }
            }
        }
    });

    let body = SseBody::new(SseClientStream {
        receiver: frame_rx,
        terminal_receiver: Some(terminal_rx),
        terminal_delivered: false,
        cancel_token: token_for_stream,
        cancel_tokens: Arc::clone(&state.cancel_tokens),
        cancel_key,
    })
    .keep_alive(SseKeepAlive::default());
    (
        StatusCode::OK,
        [
            (header::CACHE_CONTROL, "no-cache"),
            (header::CONTENT_TYPE, "text/event-stream"),
        ],
        body,
    )
        .into_response()
}

/// Dispatch one streamed gateway chat turn, emitting [`TurnEvent`]s on
/// `event_tx`. Test builds dispatch to the mock model provider directly
/// (mirroring `run_gateway_chat_with_tools`); production builds construct
/// the per-request agent and drive it through `turn_streamed`.
async fn dispatch_gateway_turn_streaming(
    state: &AppState,
    message: &str,
    event_tx: tokio::sync::mpsc::Sender<clawcrew_api::agent::TurnEvent>,
    cancel_token: &tokio_util::sync::CancellationToken,
    session_id: Option<&str>,
    agent_override: Option<&str>,
    session_key: &str,
) -> anyhow::Result<String> {
    #[cfg(test)]
    {
        // Keep the lightweight provider fixture for the existing webhook
        // infrastructure tests, but allow configured-agent fixtures to run
        // through the same Agent construction and cost scope as production.
        let configured_agent = {
            let config = state.config.read();
            resolve_gateway_chat_agent_alias(&config, agent_override)
                .is_some_and(|alias| config.agent(&alias).is_some())
        };
        if configured_agent {
            return dispatch_gateway_turn_streaming_with_agent(
                state,
                message,
                event_tx,
                cancel_token,
                session_id,
                agent_override,
                session_key,
            )
            .await;
        }

        record_gateway_chat_dispatch_for_test(message, session_id, agent_override);
        let call =
            state
                .model_provider
                .chat_with_system(None, message, &state.model, state.temperature);
        tokio::select! {
            result = call => {
                let response = result?;
                let _ = event_tx
                    .send(clawcrew_api::agent::TurnEvent::Chunk { delta: response.clone() })
                    .await;
                Ok(response)
            }
            _ = cancel_token.cancelled() => Err(anyhow::Error::msg(
                "streamed webhook turn cancelled",
            )),
        }
    }

    #[cfg(not(test))]
    {
        dispatch_gateway_turn_streaming_with_agent(
            state,
            message,
            event_tx,
            cancel_token,
            session_id,
            agent_override,
            session_key,
        )
        .await
    }
}

/// Construct and drive the production Agent for a streamed gateway turn.
/// Keeping this helper outside the test-only dispatch branch lets configured
/// test fixtures exercise the exact same construction, budget, and attribution
/// path used by non-test builds.
async fn dispatch_gateway_turn_streaming_with_agent(
    state: &AppState,
    message: &str,
    event_tx: tokio::sync::mpsc::Sender<clawcrew_api::agent::TurnEvent>,
    cancel_token: &tokio_util::sync::CancellationToken,
    session_id: Option<&str>,
    agent_override: Option<&str>,
    session_key: &str,
) -> anyhow::Result<String> {
    let config = state.config.read().clone();
    let agent_alias = require_gateway_chat_agent_alias(&config, agent_override)?;
    let mut agent =
        clawcrew_runtime::agent::Agent::from_live_config_with_session_cwd_and_mcp_backchannel(
            Arc::clone(&state.config),
            &agent_alias,
            None,
            true,
            false,
            false,
            state.sop_engine.clone(),
            state.sop_audit.clone(),
            Some(state.canvas_store.clone()),
        )
        .await?;
    #[cfg(test)]
    agent.set_turn_datetime_for_test(gateway_fixture_turn_datetime);
    if let Some(session) = session_id {
        agent.set_memory_session_id(Some(clawcrew_api::session_keys::sanitize_session_key(
            session,
        )));
    }

    let cost_tracking_context = state.cost_tracker.as_ref().map(|tracker| {
        clawcrew_runtime::agent::cost::tool_loop_cost_tracking_context_from_tracker(
            &config,
            &agent_alias,
            tracker.clone(),
        )
    });
    let turn_usage = state.cost_tracker.as_ref().map(|_| {
        Arc::new(Mutex::new(
            clawcrew_runtime::agent::cost::TurnUsage::default(),
        ))
    });
    let (response, _) = Box::pin(clawcrew_runtime::agent::loop_::scope_session_key(
        Some(session_key.to_owned()),
        clawcrew_runtime::agent::cost::TOOL_LOOP_TURN_USAGE.scope(
            turn_usage,
            clawcrew_runtime::agent::cost::TOOL_LOOP_COST_TRACKING_CONTEXT.scope(
                cost_tracking_context,
                agent.turn_streamed(message, event_tx, Some(cancel_token.clone())),
            ),
        ),
    ))
    .await?;
    Ok(response)
}

#[cfg(test)]
fn gateway_fixture_turn_datetime() -> chrono::DateTime<chrono::Local> {
    chrono::DateTime::parse_from_rfc3339("2026-06-25T12:00:00+00:00")
        .expect("fixed gateway fixture timestamp")
        .with_timezone(&chrono::Local)
}

/// `WhatsApp` verification query params
#[derive(serde::Deserialize)]
pub struct WhatsAppVerifyQuery {
    #[serde(rename = "hub.mode")]
    pub mode: Option<String>,
    #[serde(rename = "hub.verify_token")]
    pub verify_token: Option<String>,
    #[serde(rename = "hub.challenge")]
    pub challenge: Option<String>,
}

/// GET /whatsapp — Meta webhook verification (bare path, deprecated fallback).
#[cfg(feature = "channel-whatsapp-cloud")]
async fn handle_whatsapp_verify(
    State(state): State<AppState>,
    Query(params): Query<WhatsAppVerifyQuery>,
) -> Response {
    handle_whatsapp_verify_impl(state, None, params).await
}

/// GET /whatsapp/{alias} — Meta webhook verification for a specific instance.
#[cfg(feature = "channel-whatsapp-cloud")]
async fn handle_whatsapp_verify_alias(
    State(state): State<AppState>,
    Path(alias): Path<String>,
    Query(params): Query<WhatsAppVerifyQuery>,
) -> Response {
    handle_whatsapp_verify_impl(state, Some(alias), params).await
}

#[cfg(feature = "channel-whatsapp-cloud")]
async fn handle_whatsapp_verify_impl(
    state: AppState,
    alias: Option<String>,
    params: WhatsAppVerifyQuery,
) -> Response {
    let resolved = api_webhook::resolve(&state.whatsapp, alias.as_deref());
    let Some((_alias, wa)) = resolved.entry() else {
        return api_webhook::not_found("whatsapp");
    };

    // Verify the token matches (constant-time comparison to prevent timing attacks)
    let token_matches = params
        .verify_token
        .as_deref()
        .is_some_and(|t| constant_time_eq(t, wa.verify_token()));
    let resp = if params.mode.as_deref() == Some("subscribe") && token_matches {
        if let Some(ch) = params.challenge {
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_attrs(::serde_json::json!({"channel": "whatsapp"})),
                "webhook verified successfully"
            );
            (StatusCode::OK, ch).into_response()
        } else {
            (StatusCode::BAD_REQUEST, "Missing hub.challenge".to_string()).into_response()
        }
    } else {
        ::clawcrew_log::record!(
            WARN,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                .with_attrs(::serde_json::json!({"channel": "whatsapp"})),
            "webhook verification failed — token mismatch"
        );
        (StatusCode::FORBIDDEN, "Forbidden".to_string()).into_response()
    };
    api_webhook::tag_deprecation(resp, resolved, "whatsapp")
}

/// Verify `WhatsApp` webhook signature (`X-Hub-Signature-256`).
/// Returns true if the signature is valid, false otherwise.
/// See: <https://developers.facebook.com/docs/graph-api/webhooks/getting-started#verification-requests>
pub fn verify_whatsapp_signature(app_secret: &str, body: &[u8], signature_header: &str) -> bool {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    // Signature format: "sha256=<hex_signature>"
    let Some(hex_sig) = signature_header.strip_prefix("sha256=") else {
        return false;
    };

    // Decode hex signature
    let Ok(expected) = hex::decode(hex_sig) else {
        return false;
    };

    // Compute HMAC-SHA256
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(app_secret.as_bytes()) else {
        return false;
    };
    mac.update(body);

    // Constant-time comparison
    mac.verify_slice(&expected).is_ok()
}

/// POST /whatsapp — incoming message webhook
/// POST /whatsapp — incoming message webhook (bare path, deprecated fallback).
#[cfg(feature = "channel-whatsapp-cloud")]
async fn handle_whatsapp_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    handle_whatsapp_message_impl(state, None, headers, body).await
}

/// POST /whatsapp/{alias} — incoming message webhook for a specific instance.
#[cfg(feature = "channel-whatsapp-cloud")]
async fn handle_whatsapp_message_alias(
    State(state): State<AppState>,
    Path(alias): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    handle_whatsapp_message_impl(state, Some(alias), headers, body).await
}

#[cfg(feature = "channel-whatsapp-cloud")]
async fn handle_whatsapp_message_impl(
    state: AppState,
    alias: Option<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let resolved = api_webhook::resolve(&state.whatsapp, alias.as_deref());
    let Some((alias_key, wa)) = resolved.entry() else {
        return api_webhook::not_found("whatsapp");
    };
    let app_secret = state.whatsapp_app_secret.get(alias_key).cloned();
    let resp =
        process_whatsapp_message(&state, alias_key, wa, app_secret.as_deref(), headers, body).await;
    api_webhook::tag_deprecation(resp.into_response(), resolved, "whatsapp")
}

/// Verify, parse, and dispatch a WhatsApp webhook payload for one resolved
/// instance. `app_secret` is that instance's `X-Hub-Signature-256` secret.
#[cfg(feature = "channel-whatsapp-cloud")]
async fn process_whatsapp_message(
    state: &AppState,
    alias: &str,
    wa: &Arc<WhatsAppChannel>,
    app_secret: Option<&str>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<serde_json::Value>) {
    let verified = match webhook_ingress::authenticate(
        &webhook_ingress::WHATSAPP_WEBHOOK,
        alias,
        app_secret,
        &headers,
        body,
        |secret, headers, body| {
            let signature = headers
                .get("X-Hub-Signature-256")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            verify_whatsapp_signature(secret, body, signature)
        },
    ) {
        Ok(verified) => verified,
        Err(refusal) => return refusal.into_response(&webhook_ingress::WHATSAPP_WEBHOOK),
    };

    let mut verified = match verified.parse_messages(|body| {
        let payload = serde_json::from_slice::<serde_json::Value>(body).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid JSON payload"})),
            )
        })?;
        Ok::<_, (StatusCode, Json<serde_json::Value>)>(wa.parse_webhook_payload(&payload))
    }) {
        Ok(verified) => verified,
        Err(response) => return response,
    };

    // Route approval replies to pending approval requests before dispatching
    // to the agent.
    let mut approvals = wa.pending_approvals().lock().await;
    verified.retain(|msg| {
        let Some((token, response)) = clawcrew_channels::util::parse_approval_reply(&msg.content)
        else {
            return true;
        };
        let Some(sender) = approvals.remove(&token) else {
            return true;
        };
        let _ = sender.send(response);
        false
    });
    drop(approvals);

    let channel: Arc<dyn Channel> = wa.clone();
    webhook_ingress::dispatch_verified_webhook(
        state,
        verified,
        webhook_ingress::WebhookDispatchContext {
            channel,
            memory_key: whatsapp_memory_key,
            agent_override: None,
            mode: webhook_ingress::WebhookDispatchMode::Synchronous,
            #[cfg(test)]
            suppress_reply_send: true,
        },
    )
    .await
}

/// POST /linq — incoming message webhook (bare path, deprecated fallback).
#[cfg(feature = "channel-linq")]
async fn handle_linq_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    handle_linq_webhook_impl(state, None, headers, body).await
}

/// POST /linq/{alias} — incoming message webhook for a specific instance.
#[cfg(feature = "channel-linq")]
async fn handle_linq_webhook_alias(
    State(state): State<AppState>,
    Path(alias): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    handle_linq_webhook_impl(state, Some(alias), headers, body).await
}

#[cfg(feature = "channel-linq")]
async fn handle_linq_webhook_impl(
    state: AppState,
    alias: Option<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let resolved = api_webhook::resolve(&state.linq, alias.as_deref());
    let Some((alias_key, linq)) = resolved.entry() else {
        return api_webhook::not_found("linq");
    };
    let signing_secret = state.linq_signing_secrets.get(alias_key).cloned();
    let resp = process_linq_webhook(
        &state,
        alias_key,
        linq,
        signing_secret.as_deref(),
        headers,
        body,
    )
    .await;
    api_webhook::tag_deprecation(resp.into_response(), resolved, "linq")
}

/// Verify, parse, and dispatch a Linq webhook payload for one resolved instance.
/// `signing_secret` is that instance's `X-Webhook-Signature` secret.
#[cfg(feature = "channel-linq")]
async fn process_linq_webhook(
    state: &AppState,
    alias: &str,
    linq: &Arc<LinqChannel>,
    signing_secret: Option<&str>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<serde_json::Value>) {
    let verified = match webhook_ingress::authenticate(
        &webhook_ingress::LINQ_WEBHOOK,
        alias,
        signing_secret,
        &headers,
        body,
        |secret, headers, body| {
            let body_str = String::from_utf8_lossy(body);
            let timestamp = headers
                .get("X-Webhook-Timestamp")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            let signature = headers
                .get("X-Webhook-Signature")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            clawcrew_channels::linq::verify_linq_signature(secret, &body_str, timestamp, signature)
        },
    ) {
        Ok(verified) => verified,
        Err(refusal) => return refusal.into_response(&webhook_ingress::LINQ_WEBHOOK),
    };

    let verified = match verified.parse_messages(|body| {
        let payload = serde_json::from_slice::<serde_json::Value>(body).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid JSON payload"})),
            )
        })?;
        Ok::<_, (StatusCode, Json<serde_json::Value>)>(linq.parse_webhook_payload(&payload))
    }) {
        Ok(verified) => verified,
        Err(response) => return response,
    };

    if verified.is_empty() {
        // Acknowledge status/delivery events before ownership resolution.
        return (StatusCode::OK, Json(serde_json::json!({"status": "ok"})));
    }

    let channel_ref = linq_channel_ref(alias);
    let (agent_override, has_channel_bindings) = {
        let config = state.config.read();
        (
            config.agent_for_channel(&channel_ref).map(str::to_owned),
            config
                .agents
                .values()
                .any(|agent| !agent.channels.is_empty()),
        )
    };
    if agent_override.is_none() && has_channel_bindings {
        ::clawcrew_log::record!(
            WARN,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                .with_outcome(::clawcrew_log::EventOutcome::Failure)
                .with_attrs(::serde_json::json!({"channel": "linq", "alias": alias})),
            "Linq webhook ignored because no enabled agent owns the channel alias"
        );
        return (
            StatusCode::OK,
            Json(serde_json::json!({"status": "ignored", "reason": "no_agent_for_channel"})),
        );
    }

    let channel: Arc<dyn Channel> = linq.clone();
    webhook_ingress::dispatch_verified_webhook(
        state,
        verified,
        webhook_ingress::WebhookDispatchContext {
            channel,
            memory_key: linq_memory_key,
            agent_override,
            mode: webhook_ingress::WebhookDispatchMode::Synchronous,
            #[cfg(test)]
            suppress_reply_send: true,
        },
    )
    .await
}

/// POST /nextcloud-talk — incoming message webhook (bare path, deprecated).
#[cfg(feature = "channel-nextcloud")]
async fn handle_nextcloud_talk_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    handle_nextcloud_talk_webhook_impl(state, None, headers, body).await
}

/// POST /nextcloud-talk/{alias} — incoming message webhook for one instance.
#[cfg(feature = "channel-nextcloud")]
async fn handle_nextcloud_talk_webhook_alias(
    State(state): State<AppState>,
    Path(alias): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    handle_nextcloud_talk_webhook_impl(state, Some(alias), headers, body).await
}

#[cfg(feature = "channel-nextcloud")]
async fn handle_nextcloud_talk_webhook_impl(
    state: AppState,
    alias: Option<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let resolved = api_webhook::resolve(&state.nextcloud_talk, alias.as_deref());
    let Some((alias_key, nextcloud_talk)) = resolved.entry() else {
        return api_webhook::not_found("nextcloud-talk");
    };
    let webhook_secret = state.nextcloud_talk_webhook_secret.get(alias_key).cloned();
    let resp = process_nextcloud_talk_webhook(
        &state,
        alias_key,
        nextcloud_talk,
        webhook_secret.as_deref(),
        headers,
        body,
    )
    .await;
    api_webhook::tag_deprecation(resp.into_response(), resolved, "nextcloud-talk")
}

/// Verify, parse, and dispatch a Nextcloud Talk webhook payload for one resolved
/// instance. `webhook_secret` is that instance's HMAC signing secret.
#[cfg(feature = "channel-nextcloud")]
async fn process_nextcloud_talk_webhook(
    state: &AppState,
    alias: &str,
    nextcloud_talk: &Arc<NextcloudTalkChannel>,
    webhook_secret: Option<&str>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<serde_json::Value>) {
    let verified = match webhook_ingress::authenticate(
        &webhook_ingress::NEXTCLOUD_TALK_WEBHOOK,
        alias,
        webhook_secret,
        &headers,
        body,
        |secret, headers, body| {
            let body_str = String::from_utf8_lossy(body);
            let random = headers
                .get("X-Nextcloud-Talk-Random")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            let signature = headers
                .get("X-Nextcloud-Talk-Signature")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            clawcrew_channels::nextcloud_talk::verify_nextcloud_talk_signature(
                secret, random, &body_str, signature,
            )
        },
    ) {
        Ok(verified) => verified,
        Err(refusal) => return refusal.into_response(&webhook_ingress::NEXTCLOUD_TALK_WEBHOOK),
    };

    let verified = match verified.parse_messages(|body| {
        let payload = serde_json::from_slice::<serde_json::Value>(body).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid JSON payload"})),
            )
        })?;
        Ok::<_, (StatusCode, Json<serde_json::Value>)>(
            nextcloud_talk.parse_webhook_payload(&payload),
        )
    }) {
        Ok(verified) => verified,
        Err(response) => return response,
    };

    // Fast-ack: Nextcloud Talk cancels webhook requests that don't complete
    // within ~5s and slow local models routinely exceed that, so processing
    // happens in background tasks and the 200 returns immediately.
    let channel: Arc<dyn Channel> = nextcloud_talk.clone();
    webhook_ingress::dispatch_verified_webhook(
        state,
        verified,
        webhook_ingress::WebhookDispatchContext {
            channel,
            memory_key: nextcloud_talk_memory_key,
            agent_override: None,
            mode: webhook_ingress::WebhookDispatchMode::FastAck,
            #[cfg(test)]
            suppress_reply_send: true,
        },
    )
    .await
}

/// Maximum request body size for the Gmail webhook endpoint (1 MB).
/// Google Pub/Sub messages are typically under 10 KB.
#[cfg(feature = "channel-email")]
const GMAIL_WEBHOOK_MAX_BODY: usize = 1024 * 1024;

/// POST /webhook/gmail — incoming Gmail Pub/Sub push notification
#[cfg(feature = "channel-email")]
async fn handle_gmail_push_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let Some(ref gmail_push) = state.gmail_push else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Gmail push not configured"})),
        );
    };

    // Enforce body size limit.
    if body.len() > GMAIL_WEBHOOK_MAX_BODY {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({"error": "Request body too large"})),
        );
    }

    // Authenticate the webhook request using a shared secret.
    let secret = gmail_push.config.webhook_secret.clone();
    if !secret.is_empty() {
        let provided = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|auth| auth.strip_prefix("Bearer "))
            .unwrap_or("");

        if provided != secret {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({"channel": "gmail_push"})),
                "webhook: unauthorized request"
            );
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Unauthorized"})),
            );
        }
    }

    let body_str = String::from_utf8_lossy(&body);
    let envelope: clawcrew_channels::gmail_push::PubSubEnvelope =
        match serde_json::from_str(&body_str) {
            Ok(e) => e,
            Err(e) => {
                ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(
                        ::serde_json::json!({"error": format!("{}", e), "channel": "gmail_push"})
                    ),
                "webhook: invalid payload"
            );
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "Invalid Pub/Sub envelope"})),
                );
            }
        };

    // Process the notification asynchronously (non-blocking for the webhook response)
    let channel = Arc::clone(gmail_push);
    clawcrew_spawn::spawn!(async move {
        if let Err(e) = channel.handle_notification(&envelope).await {
            ::clawcrew_log::record!(
                ERROR,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(
                        ::serde_json::json!({"channel": "gmail_push", "error": format!("{}", e)})
                    ),
                "push notification processing failed"
            );
        }
    });

    // Acknowledge immediately — Google Pub/Sub requires a 2xx within ~10s
    (StatusCode::OK, Json(serde_json::json!({"status": "ok"})))
}

// ══════════════════════════════════════════════════════════════════════════════
// ADMIN HANDLERS (for CLI management)
// ══════════════════════════════════════════════════════════════════════════════

/// Response for admin endpoints
#[derive(serde::Serialize)]
struct AdminResponse {
    success: bool,
    message: String,
}

/// Reject requests that do not originate from a loopback address.
fn require_localhost(peer: &SocketAddr) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if peer.ip().is_loopback() {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "Admin endpoints are restricted to localhost"
            })),
        ))
    }
}

/// POST /admin/shutdown — graceful shutdown from CLI (localhost only)
async fn handle_admin_shutdown(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    require_localhost(&peer)?;
    ::clawcrew_log::record!(
        INFO,
        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
        "admin shutdown request received; initiating graceful shutdown"
    );

    let body = AdminResponse {
        success: true,
        message: "Gateway shutdown initiated".to_string(),
    };

    let _ = state.shutdown_tx.send(true);

    Ok((StatusCode::OK, Json(body)))
}

/// Authorization decision for `POST /admin/reload`, derived purely from the
/// caller's loopback status, the `gateway.allow_remote_admin` flag, and
/// whether pairing is enabled.
#[derive(Debug, PartialEq, Eq)]
enum AdminReloadGate {
    /// Loopback caller (the CLI) — allow without further checks.
    Allow,
    /// Non-loopback caller, opted in with pairing on — allow only if pairing
    /// auth passes.
    RequireAuth,
    /// Non-loopback caller, not opted in — reject.
    Forbidden,
    /// Non-loopback caller opted in, but pairing is disabled — reject rather
    /// than allow an unauthenticated remote reload. `require_auth` is a no-op
    /// when pairing is off, so without this guard `allow_remote_admin` would
    /// expose reload to anonymous remote callers.
    ForbiddenNoPairing,
}

fn admin_reload_gate(
    is_loopback: bool,
    allow_remote_admin: bool,
    require_pairing: bool,
) -> AdminReloadGate {
    if is_loopback {
        AdminReloadGate::Allow
    } else if !allow_remote_admin {
        AdminReloadGate::Forbidden
    } else if require_pairing {
        AdminReloadGate::RequireAuth
    } else {
        AdminReloadGate::ForbiddenNoPairing
    }
}

async fn handle_admin_reload(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    // Loopback (the CLI) is always allowed. A non-loopback caller is rejected
    // unless the operator opted in via `gateway.allow_remote_admin`, and even
    // then must pass pairing auth — which requires pairing to be enabled, so
    // opting in without pairing is rejected rather than left unauthenticated.
    let allow_remote = state.config.read().gateway.allow_remote_admin;
    // Source pairing status from the guard `require_auth` consults, not the
    // raw config field, so the gate's `RequireAuth` decision can never
    // diverge from what `require_auth` will actually enforce.
    let require_pairing = state.pairing.require_pairing();
    match admin_reload_gate(peer.ip().is_loopback(), allow_remote, require_pairing) {
        AdminReloadGate::Allow => {}
        AdminReloadGate::RequireAuth => api::require_auth(&state, &headers)?,
        AdminReloadGate::Forbidden => {
            return Err((
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "error": "Remote admin reload is disabled. Call from localhost, \
                              or set gateway.allow_remote_admin = true (with pairing \
                              enabled, then pair) to allow authenticated remote reloads."
                })),
            ));
        }
        AdminReloadGate::ForbiddenNoPairing => {
            return Err((
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "error": "Remote admin reload requires pairing. \
                              gateway.allow_remote_admin is enabled but \
                              gateway.require_pairing is off, so remote callers \
                              cannot be authenticated. Enable require_pairing, or \
                              call /admin/reload from localhost."
                })),
            ));
        }
    }

    let Some(reload_tx) = state.reload_tx.clone() else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "no daemon supervisor — running as standalone gateway. \
                          Restart the process to pick up config changes."
            })),
        ));
    };

    ::clawcrew_log::record!(
        INFO,
        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
        "admin reload request received"
    );
    // Clear the pending-reload flag before the daemon supervisor brings up
    // the new gateway instance. The fresh instance starts with the flag
    // already false, matching its "subsystems just-loaded, no pending
    // changes" state.
    state
        .pending_reload
        .store(false, std::sync::atomic::Ordering::Relaxed);
    let shutdown_tx = state.shutdown_tx.clone();
    // Brief delay so the HTTP response flushes before tear-down begins.
    clawcrew_spawn::spawn!(async move {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        // Drain axum first so the listener releases.
        let _ = shutdown_tx.send(true);
        // Then signal the daemon to re-read disk and re-spawn subsystems.
        let _ = reload_tx.send(true);
    });

    Ok((
        StatusCode::OK,
        Json(AdminResponse {
            success: true,
            message: "Daemon reload initiated".to_string(),
        }),
    ))
}

/// GET /admin/paircode — fetch current pairing code (localhost only)
async fn handle_admin_paircode(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    require_localhost(&peer)?;
    let code = state.pairing.pairing_code();

    let body = if let Some(c) = code {
        serde_json::json!({
            "success": true,
            "pairing_required": state.pairing.require_pairing(),
            "pairing_code": c,
            "message": "Use this one-time code to pair"
        })
    } else {
        serde_json::json!({
            "success": true,
            "pairing_required": state.pairing.require_pairing(),
            "pairing_code": null,
            "message": if state.pairing.require_pairing() {
                "Pairing is active but no new code available (already paired or code expired)"
            } else {
                "Pairing is disabled for this gateway"
            }
        })
    };

    Ok((StatusCode::OK, Json(body)))
}

/// The pairing-code policy as configured *right now*.
///
/// `AppState.pairing` outlives every config write (`persist_and_swap` at
/// `api_config.rs` replaces the whole `Config`), so the guard deliberately
/// stores no policy. Resolving it here, per mint, is what makes a
/// strengthened `[gateway.pairing_code]` take effect on the next code
/// instead of at the next restart.
pub(crate) fn live_pairing_code_policy(state: &AppState) -> PairingCodePolicy {
    state.config.read().gateway.pairing_code
}

#[derive(Debug, serde::Deserialize, Default)]
pub struct AdminPaircodeQuery {
    #[serde(default)]
    pub rotate: Option<String>,
}

async fn handle_admin_paircode_new(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(params): Query<AdminPaircodeQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    require_localhost(&peer)?;

    if !state.pairing.require_pairing() {
        let body = serde_json::json!({
            "success": false,
            "pairing_required": false,
            "pairing_code": null,
            "message": "Pairing is disabled for this gateway"
        });
        return Ok((StatusCode::BAD_REQUEST, Json(body)));
    }

    let rotate = params
        .rotate
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let revocation_message = match rotate {
        Some("all") => {
            let revoked = state.pairing.revoke_all_tokens();
            if let Some(registry) = state.device_registry.as_ref() {
                if let Err(e) = registry.clear() {
                    let body = serde_json::json!({
                        "success": false,
                        "pairing_required": true,
                        "pairing_code": null,
                        "message": format!("Tokens revoked in memory but device registry clear failed: {e}"),
                    });
                    return Ok((StatusCode::INTERNAL_SERVER_ERROR, Json(body)));
                }
            }
            if let Err(e) = persist_pairing_tokens(
                state.config.clone(),
                &state.pairing,
                state.config_write_lock.clone(),
            )
            .await
            {
                let body = serde_json::json!({
                    "success": false,
                    "pairing_required": true,
                    "pairing_code": null,
                    "message": format!("Tokens revoked in memory but config persist failed: {e}"),
                });
                return Ok((StatusCode::INTERNAL_SERVER_ERROR, Json(body)));
            }
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_attrs(::serde_json::json!({"revoked": revoked})),
                "all paired tokens revoked via admin endpoint"
            );
            Some(format!(
                "Revoked all {revoked} paired token(s) and cleared the device registry."
            ))
        }
        Some(device_id) => {
            let Some(registry) = state.device_registry.as_ref() else {
                let body = serde_json::json!({
                    "success": false,
                    "pairing_required": true,
                    "pairing_code": null,
                    "message": "Device registry is disabled; cannot rotate a single device.",
                });
                return Ok((StatusCode::SERVICE_UNAVAILABLE, Json(body)));
            };
            let token_hash = match registry.revoke(device_id) {
                Ok(Some(hash)) => hash,
                Ok(None) => {
                    let body = serde_json::json!({
                        "success": false,
                        "pairing_required": true,
                        "pairing_code": null,
                        "message": format!("Device '{device_id}' not found; nothing revoked."),
                    });
                    return Ok((StatusCode::NOT_FOUND, Json(body)));
                }
                Err(e) => {
                    let body = serde_json::json!({
                        "success": false,
                        "pairing_required": true,
                        "pairing_code": null,
                        "message": format!("Device registry error: {e}"),
                    });
                    return Ok((StatusCode::INTERNAL_SERVER_ERROR, Json(body)));
                }
            };
            state.pairing.revoke_token_hash(&token_hash);
            if let Err(e) = persist_pairing_tokens(
                state.config.clone(),
                &state.pairing,
                state.config_write_lock.clone(),
            )
            .await
            {
                let body = serde_json::json!({
                    "success": false,
                    "pairing_required": true,
                    "pairing_code": null,
                    "message": format!("Token revoked in memory but config persist failed: {e}"),
                });
                return Ok((StatusCode::INTERNAL_SERVER_ERROR, Json(body)));
            }
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                "single device token revoked via admin endpoint"
            );
            Some(format!(
                "Revoked the bearer token for device '{device_id}'."
            ))
        }
        None => None,
    };

    let code = state
        .pairing
        .generate_new_pairing_code(live_pairing_code_policy(&state))
        .expect("require_pairing checked above");
    if rotate.is_none() {
        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
            "new pairing code generated via admin endpoint"
        );
    }

    let message = match revocation_message {
        Some(revoked) => {
            format!("{revoked} Use this one-time code to re-pair.")
        }
        None => "New pairing code generated — use this one-time code to pair".to_string(),
    };

    let body = serde_json::json!({
        "success": true,
        "pairing_required": true,
        "pairing_code": code,
        "message": message,
    });
    Ok((StatusCode::OK, Json(body)))
}

async fn handle_pair_code(State(state): State<AppState>) -> impl IntoResponse {
    let require = state.pairing.require_pairing();
    let is_paired = state.pairing.is_paired();

    // Only expose the code during initial setup (before first pairing)
    let code = if require && !is_paired {
        state.pairing.pairing_code()
    } else {
        None
    };

    let body = serde_json::json!({
        "success": true,
        "pairing_required": require,
        "pairing_code": code,
    });

    (StatusCode::OK, Json(body))
}

#[cfg(test)]
mod tests;
