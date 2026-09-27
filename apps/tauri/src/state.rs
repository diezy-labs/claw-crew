//! Shared application state for Tauri.

use std::process::Child;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

/// Loopback port the desktop app expects the gateway/daemon on by default.
pub const DEFAULT_GATEWAY_PORT: u16 = 42617;
/// Default gateway base URL when no override is provided.
pub const DEFAULT_GATEWAY_URL: &str = "http://127.0.0.1:42617";

/// Sources the gateway base URL from the environment, falling back to the single
/// canonical default. Every code path (daemon spawn, tray "Show Browser",
/// dashboard open, health polling) reads the resolved URL from shared state so
/// the port is defined in exactly one place.
pub fn resolve_gateway_url() -> String {
    std::env::var("CLAWCREW_GATEWAY_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_GATEWAY_URL.to_string())
}

/// Derive the gateway port from a base URL, defaulting to [`DEFAULT_GATEWAY_PORT`].
pub fn gateway_port_from_url(url: &str) -> u16 {
    tauri::Url::parse(url)
        .ok()
        .and_then(|u| u.port())
        .unwrap_or(DEFAULT_GATEWAY_PORT)
}

/// Agent status as reported by the gateway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Working,
    Error,
}

/// Shared application state behind an `Arc<RwLock<_>>`.
#[derive(Debug, Clone)]
pub struct AppState {
    pub gateway_url: String,
    pub token: Option<String>,
    pub connected: bool,
    pub agent_status: AgentStatus,
    pub service_enabled: bool,
    pub owned_daemon: Arc<Mutex<Option<Child>>>,
    pub owned_agent_engine: Arc<Mutex<Option<Child>>>,
    pub agent_engine_grpc_port: u16,
    pub agent_engine_metrics_port: u16,
    pub daemon_spawn_lock: Arc<tokio::sync::Mutex<()>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            gateway_url: resolve_gateway_url(),
            token: None,
            connected: false,
            agent_status: AgentStatus::Idle,
            service_enabled: true,
            owned_daemon: Arc::new(Mutex::new(None)),
            owned_agent_engine: Arc::new(Mutex::new(None)),
            agent_engine_grpc_port: 50051,
            agent_engine_metrics_port: 9090,
            daemon_spawn_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}

/// Thread-safe wrapper around `AppState`.
pub type SharedState = Arc<RwLock<AppState>>;

/// Create the default shared state.
pub fn shared_state() -> SharedState {
    Arc::new(RwLock::new(AppState::default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state() {
        let state = AppState::default();
        assert_eq!(state.gateway_url, resolve_gateway_url());
        assert!(state.token.is_none());
        assert!(!state.connected);
        assert_eq!(state.agent_status, AgentStatus::Idle);
        assert!(state.service_enabled);
    }

    #[test]
    fn shared_state_is_cloneable() {
        let s1 = shared_state();
        let s2 = s1.clone();
        // Both references point to the same allocation.
        assert!(Arc::ptr_eq(&s1, &s2));
    }

    #[tokio::test]
    async fn shared_state_concurrent_read_write() {
        let state = shared_state();

        // Write from one handle.
        {
            let mut s = state.write().await;
            s.connected = true;
            s.agent_status = AgentStatus::Working;
            s.token = Some("zc_test".to_string());
        }

        // Read from cloned handle.
        let state2 = state.clone();
        let s = state2.read().await;
        assert!(s.connected);
        assert_eq!(s.agent_status, AgentStatus::Working);
        assert_eq!(s.token.as_deref(), Some("zc_test"));
    }

    #[test]
    fn agent_status_serialization() {
        assert_eq!(
            serde_json::to_string(&AgentStatus::Idle).unwrap(),
            "\"idle\""
        );
        assert_eq!(
            serde_json::to_string(&AgentStatus::Working).unwrap(),
            "\"working\""
        );
        assert_eq!(
            serde_json::to_string(&AgentStatus::Error).unwrap(),
            "\"error\""
        );
    }

    #[test]
    fn gateway_port_derives_from_url_and_defaults() {
        assert_eq!(gateway_port_from_url("http://127.0.0.1:42617"), 42617);
        assert_eq!(gateway_port_from_url("http://localhost:8080/"), 8080);
        assert_eq!(
            gateway_port_from_url("not a url"),
            DEFAULT_GATEWAY_PORT
        );
        assert_eq!(DEFAULT_GATEWAY_URL, "http://127.0.0.1:42617");
    }
}
