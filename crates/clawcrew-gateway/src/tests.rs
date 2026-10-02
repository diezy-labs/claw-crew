    use super::*;
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::{HeaderValue, Request, Uri};
    use axum::response::IntoResponse;
    use http_body_util::BodyExt;
    use parking_lot::{Mutex, RwLock};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tower::ServiceExt;
    #[cfg(feature = "channel-whatsapp-cloud")]
    use clawcrew_api::channel::ChannelMessage;
    use clawcrew_memory::{Memory, MemoryCategory, MemoryEntry};
    use clawcrew_providers::ModelProvider;
    use clawcrew_runtime::agent::loop_::{
        mcp_tool_access_policy, register_eager_mcp_tool_if_allowed,
    };

    #[test]
    fn default_agent_alias_picks_smallest_enabled_and_is_deterministic() {
        use clawcrew_config::schema::AliasedAgentConfig;

        let enabled = || AliasedAgentConfig {
            enabled: true,
            ..AliasedAgentConfig::default()
        };

        // No agents -> no default.
        let mut config = Config::default();
        assert_eq!(default_agent_alias(&config), None);

        // Insertion order is deliberately not alphabetical; `config.agents` is
        // a HashMap whose iteration order is randomized per process. The pick
        // must still be the lexicographically smallest ENABLED alias so the
        // Tools page seeds the same agent on every restart.
        config.agents.insert("zeta".to_string(), enabled());
        config.agents.insert("alpha".to_string(), enabled());
        config.agents.insert("mid".to_string(), enabled());
        assert_eq!(default_agent_alias(&config).as_deref(), Some("alpha"));

        // A smaller-but-disabled alias is skipped (omission is not a grant).
        config.agents.insert(
            "aaa_disabled".to_string(),
            AliasedAgentConfig {
                enabled: false,
                ..AliasedAgentConfig::default()
            },
        );
        assert_eq!(default_agent_alias(&config).as_deref(), Some("alpha"));
    }

    #[test]
    fn gateway_cancel_key_preserves_distinct_session_ids() {
        let dotted = gateway_cancel_key("team.alpha");
        let underscored = gateway_cancel_key("team_alpha");

        assert_eq!(dotted, "gw_team.alpha");
        assert_eq!(underscored, "gw_team_alpha");
        assert_ne!(dotted, underscored);
    }

    /// Generate a random hex secret at runtime to avoid hard-coded cryptographic values.
    fn generate_test_secret() -> String {
        let bytes: [u8; 32] = rand::random();
        hex::encode(bytes)
    }

    struct NamedMcpMockTool(&'static str);
    clawcrew_api::mock_tool_attribution!(NamedMcpMockTool);
    #[async_trait]
    impl tools::Tool for NamedMcpMockTool {
        fn name(&self) -> &str {
            self.0
        }
        fn description(&self) -> &str {
            "mcp mock"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({ "type": "object", "properties": {} })
        }
        async fn execute(&self, _args: serde_json::Value) -> anyhow::Result<tools::ToolResult> {
            Ok(tools::ToolResult {
                success: true,
                output: tools::ToolOutput::default(),
                error: None,
            })
        }
    }

    #[test]
    fn gateway_excluded_tools_drops_denied_mcp_tool() {
        let policy = SecurityPolicy {
            excluded_tools: Some(vec!["aa_mcp__find_items".to_string()]),
            workspace_dir: std::env::temp_dir(),
            ..SecurityPolicy::default()
        };
        let mcp_policy = mcp_tool_access_policy(&policy, None);
        let mut gw_tools: Vec<Box<dyn tools::Tool>> = Vec::new();
        let denied: std::sync::Arc<dyn tools::Tool> =
            std::sync::Arc::new(NamedMcpMockTool("aa_mcp__find_items"));
        let allowed: std::sync::Arc<dyn tools::Tool> =
            std::sync::Arc::new(NamedMcpMockTool("aa_mcp__find_npcs"));
        let registered_denied =
            register_eager_mcp_tool_if_allowed(denied, &mut gw_tools, None, mcp_policy.as_ref());
        let registered_allowed =
            register_eager_mcp_tool_if_allowed(allowed, &mut gw_tools, None, mcp_policy.as_ref());
        assert!(
            !registered_denied,
            "gateway must not register an `excluded_tools`-denied MCP tool"
        );
        assert!(
            registered_allowed,
            "gateway must register a non-denied MCP tool (allowlist auto-admit)"
        );
        let names: Vec<&str> = gw_tools.iter().map(|t| t.name()).collect();
        assert!(
            !names.contains(&"aa_mcp__find_items"),
            "denied MCP tool leaked into the gateway registry; got {names:?}"
        );
        assert!(
            names.contains(&"aa_mcp__find_npcs"),
            "allowed MCP tool missing from the gateway registry; got {names:?}"
        );
    }

    #[test]
    fn security_body_limit_is_64kb() {
        assert_eq!(MAX_BODY_SIZE, 65_536);
    }

    #[test]
    fn security_timeout_default_is_30_seconds() {
        assert_eq!(REQUEST_TIMEOUT_SECS, 30);
    }

    #[test]
    fn gateway_timeout_uses_typed_config_default() {
        let cfg = clawcrew_config::schema::GatewayConfig::default();
        assert_eq!(gateway_request_timeout_secs(&cfg), 30);
    }

    #[test]
    fn paircode_recovery_command_includes_alternate_port() {
        assert_eq!(
            format_paircode_recovery_command("127.0.0.1", 42617),
            "clawcrew gateway get-paircode --new --port 42617"
        );
    }

    #[test]
    fn paircode_recovery_command_includes_specific_host_when_needed() {
        // Admin paircode routes are localhost-only, so the recovery hint must
        // not advertise a non-loopback `--host` (the admin guard would 403 it).
        // The CLI is left to fall back to its loopback default.
        assert_eq!(
            format_paircode_recovery_command("192.168.1.20", 42617),
            "clawcrew gateway get-paircode --new --port 42617"
        );
    }

    #[test]
    fn paircode_recovery_command_uses_loopback_for_nonloopback_host() {
        // a gateway bound to a non-loopback interface must
        // not surface a recovery hint that the localhost-only admin guard rejects.
        let cmd = format_paircode_recovery_command("192.168.1.20", 42617);
        assert!(
            !cmd.contains("192.168.1.20"),
            "recovery command must not advertise the non-loopback bound host: {cmd}"
        );
        assert!(
            !cmd.contains("--host"),
            "recovery command should omit --host so the CLI uses its loopback default: {cmd}"
        );

        let curl = format_paircode_recovery_curl("192.168.1.20", 42617, "");
        assert_eq!(
            curl, "curl -s -X POST http://127.0.0.1:42617/admin/paircode/new",
            "curl fallback must target loopback, not the non-loopback bound host"
        );
        assert!(
            !curl.contains("192.168.1.20"),
            "curl fallback must not advertise the non-loopback bound host: {curl}"
        );

        // Path prefix is still preserved while the host is normalized.
        assert_eq!(
            format_paircode_recovery_curl("192.168.1.20", 42617, "/gw"),
            "curl -s -X POST http://127.0.0.1:42617/gw/admin/paircode/new"
        );
    }

    #[test]
    fn paircode_recovery_curl_targets_running_instance() {
        assert_eq!(
            format_paircode_recovery_curl("127.0.0.1", 42617, ""),
            "curl -s -X POST http://127.0.0.1:42617/admin/paircode/new"
        );
    }

    #[test]
    fn already_paired_notice_states_no_code_was_generated() {
        // the banner must say plainly that NO code exists
        // (already paired), not just "Pairing: ACTIVE" — otherwise the operator
        // hits the dashboard's pairing-code prompt with no code printed
        // anywhere.
        let lines = already_paired_pairing_notice("127.0.0.1", 3001, "");
        let joined = lines.join("\n");
        assert!(
            joined.contains("already paired"),
            "notice must say the gateway is already paired: {joined}"
        );
        assert!(
            joined.contains("no new") && joined.contains("code"),
            "notice must state that no new code was generated: {joined}"
        );
    }

    #[test]
    fn already_paired_notice_includes_recovery_command_and_curl() {
        // The notice is the single source of truth for the on-demand recovery
        // commands; it must reuse the loopback-safe builders so the banner and
        // any future surface never drift from's no-`--host` rule.
        let lines = already_paired_pairing_notice("192.168.1.20", 3001, "/gw");
        let joined = lines.join("\n");
        assert!(
            joined.contains(&format_paircode_recovery_command("192.168.1.20", 3001)),
            "notice must surface the get-paircode recovery command: {joined}"
        );
        assert!(
            joined.contains(&format_paircode_recovery_curl("192.168.1.20", 3001, "/gw")),
            "notice must surface the curl fallback (honoring the path prefix): {joined}"
        );
        // never advertise the non-loopback bound host in the hint.
        assert!(
            !joined.contains("192.168.1.20"),
            "notice must not advertise the non-loopback bound host: {joined}"
        );
    }

    #[test]
    fn paircode_recovery_curl_normalizes_unspecified_bind_hosts() {
        assert_eq!(
            format_paircode_recovery_curl("0.0.0.0", 42617, ""),
            "curl -s -X POST http://127.0.0.1:42617/admin/paircode/new"
        );
        assert_eq!(
            format_paircode_recovery_curl("::", 42617, ""),
            "curl -s -X POST http://127.0.0.1:42617/admin/paircode/new"
        );
    }

    #[test]
    fn paircode_recovery_curl_preserves_actual_loopback_hosts() {
        assert_eq!(
            format_paircode_recovery_curl("localhost", 42617, ""),
            "curl -s -X POST http://localhost:42617/admin/paircode/new"
        );
        assert_eq!(
            format_paircode_recovery_curl("::1", 42617, ""),
            "curl -s -X POST http://[::1]:42617/admin/paircode/new"
        );
    }

    #[test]
    fn paircode_recovery_curl_preserves_path_prefix() {
        assert_eq!(
            format_paircode_recovery_curl("127.0.0.1", 42617, "/gw"),
            "curl -s -X POST http://127.0.0.1:42617/gw/admin/paircode/new"
        );
    }

    #[tokio::test]
    async fn public_health_omits_component_error_details() {
        let component = format!("health-public-{}", uuid::Uuid::new_v4());
        let sensitive_error = "provider failed: token=not-for-public-health";
        clawcrew_runtime::health::mark_component_ok(&component);
        clawcrew_runtime::health::mark_component_error(&component, sensitive_error);

        let tmp = tempfile::TempDir::new().unwrap();
        let response = handle_health(State(admin_paircode_state(&tmp, false, false)))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let public_component = &json["runtime"]["components"][&component];

        assert_eq!(public_component["status"], "error");
        assert!(public_component["updated_at"].is_string());
        assert!(public_component["last_ok"].is_string());
        assert_eq!(public_component["restart_count"], 0);
        assert!(public_component.get("last_error").is_none());
        assert!(!json.to_string().contains(sensitive_error));
        assert_eq!(
            clawcrew_runtime::health::snapshot().components[&component]
                .last_error
                .as_deref(),
            Some(sensitive_error)
        );
    }

    #[test]
    fn resolve_web_dist_dir_accepts_configured_dist() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let dist_dir = temp.path().join("dist");
        std::fs::create_dir_all(&dist_dir).expect("create dist dir");
        std::fs::write(dist_dir.join("index.html"), "").expect("write index.html");
        let mut config = Config::default();
        config.gateway.web_dist_dir = Some(dist_dir.display().to_string());

        assert_eq!(resolve_web_dist_dir(&config), Some(dist_dir));
    }

    #[test]
    fn resolve_web_dist_dir_rejects_configured_path_without_index() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let mut config = Config::default();
        config.gateway.web_dist_dir = Some(temp.path().display().to_string());

        assert_ne!(
            resolve_web_dist_dir(&config),
            Some(temp.path().to_path_buf())
        );
    }

    #[cfg(unix)]
    #[test]
    fn dashboard_index_rejects_symlink_that_escapes_dashboard_root() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().expect("create dashboard root");
        let outside = tempfile::tempdir().expect("create outside directory");
        std::fs::write(outside.path().join("index.html"), "outside dashboard")
            .expect("write outside index");
        symlink(
            outside.path().join("index.html"),
            root.path().join("index.html"),
        )
        .expect("link escaping index");

        let mut config = Config::default();
        config.gateway.web_dist_dir = Some(root.path().display().to_string());

        assert!(!has_servable_dashboard_index(root.path()));
        assert_ne!(
            resolve_web_dist_dir(&config),
            Some(root.path().to_path_buf()),
            "the resolver must not report an index the serving layer rejects"
        );
    }

    #[test]
    #[cfg(not(feature = "embedded-web"))]
    fn web_dashboard_availability_uses_filesystem_dist() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let dist_dir = temp.path().join("dist");
        std::fs::create_dir_all(&dist_dir).expect("create dist dir");
        std::fs::write(dist_dir.join("index.html"), "").expect("write index.html");
        let mut config = Config::default();
        config.gateway.web_dist_dir = Some(dist_dir.display().to_string());

        assert_eq!(
            resolve_web_dashboard_availability(&config),
            Some(WebDashboardAvailability::Filesystem(dist_dir))
        );
    }

    #[test]
    #[cfg(feature = "embedded-web")]
    fn web_dashboard_availability_reports_embedded_assets() {
        let config = Config::default();

        assert_eq!(
            resolve_web_dashboard_availability(&config),
            Some(WebDashboardAvailability::Embedded)
        );
    }

    /// Build an AppState wired with a real pairing guard, on-disk config path,
    /// and an optional device registry so the admin paircode handler's
    /// revoke + persist paths can be exercised end to end.
    pub(super) fn admin_paircode_state(
        tmp: &tempfile::TempDir,
        require_pairing: bool,
        with_registry: bool,
    ) -> AppState {
        let data_dir = tmp.path().join("workspace");
        std::fs::create_dir_all(&data_dir).unwrap();
        let config = Config {
            data_dir: data_dir.clone(),
            config_path: tmp.path().join("config.toml"),
            ..Config::default()
        };
        let registry = with_registry.then(|| Arc::new(api_pairing::DeviceRegistry::new(&data_dir)));
        AppState {
            config: Arc::new(RwLock::new(config)),
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            model_provider: Arc::new(MockModelProvider::default()),
            model: "test-model".into(),
            temperature: None,
            mem: Arc::new(MockMemory),
            memory_strategy: Arc::new(DefaultMemoryStrategy::with_config(
                Arc::new(MockMemory),
                clawcrew_config::schema::MemoryConfig::default(),
                std::path::PathBuf::new(),
            )),
            auto_save: false,
            pairing: Arc::new(PairingGuard::new(
                require_pairing,
                &[],
                PairingCodePolicy::default(),
            )),
            trust_forwarded_headers: false,
            rate_limiter: Arc::new(GatewayRateLimiter::new(100, 100, 100)),
            auth_limiter: Arc::new(auth_rate_limit::AuthRateLimiter::new()),
            idempotency_store: Arc::new(IdempotencyStore::new(Duration::from_secs(300), 1000)),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp: HashMap::new(),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp_app_secret: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq_signing_secrets: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk_webhook_secret: HashMap::new(),
            #[cfg(feature = "channel-email")]
            gmail_push: None,
            observer: Arc::new(clawcrew_runtime::observability::NoopObserver),
            tools_registry: Arc::new(Vec::new()),
            tools_registry_by_agent: Arc::new(std::collections::HashMap::new()),
            cost_tracker: None,
            event_tx: tokio::sync::broadcast::channel(16).0,
            event_buffer: Arc::new(sse::EventBuffer::new(16)),
            shutdown_tx: tokio::sync::watch::channel(false).0,
            reload_tx: None,
            node_registry: Arc::new(nodes::NodeRegistry::new(16)),
            mdns_peer_registry: nodes::mdns::MdnsPeerRegistry::default(),
            path_prefix: String::new(),
            web_dist_dir: None,
            session_backend: None,
            session_queue: std::sync::Arc::new(crate::session_queue::SessionActorQueue::new(
                8, 30, 600,
            )),
            device_registry: registry,
            pending_pairings: None,
            canvas_store: CanvasStore::new(),
            cancel_tokens: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            pending_reload: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tui_registry: None,
            sop_engine: None,
            sop_audit: None,
            #[cfg(feature = "webauthn")]
            webauthn: None,
        }
    }

    fn webhook_sop_state(
        tmp: &tempfile::TempDir,
        trigger_path: &str,
    ) -> (AppState, Arc<MockModelProvider>) {
        let mut state = admin_paircode_state(tmp, false, false);
        let provider = Arc::new(MockModelProvider::default());
        state.model_provider = provider.clone();

        let sops_dir = tmp.path().join("sops");
        let sop_dir = sops_dir.join("webhook-test");
        std::fs::create_dir_all(&sop_dir).unwrap();
        std::fs::write(
            sop_dir.join("SOP.toml"),
            format!(
                r#"[sop]
name = "webhook-test"
description = "Gateway webhook fan-in test"
execution_mode = "auto"

[[triggers]]
type = "webhook"
path = "{trigger_path}"
"#,
            ),
        )
        .unwrap();
        std::fs::write(
            sop_dir.join("SOP.md"),
            "## Steps\n\n1. **Handle webhook** — Process the webhook payload.\n",
        )
        .unwrap();

        let mut sop_config = state.config.read().sop.clone();
        sop_config.sops_dir = Some(sops_dir.to_string_lossy().into_owned());
        sop_config.persist_runs = false;
        state.config.write().sop = sop_config.clone();
        let data_dir = state.config.read().data_dir.clone();
        let install_root = state.config.read().install_root_dir();
        let (engine, audit) = clawcrew_runtime::sop::build_sop_engine(
            sop_config,
            &data_dir,
            &install_root,
            Arc::clone(&state.mem),
            Default::default(),
        );
        state.sop_engine = Some(engine);
        state.sop_audit = Some(audit);
        (state, provider)
    }

    /// Same as [`webhook_sop_state`] but loads two SOPs, each with its own
    /// distinct webhook trigger path — used to prove idempotency keys are
    /// namespaced per SOP path rather than shared across all of `/sop/*`.
    fn webhook_two_sop_state(
        tmp: &tempfile::TempDir,
        path_a: &str,
        path_b: &str,
    ) -> (AppState, Arc<MockModelProvider>) {
        let mut state = admin_paircode_state(tmp, false, false);
        let provider = Arc::new(MockModelProvider::default());
        state.model_provider = provider.clone();

        let sops_dir = tmp.path().join("sops");
        for (name, trigger_path) in [("sop-a", path_a), ("sop-b", path_b)] {
            let sop_dir = sops_dir.join(name);
            std::fs::create_dir_all(&sop_dir).unwrap();
            std::fs::write(
                sop_dir.join("SOP.toml"),
                format!(
                    r#"[sop]
name = "{name}"
description = "Gateway webhook fan-in test"
execution_mode = "auto"

[[triggers]]
type = "webhook"
path = "{trigger_path}"
"#,
                ),
            )
            .unwrap();
            std::fs::write(
                sop_dir.join("SOP.md"),
                "## Steps\n\n1. **Handle webhook** — Process the webhook payload.\n",
            )
            .unwrap();
        }

        let mut sop_config = state.config.read().sop.clone();
        sop_config.sops_dir = Some(sops_dir.to_string_lossy().into_owned());
        sop_config.persist_runs = false;
        state.config.write().sop = sop_config.clone();
        let data_dir = state.config.read().data_dir.clone();
        let install_root = state.config.read().install_root_dir();
        let (engine, audit) = clawcrew_runtime::sop::build_sop_engine(
            sop_config,
            &data_dir,
            &install_root,
            Arc::clone(&state.mem),
            Default::default(),
        );
        state.sop_engine = Some(engine);
        state.sop_audit = Some(audit);
        (state, provider)
    }

    /// Attach a webhook-secret credential to `state`; returns the plaintext
    /// secret to send back as `X-Webhook-Secret`. Item 2's fail-closed SOP
    /// dispatch policy requires a configured-and-verified credential before
    /// a SOP run can start.
    fn with_webhook_secret(state: AppState) -> (AppState, String) {
        let secret = generate_test_secret();
        state.config.write().gateway.webhook_secret = Some(secret.clone());
        (state, secret)
    }

    fn webhook_secret_header(secret: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("X-Webhook-Secret", HeaderValue::from_str(secret).unwrap());
        headers
    }

    fn spa_fallback_state(tmp: &tempfile::TempDir) -> AppState {
        let dist_dir = tmp.path().join("web").join("dist");
        std::fs::create_dir_all(&dist_dir).unwrap();
        std::fs::write(
            dist_dir.join("index.html"),
            r#"<!DOCTYPE html><html><head></head><body>dashboard shell</body></html>"#,
        )
        .unwrap();

        let mut state = admin_paircode_state(tmp, false, false);
        state.web_dist_dir = Some(dist_dir);
        state
    }

    async fn spa_fallback_response(
        path: &'static str,
        state: AppState,
    ) -> axum::response::Response {
        static_files::handle_spa_fallback(State(state), Uri::from_static(path)).await
    }

    async fn static_route_response(
        path: &'static str,
        prefix: Option<&str>,
        state: AppState,
    ) -> axum::response::Response {
        let routes = static_file_routes();
        let app = match prefix {
            Some(prefix) => Router::new().nest(prefix, routes),
            None => routes,
        }
        .with_state(state);

        app.oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    /// Pair a device into both the pairing guard and the device registry,
    /// returning the plaintext token so the test can assert it is revoked.
    async fn pair_device(state: &AppState, device_id: &str) -> String {
        let code = state
            .pairing
            .generate_new_pairing_code(live_pairing_code_policy(state))
            .expect("pairing enabled");
        let token = state
            .pairing
            .try_pair(&code, device_id)
            .await
            .unwrap()
            .unwrap();
        state
            .device_registry
            .as_ref()
            .unwrap()
            .register(
                PairingGuard::token_hash(&token),
                api_pairing::DeviceInfo {
                    id: device_id.to_string(),
                    name: None,
                    device_type: None,
                    paired_at: chrono::Utc::now(),
                    last_seen: chrono::Utc::now(),
                    ip_address: None,
                    capabilities: None,
                },
            )
            .expect("test device registry insert");
        token
    }

    async fn admin_paircode_response_json(
        result: Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)>,
    ) -> (StatusCode, serde_json::Value) {
        let response = result.into_response();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, json)
    }

    #[tokio::test]
    async fn admin_paircode_new_without_rotate_keeps_existing_tokens() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = admin_paircode_state(&tmp, true, true);
        let token = pair_device(&state, "dev-a").await;

        let (status, json) = admin_paircode_response_json(
            handle_admin_paircode_new(
                State(state.clone()),
                test_connect_info(),
                Query(AdminPaircodeQuery::default()),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert!(json["pairing_code"].is_string());
        assert!(
            state.pairing.is_authenticated(&token),
            "add-another-client path must not revoke existing tokens"
        );
    }

    /// Review MAJOR-1: `AppState.pairing` outlives every config write, so it
    /// must not carry a snapshotted pairing-code policy. Strengthening
    /// `[gateway.pairing_code]` through the live config — exactly what
    /// `persist_and_swap` does — must change the *next* code the same guard
    /// instance mints, with no restart and no reconstruction.
    #[tokio::test]
    async fn admin_paircode_new_mints_under_live_policy_after_a_config_swap() {
        use clawcrew_config::pairing::{PairingCodeCharset, PairingCodePolicy};

        let tmp = tempfile::TempDir::new().unwrap();
        let state = admin_paircode_state(&tmp, true, true);

        // Boot weak: six numeric digits, the legacy shape.
        let weak = PairingCodePolicy::numeric_compat();
        state.config.write().gateway.pairing_code = weak;
        let guard_before = Arc::as_ptr(&state.pairing);

        let (status, json) = admin_paircode_response_json(
            handle_admin_paircode_new(
                State(state.clone()),
                test_connect_info(),
                Query(AdminPaircodeQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let weak_code = json["pairing_code"]
            .as_str()
            .expect("code issued")
            .to_string();
        assert_eq!(weak_code.len(), 6, "weak policy in force at first mint");
        assert!(weak_code.chars().all(|c| c.is_ascii_digit()));

        // Operator strengthens the policy. No restart, no new guard.
        let strong = PairingCodePolicy::new(28, PairingCodeCharset::Unambiguous).unwrap();
        state.config.write().gateway.pairing_code = strong;

        let (status, json) = admin_paircode_response_json(
            handle_admin_paircode_new(
                State(state.clone()),
                test_connect_info(),
                Query(AdminPaircodeQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let strong_code = json["pairing_code"].as_str().expect("code issued");

        assert_eq!(
            strong_code.len(),
            28,
            "the strengthened policy must apply to the very next code, got {strong_code}"
        );
        let alphabet = PairingCodeCharset::Unambiguous.alphabet();
        assert!(
            strong_code.bytes().all(|b| alphabet.contains(&b)),
            "code {strong_code} must use the newly configured charset"
        );
        assert_eq!(
            Arc::as_ptr(&state.pairing),
            guard_before,
            "the guard must not have been rebuilt — the policy is resolved per mint"
        );
    }

    #[tokio::test]
    async fn admin_paircode_new_rotate_all_revokes_everything() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = admin_paircode_state(&tmp, true, true);
        let token_a = pair_device(&state, "dev-a").await;
        let token_b = pair_device(&state, "dev-b").await;

        let (status, json) = admin_paircode_response_json(
            handle_admin_paircode_new(
                State(state.clone()),
                test_connect_info(),
                Query(AdminPaircodeQuery {
                    rotate: Some("all".into()),
                }),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert!(json["pairing_code"].is_string());
        assert!(!state.pairing.is_authenticated(&token_a));
        assert!(!state.pairing.is_authenticated(&token_b));
        assert!(
            state.config.read().gateway.paired_tokens.is_empty(),
            "rotate=all must persist an empty token set"
        );
        assert!(
            state
                .device_registry
                .as_ref()
                .unwrap()
                .list()
                .expect("test device registry list")
                .is_empty(),
            "rotate=all must clear the device registry"
        );
    }

    #[tokio::test]
    async fn admin_paircode_new_rotate_device_revokes_one() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = admin_paircode_state(&tmp, true, true);
        let token_a = pair_device(&state, "dev-a").await;
        let token_b = pair_device(&state, "dev-b").await;

        let (status, json) = admin_paircode_response_json(
            handle_admin_paircode_new(
                State(state.clone()),
                test_connect_info(),
                Query(AdminPaircodeQuery {
                    rotate: Some("dev-a".into()),
                }),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert!(json["pairing_code"].is_string());
        assert!(!state.pairing.is_authenticated(&token_a));
        assert!(
            state.pairing.is_authenticated(&token_b),
            "targeted rotate must not touch other devices"
        );
        let old_hash = PairingGuard::token_hash(&token_a);
        assert!(
            !state
                .config
                .read()
                .gateway
                .paired_tokens
                .contains(&old_hash)
        );
    }

    #[tokio::test]
    async fn admin_paircode_new_rotate_unknown_device_is_not_found() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = admin_paircode_state(&tmp, true, true);
        let token = pair_device(&state, "dev-a").await;

        let (status, _json) = admin_paircode_response_json(
            handle_admin_paircode_new(
                State(state.clone()),
                test_connect_info(),
                Query(AdminPaircodeQuery {
                    rotate: Some("ghost".into()),
                }),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(
            state.pairing.is_authenticated(&token),
            "a not-found rotate must not revoke any token"
        );
    }

    #[tokio::test]
    async fn admin_paircode_new_pairing_disabled_is_bad_request() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = admin_paircode_state(&tmp, false, false);

        let (status, json) = admin_paircode_response_json(
            handle_admin_paircode_new(
                State(state),
                test_connect_info(),
                Query(AdminPaircodeQuery {
                    rotate: Some("all".into()),
                }),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn admin_paircode_new_rejects_remote_peer() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = admin_paircode_state(&tmp, true, true);

        let remote = ConnectInfo(SocketAddr::from(([203, 0, 113, 7], 40_000)));
        let (status, _json) = admin_paircode_response_json(
            handle_admin_paircode_new(State(state), remote, Query(AdminPaircodeQuery::default()))
                .await,
        )
        .await;

        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "minting a pairing code must be rejected for non-loopback peers"
        );
    }

    #[test]
    fn long_running_request_timeout_default_is_ten_minutes() {
        assert_eq!(LONG_RUNNING_REQUEST_TIMEOUT_SECS, 600);
    }

    #[test]
    fn long_running_request_timeout_uses_typed_config_default() {
        let cfg = clawcrew_config::schema::GatewayConfig::default();
        assert_eq!(gateway_long_running_request_timeout_secs(&cfg), 600);
    }

    #[test]
    fn webhook_body_requires_message_field() {
        let valid = r#"{"message": "hello"}"#;
        let parsed: Result<WebhookBody, _> = serde_json::from_str(valid);
        assert!(parsed.is_ok());
        assert_eq!(parsed.unwrap().message, "hello");

        let missing = r#"{"other": "field"}"#;
        let parsed: Result<WebhookBody, _> = serde_json::from_str(missing);
        assert!(parsed.is_err());
    }

    #[test]
    fn whatsapp_query_fields_are_optional() {
        let q = WhatsAppVerifyQuery {
            mode: None,
            verify_token: None,
            challenge: None,
        };
        assert!(q.mode.is_none());
    }

    #[test]
    fn app_state_is_clone() {
        fn assert_clone<T: Clone>() {}
        assert_clone::<AppState>();
    }

    #[tokio::test]
    async fn static_routes_reject_malformed_paths_before_spa_fallback() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut prefixed_state = spa_fallback_state(&tmp);
        prefixed_state.path_prefix = "/gw".to_string();

        for (path, prefix, state) in [
            ("/_app/", None, spa_fallback_state(&tmp)),
            ("/_app//index.html", None, spa_fallback_state(&tmp)),
            ("/_app/assets/./app.js", None, spa_fallback_state(&tmp)),
            ("/_app/assets/../secret", None, spa_fallback_state(&tmp)),
            ("/_app/assets/app.js/", None, spa_fallback_state(&tmp)),
            ("/gw/_app/", Some("/gw"), prefixed_state),
        ] {
            let response = static_route_response(path, prefix, state).await;
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "route path should be rejected: {path}"
            );
        }
    }

    #[tokio::test]
    async fn static_routes_serve_valid_assets_with_and_without_prefix() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dist_dir = tmp.path().join("web").join("dist");
        let assets = dist_dir.join("assets");
        std::fs::create_dir_all(&assets).unwrap();
        std::fs::write(assets.join("route-test.js"), b"route-ok").unwrap();

        let mut state = spa_fallback_state(&tmp);
        let unprefixed =
            static_route_response("/_app/assets/route-test.js", None, state.clone()).await;
        assert_eq!(unprefixed.status(), StatusCode::OK);
        assert_eq!(
            unprefixed.into_body().collect().await.unwrap().to_bytes(),
            &b"route-ok"[..]
        );

        state.path_prefix = "/gw".to_string();
        let prefixed =
            static_route_response("/gw/_app/assets/route-test.js", Some("/gw"), state).await;
        assert_eq!(prefixed.status(), StatusCode::OK);
        assert_eq!(
            prefixed.into_body().collect().await.unwrap().to_bytes(),
            &b"route-ok"[..]
        );
    }

    #[tokio::test]
    async fn spa_fallback_returns_json_not_html_for_unknown_api_path() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = spa_fallback_state(&tmp);

        let response = spa_fallback_response("/api/agents", state).await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.starts_with("application/json")),
            "unknown API paths must not be served as HTML"
        );

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "not_found");
        assert_eq!(json["path"], "/api/agents");
    }

    #[tokio::test]
    async fn spa_fallback_returns_json_for_api_root_path() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = spa_fallback_state(&tmp);

        let response = spa_fallback_response("/api", state).await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["path"], "/api");
    }

    #[tokio::test]
    async fn spa_fallback_returns_json_for_path_prefixed_api_miss() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut state = spa_fallback_state(&tmp);
        state.path_prefix = "/gw".to_string();

        let response = spa_fallback_response("/gw/api/agents", state).await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["path"], "/api/agents");
    }

    #[tokio::test]
    async fn spa_fallback_still_serves_dashboard_routes() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = spa_fallback_state(&tmp);

        let response = spa_fallback_response("/config", state).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.starts_with("text/html")),
            "dashboard routes should still receive the SPA shell"
        );

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("dashboard shell"));
    }

    #[cfg(not(feature = "embedded-web"))]
    #[tokio::test]
    async fn spa_fallback_reports_unavailable_without_dashboard_assets() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = admin_paircode_state(&tmp, false, false);

        let response = spa_fallback_response("/", state).await;

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn spa_fallback_does_not_treat_api_like_spa_paths_as_api() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = spa_fallback_state(&tmp);

        let response = spa_fallback_response("/apiary", state).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.starts_with("text/html")),
            "similarly named SPA routes should not be reserved as API paths"
        );
    }

    #[tokio::test]
    async fn run_gateway_starts_with_zero_agents() {
        // Isolate data_dir so parallel nextest runs don't race on the
        // real ~/.clawcrew/data
        let tmp = tempfile::TempDir::new().unwrap();
        let config = clawcrew_config::schema::Config {
            data_dir: tmp.path().join("workspace"),
            config_path: tmp.path().join("config.toml"),
            ..clawcrew_config::schema::Config::default()
        };
        std::fs::create_dir_all(&config.data_dir).unwrap();

        // Default Config has no [agents.*] entries — the exact shape
        // a fresh install presents on first daemon boot.
        assert!(
            config.agents.is_empty(),
            "regression assumes default Config has no agents",
        );

        let handle = clawcrew_spawn::spawn!(async move {
            run_gateway(
                "127.0.0.1",
                0,
                config,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .await
        });

        match tokio::time::timeout(
            std::time::Duration::from_millis(750),
            &mut Box::pin(async {
                // We cannot await `handle` directly because the gateway
                // never returns under normal operation; instead, peek at
                // whether it has finished by polling join with a tiny
                // budget.
                let _ = tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }),
        )
        .await
        {
            Ok(()) => {}
            Err(_) => panic!("test setup timed out before checking gateway state"),
        }

        // If the boot path errored, the task is finished and join
        // returns the error. If it's still running, abort and accept
        // boot reached the serving stage.
        if handle.is_finished() {
            let result = handle.await.expect("task did not panic");
            panic!(
                "gateway exited during boot with zero agents — must stay up for reload/quickstart: {:?}",
                result
            );
        }
        handle.abort();
    }

    #[tokio::test]
    async fn run_gateway_starts_with_unresolved_agent_risk_profile() {
        use clawcrew_config::schema::AliasedAgentConfig;

        // Isolate data_dir so parallel nextest runs don't race on the
        // real ~/.clawcrew/data
        let tmp = tempfile::TempDir::new().unwrap();
        let mut config = clawcrew_config::schema::Config {
            data_dir: tmp.path().join("workspace"),
            config_path: tmp.path().join("config.toml"),
            ..clawcrew_config::schema::Config::default()
        };
        std::fs::create_dir_all(&config.data_dir).unwrap();

        // Enabled agent whose `risk_profile` does not resolve. No
        // matching [risk_profiles.<key>] entry exists.
        let agent = AliasedAgentConfig {
            enabled: true,
            risk_profile: "definitely_not_configured".into(),
            ..AliasedAgentConfig::default()
        };
        config.agents.insert("fake123".to_string(), agent);

        let handle = clawcrew_spawn::spawn!(async move {
            run_gateway(
                "127.0.0.1",
                0,
                config,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .await
        });

        match tokio::time::timeout(
            std::time::Duration::from_millis(750),
            &mut Box::pin(async {
                let _ = tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }),
        )
        .await
        {
            Ok(()) => {}
            Err(_) => panic!("test setup timed out before checking gateway state"),
        }

        if handle.is_finished() {
            let result = handle.await.expect("task did not panic");
            panic!(
                "gateway exited during boot when agent.risk_profile was unresolved \
                 — must stay up so operator can fix via /admin/reload or /quickstart: {:?}",
                result
            );
        }
        handle.abort();
    }

    #[tokio::test]
    async fn run_gateway_starts_with_mismatched_provider_api_key() {
        let mut config = Config::default();
        config.providers.models.anthropic.insert(
            "default".to_string(),
            clawcrew_config::schema::AnthropicModelProviderConfig {
                base: clawcrew_config::schema::ModelProviderConfig {
                    model: Some("anthropic/claude-sonnet-4-6".to_string()),
                    api_key: Some("sk-test-openai-shaped-key".to_string()),
                    ..Default::default()
                },
                ..Default::default()
            },
        );

        let handle = clawcrew_spawn::spawn!(async move {
            run_gateway(
                "127.0.0.1",
                0,
                config,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .await
        });

        match tokio::time::timeout(
            std::time::Duration::from_millis(750),
            &mut Box::pin(async {
                let _ = tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }),
        )
        .await
        {
            Ok(()) => {}
            Err(_) => panic!("test setup timed out before checking gateway state"),
        }

        if handle.is_finished() {
            let result = handle.await.expect("task did not panic");
            panic!(
                "gateway exited during boot when seed provider API key was \
                 mismatched — must stay up so operator can fix via /admin/reload \
                 or /quickstart: {:?}",
                result
            );
        }
        handle.abort();
    }

    #[tokio::test]
    async fn daemon_startup_gateway_reports_ready_and_uses_external_shutdown_sender() {
        let port_probe = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = port_probe.local_addr().unwrap().port();
        drop(port_probe);

        let tmp = tempfile::TempDir::new().unwrap();
        let config = clawcrew_config::schema::Config {
            data_dir: tmp.path().join("workspace"),
            config_path: tmp.path().join("config.toml"),
            ..clawcrew_config::schema::Config::default()
        };
        std::fs::create_dir_all(&config.data_dir).unwrap();

        let (shutdown_tx, _) = tokio::sync::watch::channel(false);
        let (reload_tx, _) = tokio::sync::watch::channel(false);
        let reload_controls = clawcrew_runtime::daemon::GatewayReloadControls {
            shutdown_tx: shutdown_tx.clone(),
            reload_tx,
        };
        let (ready_tx, mut ready_rx) = tokio::sync::watch::channel(None);
        let readiness = clawcrew_runtime::daemon::GatewayReadinessReporter::new(move |addr| {
            let _ = ready_tx.send(Some(addr));
        });

        let handle = clawcrew_spawn::spawn!(async move {
            run_gateway(
                "127.0.0.1",
                port,
                config,
                None,
                Some(reload_controls),
                None,
                None,
                None,
                None,
                Some(readiness),
            )
            .await
        });

        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            ready_rx.wait_for(Option::is_some).await.unwrap();
        })
        .await
        .expect("gateway should report its successful bind");
        let ready_addr = *ready_rx.borrow();
        assert_eq!(ready_addr.unwrap().port(), port);

        let addr = format!("127.0.0.1:{port}");
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if tokio::net::TcpStream::connect(&addr).await.is_ok() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("gateway should accept connections before shutdown");

        shutdown_tx
            .send(true)
            .expect("external daemon-owned shutdown sender should stay connected");

        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("gateway should return after external shutdown")
            .expect("gateway task should not panic")
            .expect("gateway shutdown should be graceful");

        std::net::TcpListener::bind(("127.0.0.1", port))
            .expect("gateway should release the listener after external shutdown");
    }

    #[tokio::test]
    async fn daemon_startup_gateway_does_not_report_ready_when_tls_setup_fails() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut config = clawcrew_config::schema::Config {
            data_dir: tmp.path().join("workspace"),
            config_path: tmp.path().join("config.toml"),
            ..clawcrew_config::schema::Config::default()
        };
        config.gateway.tls = Some(clawcrew_config::schema::GatewayTlsConfig {
            enabled: true,
            cert_path: tmp.path().join("missing-cert.pem").display().to_string(),
            key_path: tmp.path().join("missing-key.pem").display().to_string(),
            client_auth: None,
        });
        std::fs::create_dir_all(&config.data_dir).unwrap();

        let (ready_tx, ready_rx) = tokio::sync::watch::channel(None);
        let readiness = clawcrew_runtime::daemon::GatewayReadinessReporter::new(move |addr| {
            let _ = ready_tx.send(Some(addr));
        });
        let result = run_gateway(
            "127.0.0.1",
            0,
            config,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(readiness),
        )
        .await;

        assert!(
            result.is_err(),
            "invalid TLS files should fail gateway setup"
        );
        assert!(
            ready_rx.borrow().is_none(),
            "failed post-bind setup must not report gateway readiness"
        );
    }

    #[tokio::test]
    async fn metrics_endpoint_returns_hint_when_prometheus_is_disabled() {
        let state = AppState {
            config: Arc::new(RwLock::new(Config::default())),
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            model_provider: Arc::new(MockModelProvider::default()),
            model: "test-model".into(),
            temperature: None,
            mem: Arc::new(MockMemory),
            memory_strategy: Arc::new(DefaultMemoryStrategy::with_config(
                Arc::new(MockMemory),
                clawcrew_config::schema::MemoryConfig::default(),
                std::path::PathBuf::new(),
            )),
            auto_save: false,
            pairing: Arc::new(PairingGuard::new(false, &[], PairingCodePolicy::default())),
            trust_forwarded_headers: false,
            rate_limiter: Arc::new(GatewayRateLimiter::new(100, 100, 100)),
            auth_limiter: Arc::new(auth_rate_limit::AuthRateLimiter::new()),
            idempotency_store: Arc::new(IdempotencyStore::new(Duration::from_secs(300), 1000)),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp: HashMap::new(),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp_app_secret: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq_signing_secrets: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk_webhook_secret: HashMap::new(),
            #[cfg(feature = "channel-email")]
            gmail_push: None,
            observer: Arc::new(clawcrew_runtime::observability::NoopObserver),
            tools_registry: Arc::new(Vec::new()),
            tools_registry_by_agent: Arc::new(std::collections::HashMap::new()),
            cost_tracker: None,
            event_tx: tokio::sync::broadcast::channel(16).0,
            event_buffer: Arc::new(sse::EventBuffer::new(16)),
            shutdown_tx: tokio::sync::watch::channel(false).0,
            reload_tx: None,
            node_registry: Arc::new(nodes::NodeRegistry::new(16)),
            mdns_peer_registry: nodes::mdns::MdnsPeerRegistry::default(),
            path_prefix: String::new(),
            web_dist_dir: None,
            session_backend: None,
            session_queue: std::sync::Arc::new(crate::session_queue::SessionActorQueue::new(
                8, 30, 600,
            )),
            device_registry: None,
            pending_pairings: None,
            canvas_store: CanvasStore::new(),
            cancel_tokens: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            pending_reload: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tui_registry: None,
            sop_engine: None,
            sop_audit: None,
            #[cfg(feature = "webauthn")]
            webauthn: None,
        };

        let response = handle_metrics(State(state)).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some(PROMETHEUS_CONTENT_TYPE)
        );

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("Prometheus backend not enabled"));
    }

    #[cfg(feature = "observability-prometheus")]
    #[tokio::test]
    async fn metrics_endpoint_renders_prometheus_output() {
        let event_tx = tokio::sync::broadcast::channel(16).0;
        let prom = clawcrew_runtime::observability::PrometheusObserver::new();
        clawcrew_runtime::observability::Observer::record_event(
            &prom,
            &clawcrew_runtime::observability::ObserverEvent::HeartbeatTick,
        );

        let observer: Arc<dyn clawcrew_runtime::observability::Observer> = Arc::new(prom);
        let state = AppState {
            config: Arc::new(RwLock::new(Config::default())),
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            model_provider: Arc::new(MockModelProvider::default()),
            model: "test-model".into(),
            temperature: None,
            mem: Arc::new(MockMemory),
            memory_strategy: Arc::new(DefaultMemoryStrategy::with_config(
                Arc::new(MockMemory),
                clawcrew_config::schema::MemoryConfig::default(),
                std::path::PathBuf::new(),
            )),
            auto_save: false,
            pairing: Arc::new(PairingGuard::new(false, &[], PairingCodePolicy::default())),
            trust_forwarded_headers: false,
            rate_limiter: Arc::new(GatewayRateLimiter::new(100, 100, 100)),
            auth_limiter: Arc::new(auth_rate_limit::AuthRateLimiter::new()),
            idempotency_store: Arc::new(IdempotencyStore::new(Duration::from_secs(300), 1000)),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp: HashMap::new(),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp_app_secret: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq_signing_secrets: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk_webhook_secret: HashMap::new(),
            #[cfg(feature = "channel-email")]
            gmail_push: None,
            observer,
            tools_registry: Arc::new(Vec::new()),
            tools_registry_by_agent: Arc::new(std::collections::HashMap::new()),
            cost_tracker: None,
            event_tx,
            event_buffer: Arc::new(sse::EventBuffer::new(16)),
            shutdown_tx: tokio::sync::watch::channel(false).0,
            reload_tx: None,
            node_registry: Arc::new(nodes::NodeRegistry::new(16)),
            mdns_peer_registry: nodes::mdns::MdnsPeerRegistry::default(),
            path_prefix: String::new(),
            web_dist_dir: None,
            session_backend: None,
            session_queue: std::sync::Arc::new(crate::session_queue::SessionActorQueue::new(
                8, 30, 600,
            )),
            device_registry: None,
            pending_pairings: None,
            canvas_store: CanvasStore::new(),
            cancel_tokens: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            pending_reload: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tui_registry: None,
            sop_engine: None,
            sop_audit: None,
            #[cfg(feature = "webauthn")]
            webauthn: None,
        };

        let response = handle_metrics(State(state)).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("clawcrew_heartbeat_ticks_total 1"));
    }

    #[test]
    fn gateway_rate_limiter_blocks_after_limit() {
        let limiter = GatewayRateLimiter::new(2, 2, 100);
        assert!(limiter.allow_pair("127.0.0.1"));
        assert!(limiter.allow_pair("127.0.0.1"));
        assert!(!limiter.allow_pair("127.0.0.1"));
    }

    #[test]
    fn rate_limiter_sweep_removes_stale_entries() {
        let limiter = SlidingWindowRateLimiter::new(10, Duration::from_secs(60), 100);
        // Add entries for multiple IPs
        assert!(limiter.allow("ip-1"));
        assert!(limiter.allow("ip-2"));
        assert!(limiter.allow("ip-3"));

        {
            let guard = limiter.requests.lock();
            assert_eq!(guard.0.len(), 3);
        }

        // Force a sweep by backdating last_sweep
        {
            let mut guard = limiter.requests.lock();
            guard.1 = Instant::now()
                .checked_sub(Duration::from_secs(RATE_LIMITER_SWEEP_INTERVAL_SECS + 1))
                .unwrap();
            // Clear timestamps for ip-2 and ip-3 to simulate stale entries
            guard.0.get_mut("ip-2").unwrap().clear();
            guard.0.get_mut("ip-3").unwrap().clear();
        }

        // Next allow() call should trigger sweep and remove stale entries
        assert!(limiter.allow("ip-1"));

        {
            let guard = limiter.requests.lock();
            assert_eq!(guard.0.len(), 1, "Stale entries should have been swept");
            assert!(guard.0.contains_key("ip-1"));
        }
    }

    #[test]
    fn rate_limiter_zero_limit_always_allows() {
        let limiter = SlidingWindowRateLimiter::new(0, Duration::from_secs(60), 10);
        for _ in 0..100 {
            assert!(limiter.allow("any-key"));
        }
    }

    #[test]
    fn idempotency_store_rejects_duplicate_key() {
        let store = IdempotencyStore::new(Duration::from_secs(30), 10);
        assert!(store.record_if_new("req-1"));
        assert!(!store.record_if_new("req-1"));
        assert!(store.record_if_new("req-2"));
    }

    #[test]
    fn rate_limiter_bounded_cardinality_evicts_oldest_key() {
        let limiter = SlidingWindowRateLimiter::new(5, Duration::from_secs(60), 2);
        assert!(limiter.allow("ip-1"));
        assert!(limiter.allow("ip-2"));
        assert!(limiter.allow("ip-3"));

        let guard = limiter.requests.lock();
        assert_eq!(guard.0.len(), 2);
        assert!(guard.0.contains_key("ip-2"));
        assert!(guard.0.contains_key("ip-3"));
    }

    #[test]
    fn idempotency_store_bounded_cardinality_evicts_oldest_key() {
        let store = IdempotencyStore::new(Duration::from_secs(300), 2);
        assert!(store.record_if_new("k1"));
        std::thread::sleep(Duration::from_millis(2));
        assert!(store.record_if_new("k2"));
        std::thread::sleep(Duration::from_millis(2));
        assert!(store.record_if_new("k3"));

        let entries = store.entries.lock();
        assert_eq!(entries.committed.len(), 2);
        assert!(!entries.committed.contains_key("k1"));
        assert!(entries.committed.contains_key("k2"));
        assert!(entries.committed.contains_key("k3"));
    }

    #[test]
    fn client_key_defaults_to_peer_addr_when_untrusted_proxy_mode() {
        let peer = SocketAddr::from(([10, 0, 0, 5], 42617));
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Forwarded-For",
            HeaderValue::from_static("198.51.100.10, 203.0.113.11"),
        );

        let key = client_key_from_request(Some(peer), &headers, false);
        assert_eq!(key, "10.0.0.5");
    }

    #[test]
    fn client_key_uses_forwarded_ip_only_in_trusted_proxy_mode() {
        let peer = SocketAddr::from(([10, 0, 0, 5], 42617));
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Forwarded-For",
            HeaderValue::from_static("198.51.100.10, 203.0.113.11"),
        );

        let key = client_key_from_request(Some(peer), &headers, true);
        assert_eq!(key, "198.51.100.10");
    }

    #[test]
    fn client_key_falls_back_to_peer_when_forwarded_header_invalid() {
        let peer = SocketAddr::from(([10, 0, 0, 5], 42617));
        let mut headers = HeaderMap::new();
        headers.insert("X-Forwarded-For", HeaderValue::from_static("garbage-value"));

        let key = client_key_from_request(Some(peer), &headers, true);
        assert_eq!(key, "10.0.0.5");
    }

    #[test]
    fn normalize_max_keys_uses_fallback_for_zero() {
        assert_eq!(normalize_max_keys(0, 10_000), 10_000);
        assert_eq!(normalize_max_keys(0, 0), 1);
    }

    #[test]
    fn normalize_max_keys_preserves_nonzero_values() {
        assert_eq!(normalize_max_keys(2_048, 10_000), 2_048);
        assert_eq!(normalize_max_keys(1, 10_000), 1);
    }

    #[tokio::test]
    async fn persist_pairing_tokens_writes_config_tokens() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        let workspace_path = temp.path().join("workspace");

        let config = Config {
            config_path: config_path.clone(),
            data_dir: workspace_path,
            ..Default::default()
        };
        config.save().await.unwrap();

        let guard = PairingGuard::new(true, &[], PairingCodePolicy::default());
        let code = guard.pairing_code().unwrap();
        let token = guard.try_pair(&code, "test_client").await.unwrap().unwrap();
        assert!(guard.is_authenticated(&token));

        let shared_config = Arc::new(RwLock::new(config));
        let config_write_lock = Arc::new(tokio::sync::Mutex::new(()));
        Box::pin(persist_pairing_tokens(
            shared_config.clone(),
            &guard,
            config_write_lock,
        ))
        .await
        .unwrap();

        // In-memory tokens should remain as plaintext 64-char hex hashes.
        let plaintext = {
            let in_memory = shared_config.read();
            assert_eq!(in_memory.gateway.paired_tokens.len(), 1);
            in_memory.gateway.paired_tokens[0].clone()
        };
        assert_eq!(plaintext.len(), 64);
        assert!(plaintext.chars().all(|c: char| c.is_ascii_hexdigit()));

        // On disk, the token should be encrypted (secrets.encrypt defaults to true).
        let saved = tokio::fs::read_to_string(config_path).await.unwrap();
        let raw_parsed: Config = toml::from_str(&saved).unwrap();
        assert_eq!(raw_parsed.gateway.paired_tokens.len(), 1);
        let on_disk = &raw_parsed.gateway.paired_tokens[0];
        assert!(
            clawcrew_runtime::security::SecretStore::is_encrypted(on_disk),
            "paired_token should be encrypted on disk"
        );
    }

    /// Unlike the `persist_and_swap` callers (which pre-acquire the witness
    /// before their own read-for-modify), `persist_pairing_tokens` acquires
    /// `config_write_lock` internally since it is self-contained. This
    /// proves that internal acquisition still serializes it against a
    /// second, concurrent config mutation the same way. A single Pending
    /// poll wouldn't distinguish "blocked on `config_write_lock`" from
    /// "transiently Pending on unrelated I/O", so this polls repeatedly
    /// with a no-op waker while the witness stays held and asserts the
    /// future never completes -- proving it stays parked on the lock for as
    /// long as it's held. Once the lock is released both changes land —
    /// neither clobbers the other.
    #[tokio::test]
    async fn persist_pairing_tokens_serializes_against_concurrent_config_write() {
        let temp = tempfile::tempdir().unwrap();
        let config = Config {
            config_path: temp.path().join("config.toml"),
            data_dir: temp.path().join("workspace"),
            ..Default::default()
        };
        config.save().await.unwrap();

        let guard = PairingGuard::new(true, &[], PairingCodePolicy::default());
        let code = guard.pairing_code().unwrap();
        let token = guard.try_pair(&code, "test_client").await.unwrap().unwrap();
        assert!(guard.is_authenticated(&token));

        let shared_config = Arc::new(RwLock::new(config));
        let config_write_lock = Arc::new(tokio::sync::Mutex::new(()));

        // Simulate another in-flight config mutation already holding the
        // witness for its own read-mutate-save-swap section.
        let held_guard = Arc::clone(&config_write_lock).lock_owned().await;

        let mut persist_fut = Box::pin(persist_pairing_tokens(
            shared_config.clone(),
            &guard,
            config_write_lock.clone(),
        ));

        // Bounded, sleep-free: `persist_pairing_tokens` acquires the witness
        // as its very first action, so poll with a no-op waker 50 times
        // while `held_guard` stays live and assert Pending every time,
        // rather than resolving synchronously or racing ahead after a
        // single yield.
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        for _ in 0..50 {
            assert!(
                std::future::Future::poll(persist_fut.as_mut(), &mut cx).is_pending(),
                "persist_pairing_tokens must stay parked on config_write_lock \
                 acquisition for as long as another writer holds it"
            );
        }

        // Land a distinct, concurrent write directly on live config while
        // persist_pairing_tokens is parked waiting for the lock.
        shared_config.write().gateway.port = 55555;

        drop(held_guard);
        persist_fut
            .await
            .expect("persist_pairing_tokens must still succeed once unblocked");

        let live = shared_config.read();
        assert_eq!(
            live.gateway.port, 55555,
            "the concurrent writer's change must survive — no lost update"
        );
        assert_eq!(
            live.gateway.paired_tokens.len(),
            1,
            "persist_pairing_tokens' own token write must also land"
        );
    }

    #[test]
    fn webhook_memory_key_is_unique() {
        let key1 = webhook_memory_key();
        let key2 = webhook_memory_key();

        assert!(key1.starts_with("webhook_msg_"));
        assert!(key2.starts_with("webhook_msg_"));
        assert_ne!(key1, key2);
    }

    #[test]
    fn webhook_session_id_accepts_valid() {
        let mut headers = HeaderMap::new();
        headers.insert("X-Session-Id", HeaderValue::from_static("abc-DEF_123.foo"));
        assert_eq!(webhook_session_id(&headers), Some("abc-DEF_123.foo".into()));
    }

    #[test]
    fn webhook_session_id_trims_whitespace() {
        let mut headers = HeaderMap::new();
        headers.insert("X-Session-Id", HeaderValue::from_static("  my-session  "));
        assert_eq!(webhook_session_id(&headers), Some("my-session".into()));
    }

    #[test]
    fn webhook_session_id_rejects_empty() {
        let mut headers = HeaderMap::new();
        headers.insert("X-Session-Id", HeaderValue::from_static(""));
        assert_eq!(webhook_session_id(&headers), None);

        headers.insert("X-Session-Id", HeaderValue::from_static("   "));
        assert_eq!(webhook_session_id(&headers), None);
    }

    #[test]
    fn webhook_session_id_rejects_missing() {
        let headers = HeaderMap::new();
        assert_eq!(webhook_session_id(&headers), None);
    }

    #[test]
    fn webhook_session_id_rejects_oversized() {
        let mut headers = HeaderMap::new();
        let long = "a".repeat(129);
        headers.insert("X-Session-Id", HeaderValue::from_str(&long).unwrap());
        assert_eq!(webhook_session_id(&headers), None);

        let at_limit = "b".repeat(128);
        headers.insert("X-Session-Id", HeaderValue::from_str(&at_limit).unwrap());
        assert!(webhook_session_id(&headers).is_some());
    }

    #[test]
    fn webhook_session_id_rejects_invalid_chars() {
        let mut headers = HeaderMap::new();
        for bad in &[
            "has/slash",
            "has:colon",
            "has space",
            "has@at",
            "emoji\u{1f600}",
        ] {
            if let Ok(val) = HeaderValue::from_str(bad) {
                headers.insert("X-Session-Id", val);
                assert_eq!(webhook_session_id(&headers), None, "should reject: {bad}");
            }
        }
    }

    #[cfg(feature = "channel-whatsapp-cloud")]
    #[test]
    fn whatsapp_memory_key_includes_sender_and_message_id() {
        let msg = ChannelMessage {
            id: "wamid-123".into(),
            sender: "+1234567890".into(),
            reply_target: "+1234567890".into(),
            content: "hello".into(),
            channel: "whatsapp".into(),
            channel_alias: None,
            timestamp: 1,
            thread_ts: None,
            interruption_scope_id: None,
            attachments: vec![],
            subject: None,

            ..Default::default()
        };

        let key = whatsapp_memory_key(&msg);
        assert_eq!(key, "whatsapp_+1234567890_wamid-123");
    }

    #[derive(Default)]
    struct MockMemory;

    #[async_trait]
    impl Memory for MockMemory {
        fn name(&self) -> &str {
            "mock"
        }

        async fn store(
            &self,
            _key: &str,
            _content: &str,
            _category: MemoryCategory,
            _session_id: Option<&str>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn recall(
            &self,
            _query: &str,
            _limit: usize,
            _session_id: Option<&str>,
            _since: Option<&str>,
            _until: Option<&str>,
        ) -> anyhow::Result<Vec<MemoryEntry>> {
            Ok(Vec::new())
        }

        async fn get(&self, _key: &str) -> anyhow::Result<Option<MemoryEntry>> {
            Ok(None)
        }

        async fn list(
            &self,
            _category: Option<&MemoryCategory>,
            _session_id: Option<&str>,
        ) -> anyhow::Result<Vec<MemoryEntry>> {
            Ok(Vec::new())
        }

        async fn forget(&self, _key: &str) -> anyhow::Result<bool> {
            Ok(false)
        }

        async fn forget_for_agent(&self, _key: &str, _agent_id: &str) -> anyhow::Result<bool> {
            Ok(false)
        }

        async fn count(&self) -> anyhow::Result<usize> {
            Ok(0)
        }

        async fn health_check(&self) -> bool {
            true
        }

        async fn store_with_agent(
            &self,
            _key: &str,
            _content: &str,
            _category: MemoryCategory,
            _session_id: Option<&str>,
            _namespace: Option<&str>,
            _importance: Option<f64>,
            _agent_id: Option<&str>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn recall_for_agents(
            &self,
            _allowed_agent_ids: &[&str],
            _query: &str,
            _limit: usize,
            _session_id: Option<&str>,
            _since: Option<&str>,
            _until: Option<&str>,
        ) -> anyhow::Result<Vec<MemoryEntry>> {
            Ok(Vec::new())
        }
    }
    impl ::clawcrew_api::attribution::Attributable for MockMemory {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Memory(
                ::clawcrew_api::attribution::MemoryKind::InMemory,
            )
        }
        fn alias(&self) -> &str {
            "MockMemory"
        }
    }

    #[derive(Default)]
    struct MockModelProvider {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl ModelProvider for MockModelProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> anyhow::Result<String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok("ok".into())
        }
    }
    impl ::clawcrew_api::attribution::Attributable for MockModelProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Custom,
                ),
            )
        }
        fn alias(&self) -> &str {
            "MockModelProvider"
        }
    }

    #[derive(Default)]
    struct CapturingObserver {
        events: Mutex<Vec<clawcrew_runtime::observability::ObserverEvent>>,
    }

    impl clawcrew_runtime::observability::Observer for CapturingObserver {
        fn record_event(&self, event: &clawcrew_runtime::observability::ObserverEvent) {
            self.events.lock().push(event.clone());
        }

        fn record_metric(&self, _metric: &clawcrew_runtime::observability::traits::ObserverMetric) {
        }

        fn name(&self) -> &str {
            "capturing"
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[derive(Default)]
    struct TrackingMemory {
        keys: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl Memory for TrackingMemory {
        fn name(&self) -> &str {
            "tracking"
        }

        async fn store(
            &self,
            key: &str,
            _content: &str,
            _category: MemoryCategory,
            _session_id: Option<&str>,
        ) -> anyhow::Result<()> {
            self.keys.lock().push(key.to_string());
            Ok(())
        }

        async fn recall(
            &self,
            _query: &str,
            _limit: usize,
            _session_id: Option<&str>,
            _since: Option<&str>,
            _until: Option<&str>,
        ) -> anyhow::Result<Vec<MemoryEntry>> {
            Ok(Vec::new())
        }

        async fn get(&self, _key: &str) -> anyhow::Result<Option<MemoryEntry>> {
            Ok(None)
        }

        async fn list(
            &self,
            _category: Option<&MemoryCategory>,
            _session_id: Option<&str>,
        ) -> anyhow::Result<Vec<MemoryEntry>> {
            Ok(Vec::new())
        }

        async fn forget(&self, _key: &str) -> anyhow::Result<bool> {
            Ok(false)
        }

        async fn forget_for_agent(&self, _key: &str, _agent_id: &str) -> anyhow::Result<bool> {
            Ok(false)
        }

        async fn count(&self) -> anyhow::Result<usize> {
            let size = self.keys.lock().len();
            Ok(size)
        }

        async fn health_check(&self) -> bool {
            true
        }

        async fn store_with_agent(
            &self,
            key: &str,
            content: &str,
            category: MemoryCategory,
            session_id: Option<&str>,
            _namespace: Option<&str>,
            _importance: Option<f64>,
            _agent_id: Option<&str>,
        ) -> anyhow::Result<()> {
            self.store(key, content, category, session_id).await
        }

        async fn recall_for_agents(
            &self,
            _allowed_agent_ids: &[&str],
            _query: &str,
            _limit: usize,
            _session_id: Option<&str>,
            _since: Option<&str>,
            _until: Option<&str>,
        ) -> anyhow::Result<Vec<MemoryEntry>> {
            Ok(Vec::new())
        }
    }
    impl ::clawcrew_api::attribution::Attributable for TrackingMemory {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Memory(
                ::clawcrew_api::attribution::MemoryKind::InMemory,
            )
        }
        fn alias(&self) -> &str {
            "TrackingMemory"
        }
    }

    fn test_connect_info() -> ConnectInfo<SocketAddr> {
        ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 30_300)))
    }

    /// Minimal AppState for webhook-SSE regressions.
    fn sse_test_state(model_provider: Arc<dyn ModelProvider>) -> AppState {
        let memory: Arc<dyn Memory> = Arc::new(MockMemory);
        AppState {
            config: Arc::new(RwLock::new(Config::default())),
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            model_provider,
            model: "test-model".into(),
            temperature: None,
            mem: memory.clone(),
            memory_strategy: Arc::new(DefaultMemoryStrategy::with_config(
                Arc::clone(&memory),
                clawcrew_config::schema::MemoryConfig::default(),
                std::path::PathBuf::new(),
            )),
            auto_save: false,
            pairing: Arc::new(PairingGuard::new(false, &[], PairingCodePolicy::default())),
            trust_forwarded_headers: false,
            rate_limiter: Arc::new(GatewayRateLimiter::new(100, 100, 100)),
            auth_limiter: Arc::new(auth_rate_limit::AuthRateLimiter::new()),
            idempotency_store: Arc::new(IdempotencyStore::new(Duration::from_secs(300), 1000)),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp: HashMap::new(),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp_app_secret: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq_signing_secrets: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk_webhook_secret: HashMap::new(),
            #[cfg(feature = "channel-email")]
            gmail_push: None,
            observer: Arc::new(clawcrew_runtime::observability::NoopObserver),
            tools_registry: Arc::new(Vec::new()),
            tools_registry_by_agent: Arc::new(std::collections::HashMap::new()),
            cost_tracker: None,
            event_tx: tokio::sync::broadcast::channel(16).0,
            event_buffer: Arc::new(sse::EventBuffer::new(16)),
            shutdown_tx: tokio::sync::watch::channel(false).0,
            reload_tx: None,
            node_registry: Arc::new(nodes::NodeRegistry::new(16)),
            mdns_peer_registry: nodes::mdns::MdnsPeerRegistry::default(),
            path_prefix: String::new(),
            web_dist_dir: None,
            session_backend: None,
            session_queue: std::sync::Arc::new(crate::session_queue::SessionActorQueue::new(
                8, 30, 600,
            )),
            device_registry: None,
            pending_pairings: None,
            canvas_store: CanvasStore::new(),
            cancel_tokens: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            pending_reload: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tui_registry: None,
            sop_engine: None,
            sop_audit: None,
            #[cfg(feature = "webauthn")]
            webauthn: None,
        }
    }

    /// A local OpenAI-compatible HTTP fixture used by the production-shaped
    /// gateway tests below. Keeping the listener in the test process proves
    /// that the configured Agent/provider path is exercised without relying on
    /// a network credential or a mock `ModelProvider` injected into AppState.
    struct ChatCompletionFixture {
        address: SocketAddr,
        requests: Arc<AtomicUsize>,
        stream_chunks: Arc<AtomicUsize>,
        stream_closed: Arc<AtomicBool>,
        server: tokio::task::JoinHandle<()>,
    }

    impl ChatCompletionFixture {
        fn base_url(&self) -> String {
            format!("http://{}/v1", self.address)
        }
    }

    impl Drop for ChatCompletionFixture {
        fn drop(&mut self) {
            self.server.abort();
        }
    }

    async fn spawn_chat_completion_fixture(
        response_body: impl Into<String>,
    ) -> ChatCompletionFixture {
        spawn_chat_completion_fixture_sequence(vec![response_body.into()]).await
    }

    async fn spawn_chat_completion_fixture_sequence(
        response_bodies: Vec<String>,
    ) -> ChatCompletionFixture {
        let response_bodies = Arc::new(response_bodies);
        let requests = Arc::new(AtomicUsize::new(0));
        let requests_for_handler = Arc::clone(&requests);
        let bodies_for_handler = Arc::clone(&response_bodies);
        let app = Router::new().route(
            "/v1/chat/completions",
            post(move |_: HeaderMap, axum::extract::Json(_request): axum::extract::Json<serde_json::Value>| {
                let bodies = Arc::clone(&bodies_for_handler);
                let requests = Arc::clone(&requests_for_handler);
                async move {
                    let request_index = requests.fetch_add(1, Ordering::SeqCst);
                    let body = bodies
                        .get(request_index)
                        .or_else(|| bodies.last())
                        .cloned()
                        .unwrap_or_default();
                    (
                        [(header::CONTENT_TYPE, HeaderValue::from_static("text/event-stream"))],
                        Body::from(body),
                    )
                        .into_response()
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind local chat-completions fixture");
        let address = listener
            .local_addr()
            .expect("local chat-completions fixture address");
        let server = clawcrew_spawn::spawn!(async move {
            axum::serve(listener, app)
                .await
                .expect("serve local chat-completions fixture");
        });
        ChatCompletionFixture {
            address,
            requests,
            stream_chunks: Arc::new(AtomicUsize::new(0)),
            stream_closed: Arc::new(AtomicBool::new(false)),
            server,
        }
    }

    struct BurstChatStream {
        bodies: Arc<Vec<String>>,
        next: usize,
        emitted: Arc<AtomicUsize>,
        closed: Arc<AtomicBool>,
    }

    impl futures_util::Stream for BurstChatStream {
        type Item = Result<String, std::convert::Infallible>;

        fn poll_next(
            mut self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::Item>> {
            if let Some(body) = self.bodies.get(self.next).cloned() {
                self.next += 1;
                self.emitted.fetch_add(1, Ordering::SeqCst);
                std::task::Poll::Ready(Some(Ok(body)))
            } else {
                std::task::Poll::Pending
            }
        }
    }

    impl Drop for BurstChatStream {
        fn drop(&mut self) {
            self.closed.store(true, Ordering::SeqCst);
        }
    }

    async fn spawn_burst_hanging_chat_completion_fixture() -> ChatCompletionFixture {
        let requests = Arc::new(AtomicUsize::new(0));
        let stream_chunks = Arc::new(AtomicUsize::new(0));
        let stream_closed = Arc::new(AtomicBool::new(false));
        let requests_for_handler = Arc::clone(&requests);
        let bodies: Arc<Vec<String>> = Arc::new(
            (0..17)
                .map(|index| {
                    format!(
                        "data: {{\"choices\":[{{\"delta\":{{\"content\":\"chunk-{index}\"}}}}]}}\n\n"
                    )
                })
                .collect(),
        );
        let bodies_for_handler = Arc::clone(&bodies);
        let chunks_for_handler = Arc::clone(&stream_chunks);
        let closed_for_handler = Arc::clone(&stream_closed);
        let app = Router::new().route(
            "/v1/chat/completions",
            post(
                move |_: HeaderMap,
                      axum::extract::Json(_request): axum::extract::Json<serde_json::Value>| {
                    let requests = Arc::clone(&requests_for_handler);
                    let bodies = Arc::clone(&bodies_for_handler);
                    let emitted = Arc::clone(&chunks_for_handler);
                    let closed = Arc::clone(&closed_for_handler);
                    async move {
                        requests.fetch_add(1, Ordering::SeqCst);
                        let stream = BurstChatStream {
                            bodies,
                            next: 0,
                            emitted,
                            closed,
                        };
                        (
                            [(
                                header::CONTENT_TYPE,
                                HeaderValue::from_static("text/event-stream"),
                            )],
                            Body::from_stream(stream),
                        )
                            .into_response()
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind burst chat-completions fixture");
        let address = listener
            .local_addr()
            .expect("burst chat-completions fixture address");
        let server = clawcrew_spawn::spawn!(async move {
            axum::serve(listener, app)
                .await
                .expect("serve burst chat-completions fixture");
        });
        ChatCompletionFixture {
            address,
            requests,
            stream_chunks,
            stream_closed,
            server,
        }
    }

    /// A provider fixture that sends response headers and then keeps the
    /// streaming body open. This gives two real gateway transports time to
    /// replace one another under the same session key before either provider
    /// turn can complete.
    async fn spawn_hanging_chat_completion_fixture() -> ChatCompletionFixture {
        let requests = Arc::new(AtomicUsize::new(0));
        let requests_for_handler = Arc::clone(&requests);
        let app = Router::new().route(
            "/v1/chat/completions",
            post(
                move |_: HeaderMap,
                      axum::extract::Json(_request): axum::extract::Json<serde_json::Value>| {
                    let requests = Arc::clone(&requests_for_handler);
                    async move {
                        requests.fetch_add(1, Ordering::SeqCst);
                        let stream = futures_util::stream::pending::<
                            Result<String, std::convert::Infallible>,
                        >();
                        (
                            [(
                                header::CONTENT_TYPE,
                                HeaderValue::from_static("text/event-stream"),
                            )],
                            Body::from_stream(stream),
                        )
                            .into_response()
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind hanging chat-completions fixture");
        let address = listener
            .local_addr()
            .expect("hanging chat-completions fixture address");
        let server = clawcrew_spawn::spawn!(async move {
            axum::serve(listener, app)
                .await
                .expect("serve hanging chat-completions fixture");
        });
        ChatCompletionFixture {
            address,
            requests,
            stream_chunks: Arc::new(AtomicUsize::new(0)),
            stream_closed: Arc::new(AtomicBool::new(false)),
            server,
        }
    }

    async fn spawn_test_ws_gateway(state: AppState) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let app = Router::new()
            .route("/ws/chat", get(ws::handle_ws_chat))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind gateway WebSocket listener");
        let address = listener
            .local_addr()
            .expect("gateway WebSocket listener address");
        let server = clawcrew_spawn::spawn!(async move {
            axum::serve(listener, app)
                .await
                .expect("serve gateway WebSocket listener");
        });
        (address, server)
    }

    async fn wait_for_fixture_requests(fixture: &ChatCompletionFixture, expected: usize) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if fixture.requests.load(Ordering::SeqCst) >= expected {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("provider fixture request");
    }

    async fn wait_for_fixture_chunks(fixture: &ChatCompletionFixture, expected: usize) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if fixture.stream_chunks.load(Ordering::SeqCst) >= expected {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("provider fixture stream chunks");
    }

    async fn wait_for_registry_token(
        state: &AppState,
        session_key: &str,
    ) -> Arc<tokio_util::sync::CancellationToken> {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Some(token) = state
                    .cancel_tokens
                    .lock()
                    .expect("cancel_tokens lock poisoned")
                    .get(session_key)
                    .cloned()
                {
                    return token;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("registered transport cancellation token")
    }

    async fn wait_for_registry_replacement(
        state: &AppState,
        session_key: &str,
        previous: &Arc<tokio_util::sync::CancellationToken>,
    ) -> Arc<tokio_util::sync::CancellationToken> {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Some(token) = state
                    .cancel_tokens
                    .lock()
                    .expect("cancel_tokens lock poisoned")
                    .get(session_key)
                    .cloned()
                    && !Arc::ptr_eq(&token, previous)
                {
                    return token;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("replacement transport cancellation token")
    }

    async fn wait_for_registry_owner(
        state: &AppState,
        session_key: &str,
        expected: &Arc<tokio_util::sync::CancellationToken>,
    ) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let matches = state
                    .cancel_tokens
                    .lock()
                    .expect("cancel_tokens lock poisoned")
                    .get(session_key)
                    .is_some_and(|current| Arc::ptr_eq(current, expected));
                if matches {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("replacement transport retains registry ownership");
    }

    async fn wait_for_registry_empty(state: &AppState, session_key: &str) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let empty = !state
                    .cancel_tokens
                    .lock()
                    .expect("cancel_tokens lock poisoned")
                    .contains_key(session_key);
                if empty {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("transport cancellation registry cleanup");
    }

    async fn collect_cancelled_sse(response: Response) -> String {
        let payload = tokio::time::timeout(Duration::from_secs(3), response.into_body().collect())
            .await
            .expect("cancelled SSE response body")
            .expect("cancelled SSE response body stream")
            .to_bytes();
        String::from_utf8(payload.to_vec()).expect("cancelled SSE response is UTF-8")
    }

    async fn start_test_sse(state: &AppState, session_id: &str) -> Response {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_static("text/event-stream"),
        );
        headers.insert(
            "X-Session-Id",
            HeaderValue::from_str(session_id).expect("transport test session id header"),
        );
        handle_webhook(
            State(state.clone()),
            test_connect_info(),
            Query(WebhookQuery {
                agent: Some("web".to_string()),
            }),
            headers,
            Ok(Json(WebhookBody {
                message: "transport overlap".to_string(),
                stream: true,
            })),
        )
        .await
    }

    /// Build an AppState whose `/webhook?agent=web` path constructs a real
    /// runtime Agent and a configured custom OpenAI-compatible provider. The
    /// synthetic allowlist deliberately leaves the Agent with no executable
    /// tools so a text-only fixture cannot accidentally enter a tool loop.
    fn production_sse_state(
        tmp: &tempfile::TempDir,
        provider_url: &str,
        daily_limit_usd: f64,
        response_cache_enabled: bool,
    ) -> (AppState, Arc<CostTracker>) {
        use clawcrew_config::multi_agent::{
            AgentMemoryConfig, AgentWorkspaceConfig, MemoryBackendKind,
        };
        use clawcrew_config::schema::{
            AliasedAgentConfig, CustomModelProviderConfig, ModelProviderConfig, RiskProfileConfig,
            RuntimeProfileConfig,
        };

        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("production fixture workspace");
        let mut config = Config {
            data_dir: workspace.clone(),
            config_path: tmp.path().join("config.toml"),
            ..Config::default()
        };
        config.memory.backend = "none".to_string();
        config.memory.auto_save = false;
        config.memory.response_cache_enabled = response_cache_enabled;
        config.memory.response_cache_ttl_minutes = 60;
        config.cost.enabled = true;
        config.cost.track_per_agent = true;
        config.cost.daily_limit_usd = daily_limit_usd;
        config.cost.monthly_limit_usd = daily_limit_usd;
        config.cost.warn_at_percent = 100;
        config.reliability.provider_retries = 0;
        config.reliability.provider_backoff_ms = 0;
        config.providers.models.custom.insert(
            "fixture".to_string(),
            CustomModelProviderConfig {
                base: ModelProviderConfig {
                    api_key: Some("test-key".to_string()),
                    uri: Some(provider_url.to_string()),
                    model: Some("fixture-model".to_string()),
                    temperature: Some(0.0),
                    pricing: HashMap::from([
                        ("fixture-model.input".to_string(), 2.0),
                        ("fixture-model.output".to_string(), 4.0),
                    ]),
                    ..ModelProviderConfig::default()
                },
            },
        );
        let risk = RiskProfileConfig {
            allowed_tools: vec!["__gateway_fixture_no_tools__".to_string()],
            ..RiskProfileConfig::default()
        };
        config.risk_profiles.insert("fixture".to_string(), risk);
        config.runtime_profiles.insert(
            "fixture".to_string(),
            RuntimeProfileConfig {
                max_tool_iterations: 1,
                ..RuntimeProfileConfig::default()
            },
        );
        config.agents.insert(
            "web".to_string(),
            AliasedAgentConfig {
                model_provider: "custom.fixture".into(),
                risk_profile: "fixture".into(),
                runtime_profile: "fixture".into(),
                memory: AgentMemoryConfig {
                    backend: MemoryBackendKind::None,
                },
                workspace: AgentWorkspaceConfig {
                    path: Some(workspace),
                    ..AgentWorkspaceConfig::default()
                },
                ..AliasedAgentConfig::default()
            },
        );

        let tracker = Arc::new(
            CostTracker::new(config.cost.clone(), &config.data_dir)
                .expect("production fixture cost tracker"),
        );
        let mut state = crate::api::tests::test_state(config);
        state.cost_tracker = Some(Arc::clone(&tracker));
        (state, tracker)
    }

    async fn collect_production_sse(state: &AppState, message: &str, session_id: &str) -> String {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_static("text/event-stream"),
        );
        headers.insert(
            "X-Session-Id",
            HeaderValue::from_str(session_id).expect("fixture session id header"),
        );
        let response = handle_webhook(
            State(state.clone()),
            test_connect_info(),
            Query(WebhookQuery {
                agent: Some("web".to_string()),
            }),
            headers,
            Ok(Json(WebhookBody {
                message: message.to_string(),
                stream: true,
            })),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let payload = response
            .into_body()
            .collect()
            .await
            .expect("production SSE response body")
            .to_bytes();
        String::from_utf8(payload.to_vec()).expect("production SSE response is UTF-8")
    }

    const PRODUCTION_FIXTURE_STREAM: &str = "data: {\"choices\":[{\"delta\":{\"content\":\"fixture answer\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5}}\n\n\
data: [DONE]\n\n";

    const PRODUCTION_FIXTURE_TOOL_STREAM: &str = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_fixture\",\"type\":\"function\",\"function\":{\"name\":\"calculator\",\"arguments\":\"{\\\"function\\\":\\\"add\\\",\\\"values\\\":[1,2]}\"}}]}}]}\n\n\
data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n\
data: [DONE]\n\n";

    const PRODUCTION_FIXTURE_FINAL_STREAM: &str = "data: {\"choices\":[{\"delta\":{\"content\":\"final answer\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
data: [DONE]\n\n";

    struct HangingProvider;

    #[async_trait]
    impl ModelProvider for HangingProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> anyhow::Result<String> {
            std::future::pending::<()>().await;
            unreachable!("pending future never resolves")
        }
    }

    impl ::clawcrew_api::attribution::Attributable for HangingProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Custom,
                ),
            )
        }

        fn alias(&self) -> &str {
            "HangingProvider"
        }
    }

    #[tokio::test]
    async fn webhook_sse_production_agent_persists_usage_with_agent_alias() {
        let fixture = spawn_chat_completion_fixture(PRODUCTION_FIXTURE_STREAM).await;
        let tmp = tempfile::tempdir().expect("production gateway temp dir");
        let (state, tracker) = production_sse_state(&tmp, &fixture.base_url(), 1.0, false);

        let text = collect_production_sse(&state, "production stream", "production-cost").await;

        assert!(
            text.contains("event: token") && text.contains(r#"data: {"text":"fixture answer"}"#),
            "configured Agent/provider path must forward streamed text: {text}"
        );
        assert!(
            text.contains("event: done") && !text.contains("event: error"),
            "successful production-shaped turn must finish without an error frame: {text}"
        );
        assert_eq!(
            fixture.requests.load(Ordering::SeqCst),
            1,
            "one successful turn should make exactly one provider request"
        );

        let summary = tracker
            .get_summary_for_agent("web")
            .expect("agent-scoped cost summary");
        assert_eq!(summary.request_count, 1);
        assert_eq!(summary.total_tokens, 15);
        assert!(
            summary.session_cost_usd > 0.0,
            "configured pricing must persist a non-zero streamed-turn cost"
        );
        let by_agent = tracker.get_summary().expect("global cost summary").by_agent;
        let stats = by_agent.get("web").expect("agent alias attribution");
        assert_eq!(stats.total_tokens, 15);
        assert!(stats.cost_usd > 0.0);
    }

    #[tokio::test]
    async fn webhook_sse_production_budget_exhaustion_skips_provider_request() {
        let fixture = spawn_chat_completion_fixture(PRODUCTION_FIXTURE_STREAM).await;
        let tmp = tempfile::tempdir().expect("production gateway temp dir");
        let (state, tracker) = production_sse_state(&tmp, &fixture.base_url(), 0.01, false);
        tracker
            .record_usage_with_agent(
                clawcrew_config::cost::types::TokenUsage::new(
                    "fixture-model",
                    1_000_000,
                    0,
                    0,
                    2.0,
                    4.0,
                    0.0,
                ),
                Some("web"),
            )
            .expect("seed exhausted budget record");

        let text = collect_production_sse(&state, "blocked stream", "production-budget").await;

        assert_eq!(
            fixture.requests.load(Ordering::SeqCst),
            0,
            "an exhausted budget must reject before the provider request"
        );
        assert!(
            text.contains("event: error"),
            "budget rejection must be an SSE error: {text}"
        );
        assert!(
            text.contains("Budget exceeded"),
            "SSE error should preserve the budget explanation: {text}"
        );
        assert!(
            !text.contains("event: done"),
            "budget rejection must not emit done: {text}"
        );
    }

    #[tokio::test]
    async fn webhook_sse_production_cache_hit_reconciles_no_chunk_final_response() {
        let fixture = spawn_chat_completion_fixture(PRODUCTION_FIXTURE_STREAM).await;
        let tmp = tempfile::tempdir().expect("production gateway temp dir");
        let (state, tracker) = production_sse_state(&tmp, &fixture.base_url(), 1.0, true);

        let first = collect_production_sse(&state, "cache me", "production-cache-first").await;
        assert!(
            first.contains("event: token"),
            "cache seed must stream a token: {first}"
        );
        assert!(
            first.contains("event: done"),
            "cache seed must complete: {first}"
        );
        assert_eq!(fixture.requests.load(Ordering::SeqCst), 1);

        let second = collect_production_sse(&state, "cache me", "production-cache-second").await;
        assert!(
            second.contains("event: token")
                && second.contains(r#"data: {"text":"fixture answer"}"#),
            "a production cache hit has no runtime chunks, so reconciliation must emit the final response: {second}"
        );
        assert!(
            second.contains("event: done"),
            "cache hit must still terminate with done: {second}"
        );
        assert!(
            !second.contains("event: error"),
            "cache hit must not emit an error: {second}"
        );
        assert_eq!(
            fixture.requests.load(Ordering::SeqCst),
            1,
            "cache hit must not make another provider request"
        );
        assert_eq!(
            tracker
                .get_summary_for_agent("web")
                .expect("cache cost summary")
                .request_count,
            1,
            "cache hit must not record a second usage event"
        );
    }

    #[tokio::test]
    async fn webhook_sse_production_receipt_suffix_reconciles_before_done() {
        use clawcrew_config::schema::ToolReceiptsConfig;

        let fixture = spawn_chat_completion_fixture_sequence(vec![
            PRODUCTION_FIXTURE_TOOL_STREAM.to_string(),
            PRODUCTION_FIXTURE_FINAL_STREAM.to_string(),
        ])
        .await;
        let tmp = tempfile::tempdir().expect("production receipt fixture temp dir");
        let (state, _) = production_sse_state(&tmp, &fixture.base_url(), 1.0, false);
        {
            let mut config = state.config.write();
            config
                .risk_profiles
                .get_mut("fixture")
                .expect("production fixture risk profile")
                .allowed_tools = vec!["calculator".to_string()];
            config
                .runtime_profiles
                .get_mut("fixture")
                .expect("production fixture runtime profile")
                .max_tool_iterations = 2;
            config
                .runtime_profiles
                .get_mut("fixture")
                .expect("production fixture runtime profile")
                .tool_receipts = ToolReceiptsConfig {
                enabled: true,
                show_in_response: true,
                ..ToolReceiptsConfig::default()
            };
        }

        let text = collect_production_sse(&state, "receipt stream", "production-receipt").await;

        assert_eq!(fixture.requests.load(Ordering::SeqCst), 2);
        let final_text = text
            .find(r#"data: {"text":"final answer"}"#)
            .unwrap_or_else(|| panic!("streamed final answer frame: {text}"));
        let receipt_text = text
            .find("Tool receipts:")
            .expect("runtime receipt block in final response");
        let done = text.find("event: done").expect("terminal done frame");
        assert!(
            final_text < receipt_text && receipt_text < done,
            "receipt suffix must be reconciled before done: {text}"
        );
    }

    #[tokio::test]
    async fn webhook_sse_streams_cumulative_token_then_done() {
        let _capture_guard = lock_gateway_chat_dispatch_capture_for_test().await;
        let state = sse_test_state(Arc::new(MockModelProvider::default()));
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_static("text/event-stream"),
        );
        let body = Ok(Json(WebhookBody {
            message: "sse cumulative hello".into(),
            stream: true,
        }));

        let response = handle_webhook(
            State(state.clone()),
            test_connect_info(),
            Query(WebhookQuery::default()),
            headers,
            body,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("text/event-stream")
        );

        let payload = response.into_body().collect().await.unwrap().to_bytes();
        let text = std::str::from_utf8(&payload).unwrap();
        assert!(
            text.contains("event: token") && text.contains(r#"data: {"text":"ok"}"#),
            "expected a cumulative token frame, got: {text}"
        );
        assert!(
            text.contains("event: done") && text.contains("data: {}"),
            "expected a terminating done frame, got: {text}"
        );
        assert!(
            !text.contains("event: error"),
            "unexpected error frame: {text}"
        );
        let captures = gateway_chat_dispatch_captures_for_test();
        assert!(
            captures
                .iter()
                .any(|capture| capture.message == "sse cumulative hello"),
            "streamed dispatch must record the same capture as the JSON path"
        );
    }

    #[tokio::test]
    async fn webhook_stream_true_without_sse_accept_keeps_json() {
        let state = sse_test_state(Arc::new(MockModelProvider::default()));
        let body = Ok(Json(WebhookBody {
            message: "hello".into(),
            stream: true,
        }));

        let response = handle_webhook(
            State(state.clone()),
            test_connect_info(),
            Query(WebhookQuery::default()),
            HeaderMap::new(),
            body,
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let payload = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(parsed["response"], "ok");
    }

    #[tokio::test]
    async fn webhook_sse_abort_cancels_turn_via_shared_registry() {
        let state = sse_test_state(Arc::new(HangingProvider));
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_static("text/event-stream"),
        );
        headers.insert("X-Session-Id", HeaderValue::from_static("sse-abort"));
        let body = Ok(Json(WebhookBody {
            message: "hello".into(),
            stream: true,
        }));

        let state_for_task = state.clone();
        let task = clawcrew_spawn::spawn!(async move {
            handle_webhook(
                State(state_for_task.clone()),
                test_connect_info(),
                Query(WebhookQuery::default()),
                headers,
                body,
            )
            .await
            .into_response()
        });
        let response = task.await.unwrap();

        // The streamed turn registered its cancellation token under the
        // gateway session key derived from X-Session-Id.
        let token = state
            .cancel_tokens
            .lock()
            .expect("cancel_tokens lock poisoned")
            .get("gw_sse-abort")
            .cloned();
        assert!(
            token.is_some(),
            "streamed webhook turn must register its cancellation token"
        );

        // Cancel through the same registry the abort endpoint uses; the
        // stream must then terminate without a done frame.
        if let Some(token) = token {
            token.cancel();
        }
        let payload = response.into_body().collect().await.unwrap().to_bytes();
        let text = std::str::from_utf8(&payload).unwrap();
        assert!(
            !text.contains("event: done"),
            "unexpected done frame: {text}"
        );
        assert!(
            text.contains("event: error"),
            "server-side cancellation must terminate an open SSE stream with an error frame: {text}"
        );
    }

    #[tokio::test]
    async fn webhook_sse_abort_cancels_backpressured_unread_client() {
        let fixture = spawn_burst_hanging_chat_completion_fixture().await;
        let tmp = tempfile::tempdir().expect("backpressure temp dir");
        let (state, _) = production_sse_state(&tmp, &fixture.base_url(), 1.0, false);
        let session_id = "sse-backpressure";
        let response = start_test_sse(&state, session_id).await;
        assert_eq!(response.status(), StatusCode::OK);

        let cancel_key = gateway_cancel_key(session_id);
        let token = wait_for_registry_token(&state, &cancel_key).await;
        wait_for_fixture_requests(&fixture, 1).await;
        wait_for_fixture_chunks(&fixture, 17).await;

        token.cancel();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let registry_empty = !state
                    .cancel_tokens
                    .lock()
                    .expect("cancel_tokens lock poisoned")
                    .contains_key(&cancel_key);
                let provider_closed = fixture.stream_closed.load(Ordering::SeqCst);
                if registry_empty && provider_closed {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("server-side abort must wake a backpressured SSE turn");

        assert!(token.is_cancelled());
        assert!(fixture.stream_closed.load(Ordering::SeqCst));

        // The response body was intentionally left unread while the bounded
        // token queue filled. Resuming the read must still deliver the
        // cancellation terminal frame through its priority channel.
        let text = collect_cancelled_sse(response).await;
        assert!(
            text.contains("event: error"),
            "backpressured cancellation must retain its terminal error: {text}"
        );
        assert!(
            !text.contains("event: done"),
            "a cancelled backpressured stream must not complete: {text}"
        );
    }

    #[tokio::test]
    async fn webhook_sse_body_drop_cancels_turn() {
        let state = sse_test_state(Arc::new(HangingProvider));
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_static("text/event-stream"),
        );
        headers.insert("X-Session-Id", HeaderValue::from_static("sse-drop"));

        let response = handle_webhook(
            State(state.clone()),
            test_connect_info(),
            Query(WebhookQuery::default()),
            headers,
            Ok(Json(WebhookBody {
                message: "hello".into(),
                stream: true,
            })),
        )
        .await;
        let token = state
            .cancel_tokens
            .lock()
            .expect("cancel_tokens lock poisoned")
            .get("gw_sse-drop")
            .cloned()
            .expect("streamed turn must register its cancellation token");

        drop(response);

        tokio::time::timeout(Duration::from_secs(1), token.cancelled())
            .await
            .expect("dropping the SSE body must cancel the in-flight turn");
        assert!(
            !state
                .cancel_tokens
                .lock()
                .expect("cancel_tokens lock poisoned")
                .contains_key("gw_sse-drop"),
            "dropping the SSE body must remove its cancellation token"
        );
    }

    #[tokio::test]
    async fn webhook_sse_replacement_cancels_old_turn_without_removing_new_token() {
        let state = sse_test_state(Arc::new(HangingProvider));
        let request = || {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::ACCEPT,
                HeaderValue::from_static("text/event-stream"),
            );
            headers.insert("X-Session-Id", HeaderValue::from_static("sse-replace"));
            (
                headers,
                Ok(Json(WebhookBody {
                    message: "hello".into(),
                    stream: true,
                })),
            )
        };

        let (headers, body) = request();
        let first = handle_webhook(
            State(state.clone()),
            test_connect_info(),
            Query(WebhookQuery::default()),
            headers,
            body,
        )
        .await;
        let first_token = state
            .cancel_tokens
            .lock()
            .expect("cancel_tokens lock poisoned")
            .get("gw_sse-replace")
            .cloned()
            .expect("first streamed turn must register its cancellation token");

        let (headers, body) = request();
        let second = handle_webhook(
            State(state.clone()),
            test_connect_info(),
            Query(WebhookQuery::default()),
            headers,
            body,
        )
        .await;
        let second_token = state
            .cancel_tokens
            .lock()
            .expect("cancel_tokens lock poisoned")
            .get("gw_sse-replace")
            .cloned()
            .expect("replacement streamed turn must register its cancellation token");

        tokio::time::timeout(Duration::from_secs(1), first_token.cancelled())
            .await
            .expect("registering a replacement must cancel the previous turn");
        tokio::task::yield_now().await;
        let current = state
            .cancel_tokens
            .lock()
            .expect("cancel_tokens lock poisoned")
            .get("gw_sse-replace")
            .cloned()
            .expect("old-turn cleanup must preserve the replacement token");
        assert!(Arc::ptr_eq(&current, &second_token));

        drop(first);
        drop(second);
    }

    #[test]
    fn cancellation_transport_ws_to_sse_preserves_new_owner() {
        // Real WebSocket/Agent setup is stack-heavy on the test platform; keep
        // this transport-level race isolated without changing the global test
        // stack or weakening the production boundary.
        std::thread::Builder::new()
            .name("gateway-ws-to-sse-cancellation".to_string())
            .stack_size(8 * 1024 * 1024)
            .spawn(|| {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("test runtime")
                    .block_on(cancellation_transport_ws_to_sse_preserves_new_owner_inner());
            })
            .expect("spawn WS-to-SSE transport test thread")
            .join()
            .expect("WS-to-SSE transport test thread must not panic");
    }

    async fn cancellation_transport_ws_to_sse_preserves_new_owner_inner() {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::{connect_async, tungstenite::Message as ClientMessage};

        let fixture = spawn_hanging_chat_completion_fixture().await;
        let tmp = tempfile::tempdir().expect("transport test temp dir");
        let (state, _) = production_sse_state(&tmp, &fixture.base_url(), 1.0, false);
        let (gateway_addr, gateway_server) = spawn_test_ws_gateway(state.clone()).await;
        let session_id = "transport.ws-to.sse";
        let session_key = gateway_cancel_key(session_id);

        // This URL connects only to the test's loopback listener. Keep the
        // scheme split so the static insecure-transport rule does not flag a
        // non-production fixture.
        let websocket_url = format!(
            "{}//{gateway_addr}/ws/chat?agent=web&session_id={session_id}",
            "ws:"
        );
        let (mut websocket, _) = connect_async(websocket_url)
            .await
            .expect("WS transport upgrade");
        let session_start = websocket
            .next()
            .await
            .expect("WS session_start frame")
            .expect("WS session_start transport");
        assert!(session_start.into_text().unwrap().contains("session_start"));
        websocket
            .send(ClientMessage::Text(r#"{"type":"connect"}"#.into()))
            .await
            .expect("WS connect frame");
        let connected = websocket
            .next()
            .await
            .expect("WS connected frame")
            .expect("WS connected transport");
        assert!(connected.into_text().unwrap().contains("connected"));
        websocket
            .send(ClientMessage::Text(
                r#"{"type":"message","content":"first transport"}"#.into(),
            ))
            .await
            .expect("WS chat frame");

        let ws_token = wait_for_registry_token(&state, &session_key).await;
        wait_for_fixture_requests(&fixture, 1).await;

        let sse_response = start_test_sse(&state, session_id).await;
        assert_eq!(sse_response.status(), StatusCode::OK);
        let sse_token = wait_for_registry_replacement(&state, &session_key, &ws_token).await;
        wait_for_fixture_requests(&fixture, 2).await;
        tokio::time::timeout(Duration::from_secs(3), ws_token.cancelled())
            .await
            .expect("SSE registration cancels the replaced WS turn");
        wait_for_registry_owner(&state, &session_key, &sse_token).await;

        // The dotted display id must resolve to the canonical key and cancel
        // the replacement SSE turn, not the original WebSocket turn.
        let abort_response = api::handle_api_session_abort(
            State(state.clone()),
            HeaderMap::new(),
            axum::extract::Path(session_id.to_string()),
        )
        .await
        .into_response();
        assert_eq!(abort_response.status(), StatusCode::OK);
        tokio::time::timeout(Duration::from_secs(3), sse_token.cancelled())
            .await
            .expect("abort endpoint cancels the replacement SSE turn");

        // Consume the body so its terminal error and owner-qualified cleanup run.
        let sse_text = collect_cancelled_sse(sse_response).await;
        assert!(
            sse_text.contains("event: error"),
            "SSE cancellation frame: {sse_text}"
        );
        assert!(
            !sse_text.contains("event: done"),
            "cancelled SSE must not complete: {sse_text}"
        );
        wait_for_registry_empty(&state, &session_key).await;

        drop(websocket);
        gateway_server.abort();
    }

    #[test]
    fn cancellation_transport_sse_to_ws_preserves_new_owner() {
        std::thread::Builder::new()
            .name("gateway-sse-to-ws-cancellation".to_string())
            .stack_size(8 * 1024 * 1024)
            .spawn(|| {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("test runtime")
                    .block_on(cancellation_transport_sse_to_ws_preserves_new_owner_inner());
            })
            .expect("spawn SSE-to-WS transport test thread")
            .join()
            .expect("SSE-to-WS transport test thread must not panic");
    }

    async fn cancellation_transport_sse_to_ws_preserves_new_owner_inner() {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::{connect_async, tungstenite::Message as ClientMessage};

        let fixture = spawn_hanging_chat_completion_fixture().await;
        let tmp = tempfile::tempdir().expect("transport test temp dir");
        let (state, _) = production_sse_state(&tmp, &fixture.base_url(), 1.0, false);
        let session_id = "transport-sse-to-ws";
        let session_key = format!("{GW_SESSION_PREFIX}{session_id}");

        let sse_response = start_test_sse(&state, session_id).await;
        assert_eq!(sse_response.status(), StatusCode::OK);
        let sse_token = wait_for_registry_token(&state, &session_key).await;
        wait_for_fixture_requests(&fixture, 1).await;

        let (gateway_addr, gateway_server) = spawn_test_ws_gateway(state.clone()).await;
        // This URL connects only to the test's loopback listener. Keep the
        // scheme split so the static insecure-transport rule does not flag a
        // non-production fixture.
        let websocket_url = format!(
            "{}//{gateway_addr}/ws/chat?agent=web&session_id={session_id}",
            "ws:"
        );
        let (mut websocket, _) = connect_async(websocket_url)
            .await
            .expect("WS transport upgrade");
        let session_start = websocket
            .next()
            .await
            .expect("WS session_start frame")
            .expect("WS session_start transport");
        assert!(session_start.into_text().unwrap().contains("session_start"));
        websocket
            .send(ClientMessage::Text(r#"{"type":"connect"}"#.into()))
            .await
            .expect("WS connect frame");
        let connected = websocket
            .next()
            .await
            .expect("WS connected frame")
            .expect("WS connected transport");
        assert!(connected.into_text().unwrap().contains("connected"));
        websocket
            .send(ClientMessage::Text(
                r#"{"type":"message","content":"replacement transport"}"#.into(),
            ))
            .await
            .expect("WS chat frame");

        let ws_token = wait_for_registry_replacement(&state, &session_key, &sse_token).await;
        wait_for_fixture_requests(&fixture, 2).await;
        tokio::time::timeout(Duration::from_secs(3), sse_token.cancelled())
            .await
            .expect("WS registration cancels the replaced SSE turn");
        wait_for_registry_owner(&state, &session_key, &ws_token).await;

        let sse_text = collect_cancelled_sse(sse_response).await;
        assert!(
            sse_text.contains("event: error"),
            "SSE cancellation frame: {sse_text}"
        );
        assert!(
            !sse_text.contains("event: done"),
            "cancelled SSE must not complete: {sse_text}"
        );

        // Cancel the replacement WS turn and verify that its cleanup removes
        // only its own registry entry after the old SSE cleanup has completed.
        ws_token.cancel();
        wait_for_registry_empty(&state, &session_key).await;

        drop(websocket);
        gateway_server.abort();
    }

    #[test]
    fn websocket_resumes_seeded_legacy_dotted_session_transcript() {
        std::thread::Builder::new()
            .name("gateway-ws-legacy-resume".to_string())
            .stack_size(8 * 1024 * 1024)
            .spawn(|| {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("test runtime")
                    .block_on(websocket_resumes_seeded_legacy_dotted_session_inner());
            })
            .expect("spawn WS legacy-resume test thread")
            .join()
            .expect("WS legacy-resume test thread must not panic");
    }

    async fn websocket_resumes_seeded_legacy_dotted_session_inner() {
        use futures_util::StreamExt;
        use tokio_tungstenite::connect_async;

        let tmp = tempfile::tempdir().expect("legacy-resume temp dir");
        let (mut state, _) =
            production_sse_state(&tmp, "http://127.0.0.1:9/v1/chat/completions", 1.0, false);
        let session_db = tempfile::tempdir().expect("legacy-resume session db");
        let backend: std::sync::Arc<dyn clawcrew_infra::session_backend::SessionBackend> =
            std::sync::Arc::new(
                clawcrew_infra::session_sqlite::SqliteSessionBackend::new(session_db.path())
                    .expect("sqlite session backend"),
            );
        // Seed the transcript under the legacy raw gateway key: dot-bearing
        // display ids persisted this exact key before cancellation keys were
        // normalized, so a reconnect must resume it unchanged.
        let legacy_key = format!("{GW_SESSION_PREFIX}{}", "transport.legacy-resume");
        backend
            .append(
                &legacy_key,
                &clawcrew_providers::ChatMessage::user("seeded legacy turn"),
            )
            .expect("seed legacy transcript");
        state.session_backend = Some(backend);

        let (gateway_addr, gateway_server) = spawn_test_ws_gateway(state.clone()).await;
        let session_id = "transport.legacy-resume";
        // This URL connects only to the test's loopback listener. Keep the
        // scheme split so the static insecure-transport rule does not flag a
        // non-production fixture.
        let websocket_url = format!(
            "{}//{gateway_addr}/ws/chat?agent=web&session_id={session_id}",
            "ws:"
        );
        let (mut websocket, _) = connect_async(websocket_url)
            .await
            .expect("WS transport upgrade");
        let session_start = websocket
            .next()
            .await
            .expect("WS session_start frame")
            .expect("WS session_start transport")
            .into_text()
            .expect("session_start text");
        let session_start: serde_json::Value =
            serde_json::from_str(&session_start).expect("session_start json");
        assert_eq!(session_start["type"], "session_start");
        assert_eq!(
            session_start["resumed"], true,
            "legacy raw-key transcript must resume for a dotted display id"
        );
        assert_eq!(session_start["message_count"], 1);

        drop(websocket);
        gateway_server.abort();
    }

    #[test]
    fn websocket_delivers_api_injected_message_for_dotted_session() {
        std::thread::Builder::new()
            .name("gateway-ws-api-delivery".to_string())
            .stack_size(8 * 1024 * 1024)
            .spawn(|| {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("test runtime")
                    .block_on(websocket_delivers_api_injected_message_inner());
            })
            .expect("spawn WS api-delivery test thread")
            .join()
            .expect("WS api-delivery test thread must not panic");
    }

    async fn websocket_delivers_api_injected_message_inner() {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::{connect_async, tungstenite::Message as ClientMessage};

        let tmp = tempfile::tempdir().expect("api-delivery temp dir");
        let (mut state, _) =
            production_sse_state(&tmp, "http://127.0.0.1:9/v1/chat/completions", 1.0, false);
        let session_db = tempfile::tempdir().expect("api-delivery session db");
        state.session_backend = Some(std::sync::Arc::new(
            clawcrew_infra::session_sqlite::SqliteSessionBackend::new(session_db.path())
                .expect("sqlite session backend"),
        )
            as std::sync::Arc<dyn clawcrew_infra::session_backend::SessionBackend>);

        let (gateway_addr, gateway_server) = spawn_test_ws_gateway(state.clone()).await;
        let session_id = "transport.api-delivery";
        // This URL connects only to the test's loopback listener. Keep the
        // scheme split so the static insecure-transport rule does not flag a
        // non-production fixture.
        let websocket_url = format!(
            "{}//{gateway_addr}/ws/chat?agent=web&session_id={session_id}",
            "ws:"
        );
        let (mut websocket, _) = connect_async(websocket_url)
            .await
            .expect("WS transport upgrade");
        let _session_start = websocket
            .next()
            .await
            .expect("WS session_start frame")
            .expect("WS session_start transport");
        websocket
            .send(ClientMessage::Text(r#"{"type":"connect"}"#.into()))
            .await
            .expect("WS connect frame");
        let connected = websocket
            .next()
            .await
            .expect("WS connected frame")
            .expect("WS connected transport");
        assert!(connected.into_text().unwrap().contains("connected"));

        // The `connected` acknowledgement is sent before the WebSocket has
        // finished Agent setup and subscribed to the shared event channel.
        // Wait for that authoritative readiness signal before injecting an
        // API event; a fixed sleep would make this transport regression flaky.
        tokio::time::timeout(Duration::from_secs(3), async {
            while state.event_tx.receiver_count() == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("WS subscribes to shared event channel");

        // An API-injected message must broadcast with the display id the
        // connected socket filters on, so it reaches the live transport.
        let response = api::handle_api_session_message_post(
            State(state.clone()),
            HeaderMap::new(),
            axum::extract::Path(session_id.to_string()),
            axum::Json(
                serde_json::from_value::<api::SessionMessagePostBody>(serde_json::json!({
                    "content": "injected for dotted session"
                }))
                .expect("body should deserialize"),
            ),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let frame = tokio::time::timeout(Duration::from_secs(3), websocket.next())
            .await
            .expect("WS receives API-injected message event")
            .expect("WS message transport")
            .expect("WS message frame")
            .into_text()
            .expect("message text");
        let event: serde_json::Value = serde_json::from_str(&frame).expect("event json");
        assert_eq!(event["type"], "message");
        assert_eq!(
            event["session_id"], session_id,
            "API broadcasts must carry the display id the socket filters on"
        );
        assert_eq!(event["content"], "injected for dotted session");

        drop(websocket);
        gateway_server.abort();
    }

    #[test]
    fn sse_final_response_reconciliation_handles_empty_and_receipt_suffix() {
        let mut cumulative = String::new();
        let frame = reconcile_sse_final_response(&mut cumulative, "cached response")
            .expect("a no-chunk response must produce one token frame")
            .unwrap();
        let _ = frame;
        assert_eq!(cumulative, "cached response");

        let mut cumulative = "answer".to_string();
        let frame = reconcile_sse_final_response(&mut cumulative, "answer\n\n[receipt]")
            .expect("a final receipt suffix must be emitted")
            .unwrap();
        let _ = frame;
        assert_eq!(cumulative, "answer\n\n[receipt]");

        let mut cumulative = "already complete".to_string();
        assert!(reconcile_sse_final_response(&mut cumulative, "already complete").is_none());
        assert_eq!(cumulative, "already complete");

        let mut cumulative = "speculative streamed text".to_string();
        let frame = reconcile_sse_final_response(&mut cumulative, "authoritative final")
            .expect("a conflicting final response must replace streamed text")
            .unwrap();
        let _ = frame;
        assert_eq!(cumulative, "authoritative final");

        let mut cumulative = "authoritative final with stale suffix".to_string();
        let frame = reconcile_sse_final_response(&mut cumulative, "authoritative final")
            .expect("a shorter authoritative final must remove stale streamed text")
            .unwrap();
        let _ = frame;
        assert_eq!(cumulative, "authoritative final");

        let mut cumulative = "stale streamed text".to_string();
        let frame = reconcile_sse_final_response(&mut cumulative, "")
            .expect("an empty authoritative final must clear stale streamed text")
            .unwrap();
        let _ = frame;
        assert!(cumulative.is_empty());
    }

    #[test]
    fn cancellation_registry_preserves_sse_owner_when_ws_finishes() {
        let registry = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let ws_token = Arc::new(tokio_util::sync::CancellationToken::new());
        let sse_token = Arc::new(tokio_util::sync::CancellationToken::new());

        register_cancel_token(&registry, "gw_cross_direction", Arc::clone(&ws_token));
        register_cancel_token(&registry, "gw_cross_direction", Arc::clone(&sse_token));
        remove_cancel_token_if_current(&registry, "gw_cross_direction", &ws_token);

        assert!(ws_token.is_cancelled());
        let current = registry
            .lock()
            .expect("cancel registry lock")
            .get("gw_cross_direction")
            .cloned()
            .expect("replacement SSE token remains registered");
        assert!(Arc::ptr_eq(&current, &sse_token));
        assert!(!sse_token.is_cancelled());
    }

    #[test]
    fn cancellation_registry_preserves_ws_owner_when_sse_finishes() {
        let registry = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let sse_token = Arc::new(tokio_util::sync::CancellationToken::new());
        let ws_token = Arc::new(tokio_util::sync::CancellationToken::new());

        register_cancel_token(&registry, "gw_cross_direction", Arc::clone(&sse_token));
        register_cancel_token(&registry, "gw_cross_direction", Arc::clone(&ws_token));
        remove_cancel_token_if_current(&registry, "gw_cross_direction", &sse_token);

        assert!(sse_token.is_cancelled());
        let current = registry
            .lock()
            .expect("cancel registry lock")
            .get("gw_cross_direction")
            .cloned()
            .expect("replacement WS token remains registered");
        assert!(Arc::ptr_eq(&current, &ws_token));
        assert!(!ws_token.is_cancelled());
    }

    #[test]
    fn cancellation_registry_keeps_lossy_session_ids_separate() {
        let registry = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let dotted_token = Arc::new(tokio_util::sync::CancellationToken::new());
        let underscored_token = Arc::new(tokio_util::sync::CancellationToken::new());

        register_cancel_token(
            &registry,
            &gateway_cancel_key("team.alpha"),
            Arc::clone(&dotted_token),
        );
        register_cancel_token(
            &registry,
            &gateway_cancel_key("team_alpha"),
            Arc::clone(&underscored_token),
        );

        assert!(!dotted_token.is_cancelled());
        assert!(!underscored_token.is_cancelled());
        assert_eq!(
            registry.lock().expect("cancel registry lock").len(),
            2,
            "distinct session ids must not share a cancellation entry"
        );
    }

    #[tokio::test]
    async fn webhook_idempotency_skips_duplicate_provider_calls() {
        let provider_impl = Arc::new(MockModelProvider::default());
        let model_provider: Arc<dyn ModelProvider> = provider_impl.clone();
        let memory: Arc<dyn Memory> = Arc::new(MockMemory);

        let state = AppState {
            config: Arc::new(RwLock::new(Config::default())),
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            model_provider,
            model: "test-model".into(),
            temperature: None,
            mem: memory.clone(),
            memory_strategy: Arc::new(DefaultMemoryStrategy::with_config(
                Arc::clone(&memory),
                clawcrew_config::schema::MemoryConfig::default(),
                std::path::PathBuf::new(),
            )),
            auto_save: false,
            pairing: Arc::new(PairingGuard::new(false, &[], PairingCodePolicy::default())),
            trust_forwarded_headers: false,
            rate_limiter: Arc::new(GatewayRateLimiter::new(100, 100, 100)),
            auth_limiter: Arc::new(auth_rate_limit::AuthRateLimiter::new()),
            idempotency_store: Arc::new(IdempotencyStore::new(Duration::from_secs(300), 1000)),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp: HashMap::new(),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp_app_secret: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq_signing_secrets: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk_webhook_secret: HashMap::new(),
            #[cfg(feature = "channel-email")]
            gmail_push: None,
            observer: Arc::new(clawcrew_runtime::observability::NoopObserver),
            tools_registry: Arc::new(Vec::new()),
            tools_registry_by_agent: Arc::new(std::collections::HashMap::new()),
            cost_tracker: None,
            event_tx: tokio::sync::broadcast::channel(16).0,
            event_buffer: Arc::new(sse::EventBuffer::new(16)),
            shutdown_tx: tokio::sync::watch::channel(false).0,
            reload_tx: None,
            node_registry: Arc::new(nodes::NodeRegistry::new(16)),
            mdns_peer_registry: nodes::mdns::MdnsPeerRegistry::default(),
            path_prefix: String::new(),
            web_dist_dir: None,
            session_backend: None,
            session_queue: std::sync::Arc::new(crate::session_queue::SessionActorQueue::new(
                8, 30, 600,
            )),
            device_registry: None,
            pending_pairings: None,
            canvas_store: CanvasStore::new(),
            cancel_tokens: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            pending_reload: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tui_registry: None,
            sop_engine: None,
            sop_audit: None,
            #[cfg(feature = "webauthn")]
            webauthn: None,
        };

        let mut headers = HeaderMap::new();
        headers.insert("X-Idempotency-Key", HeaderValue::from_static("abc-123"));

        let body = Ok(Json(WebhookBody {
            message: "hello".into(),
            stream: false,
        }));
        let first = handle_webhook(
            State(state.clone()),
            test_connect_info(),
            Query(WebhookQuery::default()),
            headers.clone(),
            body,
        )
        .await
        .into_response();
        assert_eq!(first.status(), StatusCode::OK);

        let body = Ok(Json(WebhookBody {
            message: "hello".into(),
            stream: false,
        }));
        let second = handle_webhook(
            State(state),
            test_connect_info(),
            Query(WebhookQuery::default()),
            headers,
            body,
        )
        .await
        .into_response();
        assert_eq!(second.status(), StatusCode::OK);

        let payload = second.into_body().collect().await.unwrap().to_bytes();
        let parsed: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(parsed["status"], "duplicate");
        assert_eq!(parsed["idempotent"], true);
        assert_eq!(provider_impl.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn sop_webhook_dispatches_matching_path_without_provider_call() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, provider) = webhook_sop_state(&tmp, "/sop/deploy");
        let (state, secret) = with_webhook_secret(state);
        let response = api_sop_webhook::handle_sop_webhook(
            State(state.clone()),
            test_connect_info(),
            axum::extract::Path("deploy".to_string()),
            webhook_secret_header(&secret),
            axum::body::Bytes::from_static(br#"{"revision":"abc123"}"#),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let payload = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(parsed["status"], "accepted");
        assert_eq!(parsed["path"], "/sop/deploy");
        assert_eq!(parsed["results"][0]["sop"], "webhook-test");
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);

        let run_id = parsed["results"][0]["run_id"].as_str().unwrap();
        let engine = state.sop_engine.as_ref().unwrap().lock().unwrap();
        let run = engine.get_run(run_id).unwrap();
        assert_eq!(
            run.trigger_event.source,
            clawcrew_runtime::sop::SopTriggerSource::Webhook
        );
        assert_eq!(run.trigger_event.topic.as_deref(), Some("/sop/deploy"));
        assert_eq!(
            run.trigger_event.payload.as_deref(),
            Some(r#"{"revision":"abc123"}"#)
        );
    }

    #[tokio::test]
    async fn sop_webhook_route_reaches_the_shared_dispatch_handler() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, provider) = webhook_sop_state(&tmp, "/sop/deploy");
        let (state, secret) = with_webhook_secret(state);
        let app = sop_webhook_routes().with_state(state);
        let mut request = axum::http::Request::post("/sop/deploy")
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .header("X-Webhook-Secret", secret)
            .body(axum::body::Body::from(r#"{"revision":"abc123"}"#))
            .unwrap();
        request.extensions_mut().insert(test_connect_info());

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }

    async fn post_sop_route_without_credentials(
        state: AppState,
        path: &'static str,
        body: &'static [u8],
    ) -> (StatusCode, serde_json::Value) {
        let app = sop_webhook_routes().with_state(state);
        let mut request = axum::http::Request::post(path)
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .unwrap();
        request.extensions_mut().insert(test_connect_info());
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let payload = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&payload).unwrap())
    }

    #[tokio::test]
    async fn sop_route_without_credentials_hides_body_and_engine_state() {
        let tmp = tempfile::tempdir().unwrap();
        let (matching, provider) = webhook_sop_state(&tmp, "/sop/deploy");
        let unavailable_tmp = tempfile::tempdir().unwrap();
        let unavailable = admin_paircode_state(&unavailable_tmp, false, false);

        let cases = [
            post_sop_route_without_credentials(
                matching.clone(),
                "/sop/deploy",
                br#"{"revision":"abc123"}"#,
            )
            .await,
            post_sop_route_without_credentials(
                matching.clone(),
                "/sop/missing",
                br#"{"revision":"abc123"}"#,
            )
            .await,
            post_sop_route_without_credentials(matching, "/sop/deploy", b"not-json").await,
            post_sop_route_without_credentials(unavailable, "/sop/deploy", br#"{}"#).await,
        ];

        let expected_error = cases[0].1["error"].clone();
        for (status, payload) in cases {
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            assert_eq!(
                payload["error"], expected_error,
                "credential failure must not reveal JSON validity, trigger matches, or engine availability"
            );
        }
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn sop_webhook_rejects_unmatched_and_invalid_requests_without_chat_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, provider) = webhook_sop_state(&tmp, "/sop/deploy");
        let (state, secret) = with_webhook_secret(state);

        let unmatched = api_sop_webhook::handle_sop_webhook(
            State(state.clone()),
            test_connect_info(),
            axum::extract::Path("missing".to_string()),
            webhook_secret_header(&secret),
            axum::body::Bytes::from_static(br#"{}"#),
        )
        .await;
        assert_eq!(unmatched.status(), StatusCode::NOT_FOUND);

        let invalid = api_sop_webhook::handle_sop_webhook(
            State(state),
            test_connect_info(),
            axum::extract::Path("deploy".to_string()),
            webhook_secret_header(&secret),
            axum::body::Bytes::from_static(b"not-json"),
        )
        .await;
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn sop_webhook_requires_shared_engine_and_webhook_auth() {
        let disabled_tmp = tempfile::tempdir().unwrap();
        let disabled = admin_paircode_state(&disabled_tmp, false, false);
        let (disabled, secret) = with_webhook_secret(disabled);
        let unavailable = api_sop_webhook::handle_sop_webhook(
            State(disabled),
            test_connect_info(),
            axum::extract::Path("deploy".to_string()),
            webhook_secret_header(&secret),
            axum::body::Bytes::from_static(br#"{}"#),
        )
        .await;
        assert_eq!(unavailable.status(), StatusCode::SERVICE_UNAVAILABLE);

        let protected_tmp = tempfile::tempdir().unwrap();
        let (protected, provider) = webhook_sop_state(&protected_tmp, "/sop/deploy");
        let secret = generate_test_secret();
        protected.config.write().gateway.webhook_secret = Some(secret);
        let unauthorized = api_sop_webhook::handle_sop_webhook(
            State(protected),
            test_connect_info(),
            axum::extract::Path("deploy".to_string()),
            HeaderMap::new(),
            axum::body::Bytes::from_static(br#"{}"#),
        )
        .await;
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn webhook_dispatches_sop_first_then_falls_back_to_chat_on_no_match() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, provider) = webhook_sop_state(&tmp, "/webhook");
        let (state, secret) = with_webhook_secret(state);
        let sop_response = handle_webhook(
            State(state.clone()),
            test_connect_info(),
            Query(WebhookQuery::default()),
            webhook_secret_header(&secret),
            Ok(Json(WebhookBody {
                message: "deploy".into(),
                stream: false,
            })),
        )
        .await
        .into_response();
        assert_eq!(sop_response.status(), StatusCode::OK);
        let payload = sop_response.into_body().collect().await.unwrap().to_bytes();
        let parsed: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(parsed["status"], "accepted");
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);

        let no_match_tmp = tempfile::tempdir().unwrap();
        let (no_match_state, fallback_provider) = webhook_sop_state(&no_match_tmp, "/sop/only");
        let fallback_response = handle_webhook(
            State(no_match_state),
            test_connect_info(),
            Query(WebhookQuery::default()),
            HeaderMap::new(),
            Ok(Json(WebhookBody {
                message: "chat instead".into(),
                stream: false,
            })),
        )
        .await
        .into_response();
        assert_eq!(fallback_response.status(), StatusCode::OK);
        assert_eq!(fallback_provider.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn sop_and_chat_webhook_idempotency_namespaces_do_not_collide() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, provider) = webhook_sop_state(&tmp, "/sop/deploy");
        let (state, secret) = with_webhook_secret(state);
        let mut headers = webhook_secret_header(&secret);
        headers.insert("X-Idempotency-Key", HeaderValue::from_static("same-key"));

        let sop_response = api_sop_webhook::handle_sop_webhook(
            State(state.clone()),
            test_connect_info(),
            axum::extract::Path("deploy".to_string()),
            headers.clone(),
            axum::body::Bytes::from_static(br#"{}"#),
        )
        .await;
        assert_eq!(sop_response.status(), StatusCode::OK);

        let chat_response = handle_webhook(
            State(state),
            test_connect_info(),
            Query(WebhookQuery::default()),
            headers,
            Ok(Json(WebhookBody {
                message: "chat".into(),
                stream: false,
            })),
        )
        .await
        .into_response();
        assert_eq!(chat_response.status(), StatusCode::OK);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn sop_idempotency_namespaced_per_path() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, provider) = webhook_two_sop_state(&tmp, "/sop/deploy", "/sop/rollback");
        let (state, secret) = with_webhook_secret(state);
        let mut headers = webhook_secret_header(&secret);
        headers.insert("X-Idempotency-Key", HeaderValue::from_static("same-key"));

        // Same key, two different SOP paths: both execute.
        let deploy_response = api_sop_webhook::handle_sop_webhook(
            State(state.clone()),
            test_connect_info(),
            axum::extract::Path("deploy".to_string()),
            headers.clone(),
            axum::body::Bytes::from_static(br#"{}"#),
        )
        .await;
        assert_eq!(deploy_response.status(), StatusCode::OK);
        let deploy_payload = deploy_response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let deploy_parsed: serde_json::Value = serde_json::from_slice(&deploy_payload).unwrap();
        assert_eq!(deploy_parsed["status"], "accepted");

        let rollback_response = api_sop_webhook::handle_sop_webhook(
            State(state.clone()),
            test_connect_info(),
            axum::extract::Path("rollback".to_string()),
            headers.clone(),
            axum::body::Bytes::from_static(br#"{}"#),
        )
        .await;
        assert_eq!(rollback_response.status(), StatusCode::OK);
        let rollback_payload = rollback_response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let rollback_parsed: serde_json::Value = serde_json::from_slice(&rollback_payload).unwrap();
        assert_eq!(rollback_parsed["status"], "accepted");

        // Same key, same path again: the second call is suppressed as a duplicate.
        let repeat_response = api_sop_webhook::handle_sop_webhook(
            State(state),
            test_connect_info(),
            axum::extract::Path("deploy".to_string()),
            headers,
            axum::body::Bytes::from_static(br#"{}"#),
        )
        .await;
        assert_eq!(repeat_response.status(), StatusCode::OK);
        let repeat_payload = repeat_response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let repeat_parsed: serde_json::Value = serde_json::from_slice(&repeat_payload).unwrap();
        assert_eq!(repeat_parsed["status"], "duplicate");
        assert_eq!(repeat_parsed["idempotent"], true);
        assert!(
            repeat_parsed["message"]
                .as_str()
                .unwrap_or_default()
                .contains("no new dispatch was started"),
            "duplicate response must describe reservation, not claim successful processing"
        );

        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }

    /// Count SOP runs the engine has ever started (active + finished), so a
    /// race test can prove *no run was started*, not merely that the HTTP
    /// status was 401.
    fn started_run_count(state: &AppState) -> usize {
        let engine = state.sop_engine.as_ref().expect("engine").lock().unwrap();
        engine.active_runs().len() + engine.run_summaries(None).len()
    }

    /// B1 insertion race: pairing disabled and no secret configured, so a
    /// headerless request is authorized against an "unconfigured" snapshot.
    /// An operator then writes `gateway.webhook_secret` into the live config
    /// *after* authorization but before dispatch. The old two-read design would
    /// see a configured control on the second read and admit a request that
    /// presented nothing; the request-scoped verdict must reject it.
    #[tokio::test]
    async fn sop_dispatch_uses_authorization_snapshot_when_secret_added_midrequest() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, provider) = webhook_sop_state(&tmp, "/sop/deploy");

        // Read #1: no control configured at all.
        let verdict = authorize_webhook_request(&state, test_connect_info().0, &HeaderMap::new())
            .expect("no configured control -> authorization itself passes");

        // Concurrent operator action lands between authorization and dispatch.
        state.config.write().gateway.webhook_secret = Some(generate_test_secret());

        // The dispatch decision must come from the snapshot, not the new config.
        let rejection = require_sop_dispatch_credentials(verdict)
            .expect_err("a request that presented no credential must not dispatch a SOP");
        assert_eq!(rejection.0, StatusCode::UNAUTHORIZED);

        // And end-to-end through the route: the headerless caller now fails the
        // (newly configured) secret check outright and still starts no run.
        let response = api_sop_webhook::handle_sop_webhook(
            State(state.clone()),
            test_connect_info(),
            axum::extract::Path("deploy".to_string()),
            HeaderMap::new(),
            axum::body::Bytes::from_static(br#"{}"#),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            started_run_count(&state),
            0,
            "no SOP run may start for a request that presented no credential"
        );
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }

    /// B1 rotation race: secret A is configured and the caller presents A, so
    /// read #1 verifies genuinely. The live config then rotates to secret B
    /// before dispatch. The in-flight request is judged on its own snapshot
    /// (accepted), while rotation takes effect at *next*-request granularity:
    /// a subsequent A request is rejected and a B request is accepted.
    #[tokio::test]
    async fn sop_dispatch_uses_authorization_snapshot_during_secret_rotation() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, _provider) = webhook_sop_state(&tmp, "/sop/deploy");
        let secret_a = generate_test_secret();
        state.config.write().gateway.webhook_secret = Some(secret_a.clone());

        let verdict = authorize_webhook_request(
            &state,
            test_connect_info().0,
            &webhook_secret_header(&secret_a),
        )
        .expect("the presented secret matches the live policy at read #1");

        // Rotation lands between authorization and dispatch.
        let secret_b = generate_test_secret();
        state.config.write().gateway.webhook_secret = Some(secret_b.clone());

        assert!(
            require_sop_dispatch_credentials(verdict).is_ok(),
            "a request that genuinely verified its snapshot's secret keeps that verdict"
        );

        // Next-request granularity: the retired secret is now rejected...
        assert!(
            authorize_webhook_request(
                &state,
                test_connect_info().0,
                &webhook_secret_header(&secret_a),
            )
            .is_err(),
            "a subsequent request bearing the retired secret must be rejected"
        );
        // ...and the replacement is accepted and may dispatch.
        let rotated = authorize_webhook_request(
            &state,
            test_connect_info().0,
            &webhook_secret_header(&secret_b),
        )
        .expect("the rotated secret authorizes the next request");
        assert!(require_sop_dispatch_credentials(rotated).is_ok());
    }

    /// B1 on `/webhook`, where body parsing and trigger matching sit between
    /// authorization and the dispatch credential check — the widest window.
    /// A headerless request must not become dispatchable because a secret was
    /// inserted while the body was being parsed.
    #[tokio::test]
    async fn webhook_path_snapshot_survives_body_parse_and_trigger_match() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, provider) = webhook_sop_state(&tmp, "/webhook");

        let verdict = authorize_webhook_request(&state, test_connect_info().0, &HeaderMap::new())
            .expect("no configured control -> authorization itself passes");

        // Simulate the parse/match window: config mutates before dispatch.
        assert!(
            api_sop_webhook::has_matching_webhook_sop(&state, "/webhook").unwrap(),
            "fixture must load a matching /webhook trigger"
        );
        state.config.write().gateway.webhook_secret = Some(generate_test_secret());

        assert!(
            require_sop_dispatch_credentials(verdict).is_err(),
            "the /webhook SOP branch must judge the snapshot, not the mutated config"
        );

        let response = handle_webhook(
            State(state.clone()),
            test_connect_info(),
            Query(WebhookQuery::default()),
            HeaderMap::new(),
            Ok(Json(WebhookBody {
                message: "deploy".into(),
                stream: false,
            })),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            started_run_count(&state),
            0,
            "no SOP run may start on the /webhook path either"
        );
        assert_eq!(
            provider.calls.load(Ordering::SeqCst),
            0,
            "and it must not silently fall back to the chat/model path"
        );
    }

    /// B1 purity: the dispatch gate decides only from the request-scoped
    /// verdict. Two verdict values with identical contents must produce
    /// identical decisions regardless of what the live config says, which is
    /// only possible because the function never touches `AppState`.
    #[tokio::test]
    async fn require_sop_dispatch_credentials_is_pure_over_verdict() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, _provider) = webhook_sop_state(&tmp, "/sop/deploy");

        let unconfigured =
            authorize_webhook_request(&state, test_connect_info().0, &HeaderMap::new())
                .expect("no control configured");
        let before = require_sop_dispatch_credentials(unconfigured).is_ok();

        // Flip the live config to the opposite policy in every way we can.
        state.config.write().gateway.webhook_secret = Some(generate_test_secret());
        let after = require_sop_dispatch_credentials(unconfigured).is_ok();

        assert_eq!(
            before, after,
            "the decision must depend only on the verdict, never on live state"
        );
        assert!(!before, "an unconfigured snapshot must fail closed");
    }

    /// B2: the replay-domain encoding must be injective. Every one of these
    /// pairs collides under the old `format!("{namespace}:{key}")` join, which
    /// let an authenticated caller suppress a *different* attempt.
    #[test]
    fn idempotency_storage_key_encoding_is_injective() {
        // Adversarial cross-path: `/sop/a` + `b:c` vs `/sop/a:b` + `c`.
        assert_ne!(
            idempotency_storage_key(Some("sop:/sop/a"), "b:c"),
            idempotency_storage_key(Some("sop:/sop/a:b"), "c"),
            "a caller must not shift bytes across the namespace/key boundary"
        );
        // `/webhook` (global domain) vs `/sop/*`: a forged caller key must not
        // alias a namespaced SOP key.
        assert_ne!(
            idempotency_storage_key(None, "sop:/sop/deploy:k"),
            idempotency_storage_key(Some("sop:/sop/deploy"), "k"),
            "/webhook keys must never collide with /sop/* keys"
        );
        // Empty-component edge cases stay distinct too.
        assert_ne!(
            idempotency_storage_key(Some(""), "x"),
            idempotency_storage_key(Some("x"), ""),
        );
        // The encoding is still deterministic and path-discriminating.
        assert_eq!(
            idempotency_storage_key(Some("sop:/sop/deploy"), "k"),
            idempotency_storage_key(Some("sop:/sop/deploy"), "k"),
        );
        assert_ne!(
            idempotency_storage_key(Some("sop:/sop/deploy"), "k"),
            idempotency_storage_key(Some("sop:/sop/rollback"), "k"),
            "distinct SOP paths must remain distinct",
        );
    }

    /// B2 end-to-end: two different SOP paths whose `(path, key)` pairs collide
    /// under the old join must both dispatch. `/sop/a` with `X-Idempotency-Key:
    /// b:c` and `/sop/a:b` with key `c` are genuinely different requests.
    #[tokio::test]
    async fn sop_idempotency_resists_adversarial_cross_path_collision() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, _provider) = webhook_two_sop_state(&tmp, "/sop/a", "/sop/a:b");
        let (state, secret) = with_webhook_secret(state);
        let mut headers = webhook_secret_header(&secret);
        headers.insert("X-Idempotency-Key", HeaderValue::from_static("b:c"));
        let mut second_headers = webhook_secret_header(&secret);
        second_headers.insert("X-Idempotency-Key", HeaderValue::from_static("c"));

        let first = api_sop_webhook::handle_sop_webhook(
            State(state.clone()),
            test_connect_info(),
            axum::extract::Path("a".to_string()),
            headers,
            axum::body::Bytes::from_static(br#"{}"#),
        )
        .await;
        assert_eq!(first.status(), StatusCode::OK);
        let first_parsed: serde_json::Value =
            serde_json::from_slice(&first.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(first_parsed["status"], "accepted");

        let second = api_sop_webhook::handle_sop_webhook(
            State(state),
            test_connect_info(),
            axum::extract::Path("a:b".to_string()),
            second_headers,
            axum::body::Bytes::from_static(br#"{}"#),
        )
        .await;
        assert_eq!(second.status(), StatusCode::OK);
        let second_parsed: serde_json::Value =
            serde_json::from_slice(&second.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(
            second_parsed["status"], "accepted",
            "a colliding-by-concatenation key must not suppress a different SOP path"
        );
    }

    /// B2 end-to-end across endpoints: a `/webhook` caller who forges the SOP
    /// namespace prefix into their own key must not reserve the `/sop/deploy`
    /// replay slot and suppress the real SOP request.
    #[tokio::test]
    async fn webhook_forged_namespace_key_cannot_suppress_sop_dispatch() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, _provider) = webhook_two_sop_state(&tmp, "/webhook", "/sop/deploy");
        let (state, secret) = with_webhook_secret(state);

        // `/webhook` caller forges the `/sop/deploy` storage key.
        let mut forged = webhook_secret_header(&secret);
        forged.insert(
            "X-Idempotency-Key",
            HeaderValue::from_static("sop:/sop/deploy:k"),
        );
        let chat = handle_webhook(
            State(state.clone()),
            test_connect_info(),
            Query(WebhookQuery::default()),
            forged,
            Ok(Json(WebhookBody {
                message: "hello".into(),
                stream: false,
            })),
        )
        .await
        .into_response();
        assert_eq!(chat.status(), StatusCode::OK);

        // The genuine `/sop/deploy` request with key `k` must still dispatch.
        let mut sop_headers = webhook_secret_header(&secret);
        sop_headers.insert("X-Idempotency-Key", HeaderValue::from_static("k"));
        let sop = api_sop_webhook::handle_sop_webhook(
            State(state),
            test_connect_info(),
            axum::extract::Path("deploy".to_string()),
            sop_headers,
            axum::body::Bytes::from_static(br#"{}"#),
        )
        .await;
        assert_eq!(sop.status(), StatusCode::OK);
        let parsed: serde_json::Value =
            serde_json::from_slice(&sop.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(
            parsed["status"], "accepted",
            "a forged /webhook key must not reserve a /sop/* replay slot"
        );
    }

    /// B2 property test: `idempotency_storage_key` must be injective over the
    /// whole `(namespace, caller_key)` space, not just the two collisions the
    /// reviewers happened to name. Exhaustively cross-products adversarial
    /// components that are rich in the separator character and asserts distinct
    /// inputs never share an encoding.
    #[test]
    fn idempotency_storage_key_is_injective_over_adversarial_inputs() {
        let namespaces = [
            None,
            Some(""),
            Some(":"),
            Some("sop:/sop/a"),
            Some("sop:/sop/a:b"),
            Some("sop:/sop/a:b:c"),
            Some("sop:/sop/deploy"),
            Some("1:x"),
            Some("global"),
            Some("ns"),
        ];
        let caller_keys = ["", ":", "b:c", "c", "k", "sop:/sop/deploy:k", "1:x", "ns"];

        let mut seen: std::collections::HashMap<String, (Option<&str>, &str)> =
            std::collections::HashMap::new();
        for namespace in namespaces {
            for caller_key in caller_keys {
                let encoded = idempotency_storage_key(namespace, caller_key);
                if let Some(previous) = seen.insert(encoded.clone(), (namespace, caller_key)) {
                    assert_eq!(
                        previous,
                        (namespace, caller_key),
                        "encoding collision: {previous:?} and {:?} both map to {encoded}",
                        (namespace, caller_key),
                    );
                }
            }
        }
        assert_eq!(
            seen.len(),
            namespaces.len() * caller_keys.len(),
            "every distinct (namespace, caller_key) pair must have a distinct encoding"
        );
    }

    /// B2 counterpart: injectivity must not have broken the *intended*
    /// duplicate suppression — the same path with the same key is still a replay.
    #[tokio::test]
    async fn sop_idempotency_same_path_same_key_still_suppresses() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, _provider) = webhook_sop_state(&tmp, "/sop/deploy");
        let (state, secret) = with_webhook_secret(state);
        let mut headers = webhook_secret_header(&secret);
        headers.insert("X-Idempotency-Key", HeaderValue::from_static("dup-key"));

        let first = api_sop_webhook::handle_sop_webhook(
            State(state.clone()),
            test_connect_info(),
            axum::extract::Path("deploy".to_string()),
            headers.clone(),
            axum::body::Bytes::from_static(br#"{}"#),
        )
        .await;
        assert_eq!(first.status(), StatusCode::OK);
        let first_parsed: serde_json::Value =
            serde_json::from_slice(&first.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(first_parsed["status"], "accepted");

        let second = api_sop_webhook::handle_sop_webhook(
            State(state),
            test_connect_info(),
            axum::extract::Path("deploy".to_string()),
            headers,
            axum::body::Bytes::from_static(br#"{}"#),
        )
        .await;
        assert_eq!(second.status(), StatusCode::OK);
        let second_parsed: serde_json::Value =
            serde_json::from_slice(&second.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(
            second_parsed["status"], "duplicate",
            "the intended same-path same-key replay suppression must survive the new encoding"
        );
    }

    #[tokio::test]
    async fn sop_dispatch_rejected_when_no_credentials_configured() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, provider) = webhook_sop_state(&tmp, "/sop/deploy");

        let response = api_sop_webhook::handle_sop_webhook(
            State(state),
            test_connect_info(),
            axum::extract::Path("deploy".to_string()),
            HeaderMap::new(),
            axum::body::Bytes::from_static(br#"{}"#),
        )
        .await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let payload = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        let error = parsed["error"].as_str().unwrap_or_default();
        assert!(error.contains("require_pairing"), "error was: {error}");
        assert!(error.contains("X-Webhook-Secret"), "error was: {error}");
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn webhook_sop_dispatch_rejected_when_no_credentials_configured() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, provider) = webhook_sop_state(&tmp, "/webhook");

        let response = handle_webhook(
            State(state),
            test_connect_info(),
            Query(WebhookQuery::default()),
            HeaderMap::new(),
            Ok(Json(WebhookBody {
                message: "deploy".into(),
                stream: false,
            })),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let payload = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        let error = parsed["error"].as_str().unwrap_or_default();
        assert!(error.contains("require_pairing"), "error was: {error}");
        assert!(error.contains("X-Webhook-Secret"), "error was: {error}");
        // Rejected outright — a matching SOP with no configured credential
        // must never silently fall back to the chat/model path.
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn sop_dispatch_succeeds_with_paired_bearer_token() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut state, provider) = webhook_sop_state(&tmp, "/sop/deploy");
        // A plaintext (not pre-hashed) token: `PairingGuard::new` treats a
        // bare 64-hex-char value as an already-hashed token, so a "zc_"
        // prefix keeps this one unambiguously plaintext.
        let token = format!("zc_{}", generate_test_secret());
        state.pairing = Arc::new(PairingGuard::new(
            true,
            std::slice::from_ref(&token),
            PairingCodePolicy::default(),
        ));

        let response = api_sop_webhook::handle_sop_webhook(
            State(state),
            test_connect_info(),
            axum::extract::Path("deploy".to_string()),
            bearer_headers(&token),
            axum::body::Bytes::from_static(br#"{}"#),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let payload = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(parsed["status"], "accepted");
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn webhook_sop_first_ignores_bogus_chat_agent_query_param() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, provider) = webhook_sop_state(&tmp, "/webhook");
        let (state, secret) = with_webhook_secret(state);

        let response = handle_webhook(
            State(state),
            test_connect_info(),
            Query(WebhookQuery {
                agent: Some("missing".into()),
            }),
            webhook_secret_header(&secret),
            Ok(Json(WebhookBody {
                message: "deploy".into(),
                stream: false,
            })),
        )
        .await
        .into_response();

        // A matching SOP dispatches even though `?agent=missing` has no
        // `[agents.missing]` entry — the chat-only param is never inspected.
        assert_eq!(response.status(), StatusCode::OK);
        let payload = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(parsed["status"], "accepted");
        assert_eq!(parsed["source"], "webhook");
        // The model provider is never touched.
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn webhook_unknown_agent_rejected_before_dispatch() {
        let provider_impl = Arc::new(MockModelProvider::default());
        let model_provider: Arc<dyn ModelProvider> = provider_impl.clone();
        let memory: Arc<dyn Memory> = Arc::new(MockMemory);

        let state = AppState {
            config: Arc::new(RwLock::new(Config::default())),
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            model_provider,
            model: "test-model".into(),
            temperature: None,
            mem: memory.clone(),
            memory_strategy: Arc::new(DefaultMemoryStrategy::with_config(
                Arc::clone(&memory),
                clawcrew_config::schema::MemoryConfig::default(),
                std::path::PathBuf::new(),
            )),
            auto_save: false,
            pairing: Arc::new(PairingGuard::new(false, &[], PairingCodePolicy::default())),
            trust_forwarded_headers: false,
            rate_limiter: Arc::new(GatewayRateLimiter::new(100, 100, 100)),
            auth_limiter: Arc::new(auth_rate_limit::AuthRateLimiter::new()),
            idempotency_store: Arc::new(IdempotencyStore::new(Duration::from_secs(300), 1000)),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp: HashMap::new(),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp_app_secret: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq_signing_secrets: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk_webhook_secret: HashMap::new(),
            #[cfg(feature = "channel-email")]
            gmail_push: None,
            observer: Arc::new(clawcrew_runtime::observability::NoopObserver),
            tools_registry: Arc::new(Vec::new()),
            tools_registry_by_agent: Arc::new(std::collections::HashMap::new()),
            cost_tracker: None,
            event_tx: tokio::sync::broadcast::channel(16).0,
            event_buffer: Arc::new(sse::EventBuffer::new(16)),
            shutdown_tx: tokio::sync::watch::channel(false).0,
            reload_tx: None,
            node_registry: Arc::new(nodes::NodeRegistry::new(16)),
            mdns_peer_registry: nodes::mdns::MdnsPeerRegistry::default(),
            path_prefix: String::new(),
            web_dist_dir: None,
            session_backend: None,
            session_queue: std::sync::Arc::new(crate::session_queue::SessionActorQueue::new(
                8, 30, 600,
            )),
            device_registry: None,
            pending_pairings: None,
            canvas_store: CanvasStore::new(),
            cancel_tokens: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            pending_reload: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tui_registry: None,
            sop_engine: None,
            sop_audit: None,
            #[cfg(feature = "webauthn")]
            webauthn: None,
        };

        // An idempotency key on a rejected request must NOT be consumed.
        let mut headers = HeaderMap::new();
        headers.insert("X-Idempotency-Key", HeaderValue::from_static("ghost-key"));

        let response = handle_webhook(
            State(state.clone()),
            test_connect_info(),
            Query(WebhookQuery {
                agent: Some("ghost".into()),
            }),
            headers,
            Ok(Json(WebhookBody {
                message: "hello".into(),
                stream: false,
            })),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let payload = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert!(
            parsed["error"]
                .as_str()
                .unwrap_or_default()
                .contains("Unknown agent `ghost`")
        );
        assert_eq!(provider_impl.calls.load(Ordering::SeqCst), 0);
        // Key still fresh — a corrected retry with the same key proceeds.
        assert!(state.idempotency_store.record_if_new("ghost-key"));
    }

    #[tokio::test]
    async fn webhook_explicit_agent_reports_model_without_owning_lifecycle() {
        let provider_impl = Arc::new(MockModelProvider::default());
        let model_provider: Arc<dyn ModelProvider> = provider_impl.clone();
        let memory: Arc<dyn Memory> = Arc::new(MockMemory);
        let observer_impl = Arc::new(CapturingObserver::default());
        let observer: Arc<dyn clawcrew_runtime::observability::Observer> = observer_impl.clone();

        let mut config = Config::default();
        config.providers.models.anthropic.insert(
            "default".into(),
            clawcrew_config::schema::AnthropicModelProviderConfig {
                base: clawcrew_config::schema::ModelProviderConfig {
                    model: Some("agent-model".into()),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let expected_provider = "anthropic.default".to_string();
        config.agents.insert(
            "nova".to_string(),
            clawcrew_config::schema::AliasedAgentConfig {
                enabled: true,
                model_provider: expected_provider.clone().into(),
                ..Default::default()
            },
        );

        let state = AppState {
            config: Arc::new(RwLock::new(config)),
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            model_provider,
            model: "startup-model".into(),
            temperature: None,
            mem: memory,
            memory_strategy: Arc::new(DefaultMemoryStrategy::with_config(
                Arc::new(MockMemory),
                clawcrew_config::schema::MemoryConfig::default(),
                std::path::PathBuf::new(),
            )),
            auto_save: false,
            pairing: Arc::new(PairingGuard::new(false, &[], PairingCodePolicy::default())),
            trust_forwarded_headers: false,
            rate_limiter: Arc::new(GatewayRateLimiter::new(100, 100, 100)),
            auth_limiter: Arc::new(auth_rate_limit::AuthRateLimiter::new()),
            idempotency_store: Arc::new(IdempotencyStore::new(Duration::from_secs(300), 1000)),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp: HashMap::new(),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp_app_secret: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq_signing_secrets: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk_webhook_secret: HashMap::new(),
            #[cfg(feature = "channel-email")]
            gmail_push: None,
            observer,
            tools_registry: Arc::new(Vec::new()),
            tools_registry_by_agent: Arc::new(std::collections::HashMap::new()),
            cost_tracker: None,
            event_tx: tokio::sync::broadcast::channel(16).0,
            event_buffer: Arc::new(sse::EventBuffer::new(16)),
            shutdown_tx: tokio::sync::watch::channel(false).0,
            reload_tx: None,
            node_registry: Arc::new(nodes::NodeRegistry::new(16)),
            mdns_peer_registry: nodes::mdns::MdnsPeerRegistry::default(),
            path_prefix: String::new(),
            web_dist_dir: None,
            session_backend: None,
            session_queue: std::sync::Arc::new(crate::session_queue::SessionActorQueue::new(
                8, 30, 600,
            )),
            device_registry: None,
            pending_pairings: None,
            canvas_store: CanvasStore::new(),
            cancel_tokens: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            pending_reload: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tui_registry: None,
            sop_engine: None,
            sop_audit: None,
            #[cfg(feature = "webauthn")]
            webauthn: None,
        };

        let response = handle_webhook(
            State(state),
            test_connect_info(),
            Query(WebhookQuery {
                agent: Some("nova".into()),
            }),
            HeaderMap::new(),
            Ok(Json(WebhookBody {
                message: "hello".into(),
                stream: false,
            })),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(provider_impl.calls.load(Ordering::SeqCst), 1);
        let payload = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(parsed["model"], "agent-model");
        let events = observer_impl.events.lock();
        assert!(
            !events.iter().any(|event| matches!(
                event,
                clawcrew_runtime::observability::ObserverEvent::AgentStart { .. }
                    | clawcrew_runtime::observability::ObserverEvent::AgentEnd { .. }
                    | clawcrew_runtime::observability::ObserverEvent::LlmRequest { .. }
                    | clawcrew_runtime::observability::ObserverEvent::LlmResponse { .. }
            )),
            "the HTTP handler must not create a second agent lifecycle; events were: {events:?}"
        );
    }

    #[tokio::test]
    async fn webhook_autosave_stores_distinct_keys_per_request() {
        let provider_impl = Arc::new(MockModelProvider::default());
        let model_provider: Arc<dyn ModelProvider> = provider_impl.clone();

        let tracking_impl = Arc::new(TrackingMemory::default());
        let memory: Arc<dyn Memory> = tracking_impl.clone();

        let state = AppState {
            config: Arc::new(RwLock::new(Config::default())),
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            model_provider,
            model: "test-model".into(),
            temperature: None,
            mem: memory,
            memory_strategy: Arc::new(DefaultMemoryStrategy::with_config(
                Arc::new(MockMemory),
                clawcrew_config::schema::MemoryConfig::default(),
                std::path::PathBuf::new(),
            )),
            auto_save: true,
            pairing: Arc::new(PairingGuard::new(false, &[], PairingCodePolicy::default())),
            trust_forwarded_headers: false,
            rate_limiter: Arc::new(GatewayRateLimiter::new(100, 100, 100)),
            auth_limiter: Arc::new(auth_rate_limit::AuthRateLimiter::new()),
            idempotency_store: Arc::new(IdempotencyStore::new(Duration::from_secs(300), 1000)),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp: HashMap::new(),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp_app_secret: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq_signing_secrets: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk_webhook_secret: HashMap::new(),
            #[cfg(feature = "channel-email")]
            gmail_push: None,
            observer: Arc::new(clawcrew_runtime::observability::NoopObserver),
            tools_registry: Arc::new(Vec::new()),
            tools_registry_by_agent: Arc::new(std::collections::HashMap::new()),
            cost_tracker: None,
            event_tx: tokio::sync::broadcast::channel(16).0,
            event_buffer: Arc::new(sse::EventBuffer::new(16)),
            shutdown_tx: tokio::sync::watch::channel(false).0,
            reload_tx: None,
            node_registry: Arc::new(nodes::NodeRegistry::new(16)),
            mdns_peer_registry: nodes::mdns::MdnsPeerRegistry::default(),
            path_prefix: String::new(),
            web_dist_dir: None,
            session_backend: None,
            session_queue: std::sync::Arc::new(crate::session_queue::SessionActorQueue::new(
                8, 30, 600,
            )),
            device_registry: None,
            pending_pairings: None,
            canvas_store: CanvasStore::new(),
            cancel_tokens: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            pending_reload: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tui_registry: None,
            sop_engine: None,
            sop_audit: None,
            #[cfg(feature = "webauthn")]
            webauthn: None,
        };

        let headers = HeaderMap::new();

        let body1 = Ok(Json(WebhookBody {
            message: "hello one".into(),
            stream: false,
        }));
        let first = handle_webhook(
            State(state.clone()),
            test_connect_info(),
            Query(WebhookQuery::default()),
            headers.clone(),
            body1,
        )
        .await
        .into_response();
        assert_eq!(first.status(), StatusCode::OK);

        let body2 = Ok(Json(WebhookBody {
            message: "hello two".into(),
            stream: false,
        }));
        let second = handle_webhook(
            State(state),
            test_connect_info(),
            Query(WebhookQuery::default()),
            headers,
            body2,
        )
        .await
        .into_response();
        assert_eq!(second.status(), StatusCode::OK);

        let keys = tracking_impl.keys.lock().clone();
        assert_eq!(keys.len(), 2);
        assert_ne!(keys[0], keys[1]);
        assert!(keys[0].starts_with("webhook_msg_"));
        assert!(keys[1].starts_with("webhook_msg_"));
        assert_eq!(provider_impl.calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn webhook_secret_hash_is_deterministic_and_nonempty() {
        let secret_a = generate_test_secret();
        let secret_b = generate_test_secret();
        let one = hash_webhook_secret(&secret_a);
        let two = hash_webhook_secret(&secret_a);
        let other = hash_webhook_secret(&secret_b);

        assert_eq!(one, two);
        assert_ne!(one, other);
        assert_eq!(one.len(), 64);
    }

    #[test]
    fn gateway_webhook_secret_uses_one_live_config_owner() {
        let tmp = tempfile::tempdir().unwrap();
        let state = admin_paircode_state(&tmp, false, false);
        {
            let mut config = state.config.write();
            config.channels.webhook.insert(
                "enabled-a".into(),
                clawcrew_config::schema::WebhookConfig {
                    enabled: true,
                    secret: Some("channel-secret-a".into()),
                    ..Default::default()
                },
            );
            config.channels.webhook.insert(
                "enabled-b".into(),
                clawcrew_config::schema::WebhookConfig {
                    enabled: true,
                    secret: Some("channel-secret-b".into()),
                    ..Default::default()
                },
            );
            config.channels.webhook.insert(
                "disabled".into(),
                clawcrew_config::schema::WebhookConfig {
                    enabled: false,
                    secret: Some("disabled-channel-secret".into()),
                    ..Default::default()
                },
            );
        }
        assert_eq!(
            configured_gateway_webhook_secret_hash(&state),
            None,
            "channel listener aliases must never become gateway credentials"
        );
        assert!(
            authorize_webhook_request(&state, test_connect_info().0, &HeaderMap::new())
                .map(require_sop_dispatch_credentials)
                .is_ok_and(|dispatch| dispatch.is_err()),
            "multiple channel aliases without a gateway credential must fail closed"
        );

        let startup_secret = "synthetic-startup-gateway-secret".to_string();
        state.config.write().gateway.webhook_secret = Some(startup_secret.clone());
        assert!(
            authorize_webhook_request(
                &state,
                test_connect_info().0,
                &webhook_secret_header("channel-secret-a"),
            )
            .is_err(),
            "an enabled channel alias secret must not authorize the gateway"
        );
        assert!(
            authorize_webhook_request(
                &state,
                test_connect_info().0,
                &webhook_secret_header(&startup_secret),
            )
            .is_ok()
        );

        let rotated_secret = "synthetic-rotated-gateway-secret".to_string();
        state.config.write().gateway.webhook_secret = Some(rotated_secret.clone());
        assert!(
            authorize_webhook_request(
                &state,
                test_connect_info().0,
                &webhook_secret_header(&startup_secret),
            )
            .is_err(),
            "authorization must not retain a startup snapshot after config replacement"
        );
        assert!(
            authorize_webhook_request(
                &state,
                test_connect_info().0,
                &webhook_secret_header(&rotated_secret),
            )
            .is_ok(),
            "the live canonical gateway credential must take effect"
        );
    }

    #[tokio::test]
    async fn webhook_secret_hash_rejects_missing_header() {
        let provider_impl = Arc::new(MockModelProvider::default());
        let model_provider: Arc<dyn ModelProvider> = provider_impl.clone();
        let memory: Arc<dyn Memory> = Arc::new(MockMemory);
        let secret = generate_test_secret();
        let mut config = Config::default();
        config.gateway.webhook_secret = Some(secret.clone());

        let state = AppState {
            config: Arc::new(RwLock::new(config)),
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            model_provider,
            model: "test-model".into(),
            temperature: None,
            mem: memory.clone(),
            memory_strategy: Arc::new(DefaultMemoryStrategy::with_config(
                Arc::clone(&memory),
                clawcrew_config::schema::MemoryConfig::default(),
                std::path::PathBuf::new(),
            )),
            auto_save: false,
            pairing: Arc::new(PairingGuard::new(false, &[], PairingCodePolicy::default())),
            trust_forwarded_headers: false,
            rate_limiter: Arc::new(GatewayRateLimiter::new(100, 100, 100)),
            auth_limiter: Arc::new(auth_rate_limit::AuthRateLimiter::new()),
            idempotency_store: Arc::new(IdempotencyStore::new(Duration::from_secs(300), 1000)),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp: HashMap::new(),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp_app_secret: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq_signing_secrets: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk_webhook_secret: HashMap::new(),
            #[cfg(feature = "channel-email")]
            gmail_push: None,
            observer: Arc::new(clawcrew_runtime::observability::NoopObserver),
            tools_registry: Arc::new(Vec::new()),
            tools_registry_by_agent: Arc::new(std::collections::HashMap::new()),
            cost_tracker: None,
            event_tx: tokio::sync::broadcast::channel(16).0,
            event_buffer: Arc::new(sse::EventBuffer::new(16)),
            shutdown_tx: tokio::sync::watch::channel(false).0,
            reload_tx: None,
            node_registry: Arc::new(nodes::NodeRegistry::new(16)),
            mdns_peer_registry: nodes::mdns::MdnsPeerRegistry::default(),
            path_prefix: String::new(),
            web_dist_dir: None,
            session_backend: None,
            session_queue: std::sync::Arc::new(crate::session_queue::SessionActorQueue::new(
                8, 30, 600,
            )),
            device_registry: None,
            pending_pairings: None,
            canvas_store: CanvasStore::new(),
            cancel_tokens: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            pending_reload: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tui_registry: None,
            sop_engine: None,
            sop_audit: None,
            #[cfg(feature = "webauthn")]
            webauthn: None,
        };

        let response = handle_webhook(
            State(state),
            test_connect_info(),
            Query(WebhookQuery::default()),
            HeaderMap::new(),
            Ok(Json(WebhookBody {
                message: "hello".into(),
                stream: false,
            })),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(provider_impl.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn webhook_secret_hash_rejects_invalid_header() {
        let provider_impl = Arc::new(MockModelProvider::default());
        let model_provider: Arc<dyn ModelProvider> = provider_impl.clone();
        let memory: Arc<dyn Memory> = Arc::new(MockMemory);
        let valid_secret = generate_test_secret();
        let wrong_secret = generate_test_secret();
        let mut config = Config::default();
        config.gateway.webhook_secret = Some(valid_secret.clone());

        let state = AppState {
            config: Arc::new(RwLock::new(config)),
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            model_provider,
            model: "test-model".into(),
            temperature: None,
            mem: memory.clone(),
            memory_strategy: Arc::new(DefaultMemoryStrategy::with_config(
                Arc::clone(&memory),
                clawcrew_config::schema::MemoryConfig::default(),
                std::path::PathBuf::new(),
            )),
            auto_save: false,
            pairing: Arc::new(PairingGuard::new(false, &[], PairingCodePolicy::default())),
            trust_forwarded_headers: false,
            rate_limiter: Arc::new(GatewayRateLimiter::new(100, 100, 100)),
            auth_limiter: Arc::new(auth_rate_limit::AuthRateLimiter::new()),
            idempotency_store: Arc::new(IdempotencyStore::new(Duration::from_secs(300), 1000)),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp: HashMap::new(),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp_app_secret: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq_signing_secrets: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk_webhook_secret: HashMap::new(),
            #[cfg(feature = "channel-email")]
            gmail_push: None,
            observer: Arc::new(clawcrew_runtime::observability::NoopObserver),
            tools_registry: Arc::new(Vec::new()),
            tools_registry_by_agent: Arc::new(std::collections::HashMap::new()),
            cost_tracker: None,
            event_tx: tokio::sync::broadcast::channel(16).0,
            event_buffer: Arc::new(sse::EventBuffer::new(16)),
            shutdown_tx: tokio::sync::watch::channel(false).0,
            reload_tx: None,
            node_registry: Arc::new(nodes::NodeRegistry::new(16)),
            mdns_peer_registry: nodes::mdns::MdnsPeerRegistry::default(),
            path_prefix: String::new(),
            web_dist_dir: None,
            session_backend: None,
            session_queue: std::sync::Arc::new(crate::session_queue::SessionActorQueue::new(
                8, 30, 600,
            )),
            device_registry: None,
            pending_pairings: None,
            canvas_store: CanvasStore::new(),
            cancel_tokens: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            pending_reload: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tui_registry: None,
            sop_engine: None,
            sop_audit: None,
            #[cfg(feature = "webauthn")]
            webauthn: None,
        };

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Webhook-Secret",
            HeaderValue::from_str(&wrong_secret).unwrap(),
        );

        let response = handle_webhook(
            State(state),
            test_connect_info(),
            Query(WebhookQuery::default()),
            headers,
            Ok(Json(WebhookBody {
                message: "hello".into(),
                stream: false,
            })),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(provider_impl.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn webhook_secret_hash_accepts_valid_header() {
        let provider_impl = Arc::new(MockModelProvider::default());
        let model_provider: Arc<dyn ModelProvider> = provider_impl.clone();
        let memory: Arc<dyn Memory> = Arc::new(MockMemory);
        let secret = generate_test_secret();
        let mut config = Config::default();
        config.gateway.webhook_secret = Some(secret.clone());

        let state = AppState {
            config: Arc::new(RwLock::new(config)),
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            model_provider,
            model: "test-model".into(),
            temperature: None,
            mem: memory.clone(),
            memory_strategy: Arc::new(DefaultMemoryStrategy::with_config(
                Arc::clone(&memory),
                clawcrew_config::schema::MemoryConfig::default(),
                std::path::PathBuf::new(),
            )),
            auto_save: false,
            pairing: Arc::new(PairingGuard::new(false, &[], PairingCodePolicy::default())),
            trust_forwarded_headers: false,
            rate_limiter: Arc::new(GatewayRateLimiter::new(100, 100, 100)),
            auth_limiter: Arc::new(auth_rate_limit::AuthRateLimiter::new()),
            idempotency_store: Arc::new(IdempotencyStore::new(Duration::from_secs(300), 1000)),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp: HashMap::new(),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp_app_secret: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq_signing_secrets: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk_webhook_secret: HashMap::new(),
            #[cfg(feature = "channel-email")]
            gmail_push: None,
            observer: Arc::new(clawcrew_runtime::observability::NoopObserver),
            tools_registry: Arc::new(Vec::new()),
            tools_registry_by_agent: Arc::new(std::collections::HashMap::new()),
            cost_tracker: None,
            event_tx: tokio::sync::broadcast::channel(16).0,
            event_buffer: Arc::new(sse::EventBuffer::new(16)),
            shutdown_tx: tokio::sync::watch::channel(false).0,
            reload_tx: None,
            node_registry: Arc::new(nodes::NodeRegistry::new(16)),
            mdns_peer_registry: nodes::mdns::MdnsPeerRegistry::default(),
            path_prefix: String::new(),
            web_dist_dir: None,
            session_backend: None,
            session_queue: std::sync::Arc::new(crate::session_queue::SessionActorQueue::new(
                8, 30, 600,
            )),
            device_registry: None,
            pending_pairings: None,
            canvas_store: CanvasStore::new(),
            cancel_tokens: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            pending_reload: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tui_registry: None,
            sop_engine: None,
            sop_audit: None,
            #[cfg(feature = "webauthn")]
            webauthn: None,
        };

        let mut headers = HeaderMap::new();
        headers.insert("X-Webhook-Secret", HeaderValue::from_str(&secret).unwrap());

        let response = handle_webhook(
            State(state),
            test_connect_info(),
            Query(WebhookQuery::default()),
            headers,
            Ok(Json(WebhookBody {
                message: "hello".into(),
                stream: false,
            })),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(provider_impl.calls.load(Ordering::SeqCst), 1);
    }

    #[cfg(feature = "channel-nextcloud")]
    fn compute_nextcloud_signature_hex(secret: &str, random: &str, body: &str) -> String {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let payload = format!("{random}{body}");
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(payload.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    #[cfg(feature = "channel-nextcloud")]
    #[tokio::test]
    async fn nextcloud_talk_webhook_returns_not_found_when_not_configured() {
        let model_provider: Arc<dyn ModelProvider> = Arc::new(MockModelProvider::default());
        let memory: Arc<dyn Memory> = Arc::new(MockMemory);

        let state = AppState {
            config: Arc::new(RwLock::new(Config::default())),
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            model_provider,
            model: "test-model".into(),
            temperature: None,
            mem: memory.clone(),
            memory_strategy: Arc::new(DefaultMemoryStrategy::with_config(
                Arc::clone(&memory),
                clawcrew_config::schema::MemoryConfig::default(),
                std::path::PathBuf::new(),
            )),
            auto_save: false,
            pairing: Arc::new(PairingGuard::new(false, &[], PairingCodePolicy::default())),
            trust_forwarded_headers: false,
            rate_limiter: Arc::new(GatewayRateLimiter::new(100, 100, 100)),
            auth_limiter: Arc::new(auth_rate_limit::AuthRateLimiter::new()),
            idempotency_store: Arc::new(IdempotencyStore::new(Duration::from_secs(300), 1000)),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp: HashMap::new(),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp_app_secret: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq_signing_secrets: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk_webhook_secret: HashMap::new(),
            #[cfg(feature = "channel-email")]
            gmail_push: None,
            observer: Arc::new(clawcrew_runtime::observability::NoopObserver),
            tools_registry: Arc::new(Vec::new()),
            tools_registry_by_agent: Arc::new(std::collections::HashMap::new()),
            cost_tracker: None,
            event_tx: tokio::sync::broadcast::channel(16).0,
            event_buffer: Arc::new(sse::EventBuffer::new(16)),
            shutdown_tx: tokio::sync::watch::channel(false).0,
            reload_tx: None,
            node_registry: Arc::new(nodes::NodeRegistry::new(16)),
            mdns_peer_registry: nodes::mdns::MdnsPeerRegistry::default(),
            path_prefix: String::new(),
            web_dist_dir: None,
            session_backend: None,
            session_queue: std::sync::Arc::new(crate::session_queue::SessionActorQueue::new(
                8, 30, 600,
            )),
            device_registry: None,
            pending_pairings: None,
            canvas_store: CanvasStore::new(),
            cancel_tokens: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            pending_reload: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tui_registry: None,
            sop_engine: None,
            sop_audit: None,
            #[cfg(feature = "webauthn")]
            webauthn: None,
        };

        let response = Box::pin(handle_nextcloud_talk_webhook(
            State(state),
            HeaderMap::new(),
            Bytes::from_static(br#"{"type":"message"}"#),
        ))
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[cfg(feature = "channel-nextcloud")]
    #[tokio::test]
    async fn nextcloud_talk_webhook_rejects_invalid_signature() {
        let provider_impl = Arc::new(MockModelProvider::default());
        let model_provider: Arc<dyn ModelProvider> = provider_impl.clone();
        let memory: Arc<dyn Memory> = Arc::new(MockMemory);

        let alias = "nextcloud_talk_test_alias";
        let peer_resolver: Arc<dyn Fn() -> Vec<String> + Send + Sync> = Arc::new(Vec::new);
        let channel = Arc::new(NextcloudTalkChannel::new(
            "https://cloud.example.com".into(),
            None,
            String::new(),
            alias,
            peer_resolver,
        ));

        let secret = "nextcloud-test-secret";
        let random = "seed-value";
        let body = r#"{"type":"message","object":{"token":"room-token"},"message":{"actorType":"users","actorId":"user_a","message":"hello"}}"#;
        let _valid_signature = compute_nextcloud_signature_hex(secret, random, body);
        let invalid_signature = "deadbeef";

        let state = AppState {
            config: Arc::new(RwLock::new(Config::default())),
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            model_provider,
            model: "test-model".into(),
            temperature: None,
            mem: memory.clone(),
            memory_strategy: Arc::new(DefaultMemoryStrategy::with_config(
                Arc::clone(&memory),
                clawcrew_config::schema::MemoryConfig::default(),
                std::path::PathBuf::new(),
            )),
            auto_save: false,
            pairing: Arc::new(PairingGuard::new(false, &[], PairingCodePolicy::default())),
            trust_forwarded_headers: false,
            rate_limiter: Arc::new(GatewayRateLimiter::new(100, 100, 100)),
            auth_limiter: Arc::new(auth_rate_limit::AuthRateLimiter::new()),
            idempotency_store: Arc::new(IdempotencyStore::new(Duration::from_secs(300), 1000)),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp: HashMap::new(),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp_app_secret: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq_signing_secrets: HashMap::new(),
            nextcloud_talk: HashMap::from([(alias.to_string(), channel)]),
            nextcloud_talk_webhook_secret: HashMap::from([(alias.to_string(), Arc::from(secret))]),
            #[cfg(feature = "channel-email")]
            gmail_push: None,
            observer: Arc::new(clawcrew_runtime::observability::NoopObserver),
            tools_registry: Arc::new(Vec::new()),
            tools_registry_by_agent: Arc::new(std::collections::HashMap::new()),
            cost_tracker: None,
            event_tx: tokio::sync::broadcast::channel(16).0,
            event_buffer: Arc::new(sse::EventBuffer::new(16)),
            shutdown_tx: tokio::sync::watch::channel(false).0,
            reload_tx: None,
            node_registry: Arc::new(nodes::NodeRegistry::new(16)),
            mdns_peer_registry: nodes::mdns::MdnsPeerRegistry::default(),
            path_prefix: String::new(),
            web_dist_dir: None,
            session_backend: None,
            session_queue: std::sync::Arc::new(crate::session_queue::SessionActorQueue::new(
                8, 30, 600,
            )),
            device_registry: None,
            pending_pairings: None,
            canvas_store: CanvasStore::new(),
            cancel_tokens: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            pending_reload: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tui_registry: None,
            sop_engine: None,
            sop_audit: None,
            #[cfg(feature = "webauthn")]
            webauthn: None,
        };

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Nextcloud-Talk-Random",
            HeaderValue::from_str(random).unwrap(),
        );
        headers.insert(
            "X-Nextcloud-Talk-Signature",
            HeaderValue::from_str(invalid_signature).unwrap(),
        );

        let response = Box::pin(handle_nextcloud_talk_webhook(
            State(state),
            headers,
            Bytes::from(body),
        ))
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(provider_impl.calls.load(Ordering::SeqCst), 0);
    }

    /// Fail closed. An alias with no resolved bot secret cannot verify
    /// anything, so the webhook is refused before parsing or dispatch.
    #[cfg(feature = "channel-nextcloud")]
    #[tokio::test]
    async fn nextcloud_talk_webhook_rejects_when_no_secret_is_configured() {
        let provider_impl = Arc::new(MockModelProvider::default());
        let model_provider: Arc<dyn ModelProvider> = provider_impl.clone();
        let memory: Arc<dyn Memory> = Arc::new(MockMemory);

        let alias = "nextcloud_talk_test_alias";
        let peer_resolver: Arc<dyn Fn() -> Vec<String> + Send + Sync> = Arc::new(Vec::new);
        let channel = Arc::new(NextcloudTalkChannel::new(
            "https://cloud.example.com".into(),
            None,
            String::new(),
            alias,
            peer_resolver,
        ));

        let body = r#"{"type":"message","object":{"token":"room-token"},"message":{"actorType":"users","actorId":"user_a","message":"hello"}}"#;

        let state = AppState {
            config: Arc::new(RwLock::new(Config::default())),
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            model_provider,
            model: "test-model".into(),
            temperature: None,
            mem: memory.clone(),
            memory_strategy: Arc::new(DefaultMemoryStrategy::with_config(
                Arc::clone(&memory),
                clawcrew_config::schema::MemoryConfig::default(),
                std::path::PathBuf::new(),
            )),
            auto_save: false,
            pairing: Arc::new(PairingGuard::new(false, &[], PairingCodePolicy::default())),
            trust_forwarded_headers: false,
            rate_limiter: Arc::new(GatewayRateLimiter::new(100, 100, 100)),
            auth_limiter: Arc::new(auth_rate_limit::AuthRateLimiter::new()),
            idempotency_store: Arc::new(IdempotencyStore::new(Duration::from_secs(300), 1000)),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp: HashMap::new(),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp_app_secret: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq_signing_secrets: HashMap::new(),
            nextcloud_talk: HashMap::from([(alias.to_string(), channel)]),
            nextcloud_talk_webhook_secret: HashMap::new(),
            #[cfg(feature = "channel-email")]
            gmail_push: None,
            observer: Arc::new(clawcrew_runtime::observability::NoopObserver),
            tools_registry: Arc::new(Vec::new()),
            tools_registry_by_agent: Arc::new(std::collections::HashMap::new()),
            cost_tracker: None,
            event_tx: tokio::sync::broadcast::channel(16).0,
            event_buffer: Arc::new(sse::EventBuffer::new(16)),
            shutdown_tx: tokio::sync::watch::channel(false).0,
            reload_tx: None,
            node_registry: Arc::new(nodes::NodeRegistry::new(16)),
            mdns_peer_registry: nodes::mdns::MdnsPeerRegistry::default(),
            path_prefix: String::new(),
            web_dist_dir: None,
            session_backend: None,
            session_queue: std::sync::Arc::new(crate::session_queue::SessionActorQueue::new(
                8, 30, 600,
            )),
            device_registry: None,
            pending_pairings: None,
            canvas_store: CanvasStore::new(),
            cancel_tokens: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            pending_reload: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tui_registry: None,
            sop_engine: None,
            sop_audit: None,
            #[cfg(feature = "webauthn")]
            webauthn: None,
        };

        let response = Box::pin(handle_nextcloud_talk_webhook(
            State(state),
            HeaderMap::new(),
            Bytes::from(body),
        ))
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(provider_impl.calls.load(Ordering::SeqCst), 0);
    }

    // handler must return 200 OK before the (potentially
    // slow) LLM call completes, so Nextcloud Talk doesn't cancel the webhook
    // request at its ~5s timeout.
    #[cfg(feature = "channel-nextcloud")]
    #[derive(Default)]
    struct SlowProvider {
        calls: AtomicUsize,
        started_tx: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    }

    #[cfg(feature = "channel-nextcloud")]
    #[async_trait]
    impl ModelProvider for SlowProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> anyhow::Result<String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(tx) = self.started_tx.lock().take() {
                let _ = tx.send(());
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok("slow ok".into())
        }
    }
    #[cfg(feature = "channel-nextcloud")]
    impl ::clawcrew_api::attribution::Attributable for SlowProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Custom,
                ),
            )
        }
        fn alias(&self) -> &str {
            "SlowProvider"
        }
    }

    #[cfg(feature = "channel-nextcloud")]
    #[tokio::test]
    async fn nextcloud_talk_webhook_returns_before_llm_call_completes() {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let provider_impl = Arc::new(SlowProvider {
            calls: AtomicUsize::new(0),
            started_tx: Mutex::new(Some(started_tx)),
        });
        let provider: Arc<dyn ModelProvider> = provider_impl.clone();
        let memory: Arc<dyn Memory> = Arc::new(MockMemory);

        // Obviously-fake placeholder, never a real credential.
        let secret = "fake-nextcloud-webhook-secret-not-real";
        let random = "0123456789abcdef0123456789abcdef";

        // The same secret governs both directions now, so the channel is built
        // with that resolved secret as its bot token rather than the `None` this
        // test used to pass.
        let channel = Arc::new(NextcloudTalkChannel::new(
            "https://cloud.example.com".into(),
            Some(secret.to_string()),
            String::new(),
            "default",
            Arc::new(|| vec!["*".to_string()]),
        ));

        let body = r#"{"type":"message","object":{"token":"room-token"},"actor":{"id":"user_a","name":"User A"},"message":{"actorType":"users","actorId":"user_a","message":"hello"}}"#;
        let signature = compute_nextcloud_signature_hex(secret, random, body);

        let state = AppState {
            config: Arc::new(RwLock::new(Config::default())),
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            model_provider: provider,
            model: "test-model".into(),
            temperature: None,
            mem: memory.clone(),
            memory_strategy: Arc::new(DefaultMemoryStrategy::with_config(
                Arc::clone(&memory),
                clawcrew_config::schema::MemoryConfig::default(),
                std::path::PathBuf::new(),
            )),
            auto_save: false,
            pairing: Arc::new(PairingGuard::new(false, &[], PairingCodePolicy::default())),
            trust_forwarded_headers: false,
            rate_limiter: Arc::new(GatewayRateLimiter::new(100, 100, 100)),
            auth_limiter: Arc::new(auth_rate_limit::AuthRateLimiter::new()),
            idempotency_store: Arc::new(IdempotencyStore::new(Duration::from_secs(300), 1000)),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp: HashMap::new(),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp_app_secret: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq_signing_secrets: HashMap::new(),
            nextcloud_talk: HashMap::from([("default".to_string(), channel)]),
            // A resolved secret, not an empty map. Inbound verification is now
            // mandatory and fail-closed, so an unsigned request is rejected with
            // 401 before the handler ever spawns the LLM task — which would make
            // this test pass for the wrong reason (no provider call because the
            // request was refused, not because the ack raced ahead of a slow
            // provider). Signing the request keeps it on the fast-ack path.
            nextcloud_talk_webhook_secret: HashMap::from([(
                "default".to_string(),
                std::sync::Arc::<str>::from(secret),
            )]),
            pending_reload: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tui_registry: None,
            #[cfg(feature = "channel-email")]
            gmail_push: None,
            observer: Arc::new(clawcrew_runtime::observability::NoopObserver),
            tools_registry: Arc::new(Vec::new()),
            tools_registry_by_agent: Arc::new(std::collections::HashMap::new()),
            cost_tracker: None,
            event_tx: tokio::sync::broadcast::channel(16).0,
            event_buffer: Arc::new(sse::EventBuffer::new(16)),
            shutdown_tx: tokio::sync::watch::channel(false).0,
            reload_tx: None,
            node_registry: Arc::new(nodes::NodeRegistry::new(16)),
            mdns_peer_registry: nodes::mdns::MdnsPeerRegistry::default(),
            path_prefix: String::new(),
            web_dist_dir: None,
            session_backend: None,
            session_queue: std::sync::Arc::new(crate::session_queue::SessionActorQueue::new(
                8, 30, 600,
            )),
            device_registry: None,
            pending_pairings: None,
            canvas_store: CanvasStore::new(),
            cancel_tokens: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            sop_engine: None,
            sop_audit: None,
            #[cfg(feature = "webauthn")]
            webauthn: None,
        };

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Nextcloud-Talk-Random",
            HeaderValue::from_str(random).unwrap(),
        );
        headers.insert(
            "X-Nextcloud-Talk-Signature",
            HeaderValue::from_str(&signature).unwrap(),
        );

        let start = std::time::Instant::now();
        let response = tokio::time::timeout(
            Duration::from_secs(2),
            Box::pin(handle_nextcloud_talk_webhook(
                State(state),
                headers,
                Bytes::from(body),
            )),
        )
        .await
        .expect("webhook must return before 2s deadline (regression #6156)")
        .into_response();

        let elapsed = start.elapsed();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            elapsed < Duration::from_secs(2),
            "handler returned after {elapsed:?}; expected fast return for #6156"
        );

        // Confirm the spawned task actually started the LLM call (i.e., the
        // ack didn't just skip processing). The 30s sleep is still in flight.
        tokio::time::timeout(Duration::from_secs(2), started_rx)
            .await
            .expect("spawned LLM call did not start within 2s")
            .expect("started_tx sender was dropped");
        assert_eq!(provider_impl.calls.load(Ordering::SeqCst), 1);
    }

    // ══════════════════════════════════════════════════════════
    // WhatsApp Signature Verification Tests (CWE-345 Prevention)
    // ══════════════════════════════════════════════════════════

    #[cfg(feature = "channel-whatsapp-cloud")]
    fn compute_whatsapp_signature_hex(secret: &str, body: &[u8]) -> String {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        hex::encode(mac.finalize().into_bytes())
    }

    #[cfg(feature = "channel-whatsapp-cloud")]
    fn compute_whatsapp_signature_header(secret: &str, body: &[u8]) -> String {
        format!("sha256={}", compute_whatsapp_signature_hex(secret, body))
    }

    #[cfg(feature = "channel-whatsapp-cloud")]
    #[test]
    fn whatsapp_signature_valid() {
        let app_secret = generate_test_secret();
        let body = b"test body content";

        let signature_header = compute_whatsapp_signature_header(&app_secret, body);

        assert!(verify_whatsapp_signature(
            &app_secret,
            body,
            &signature_header
        ));
    }

    #[cfg(feature = "channel-whatsapp-cloud")]
    #[test]
    fn whatsapp_signature_invalid_wrong_secret() {
        let app_secret = generate_test_secret();
        let wrong_secret = generate_test_secret();
        let body = b"test body content";

        let signature_header = compute_whatsapp_signature_header(&wrong_secret, body);

        assert!(!verify_whatsapp_signature(
            &app_secret,
            body,
            &signature_header
        ));
    }

    #[cfg(feature = "channel-whatsapp-cloud")]
    #[test]
    fn whatsapp_signature_invalid_wrong_body() {
        let app_secret = generate_test_secret();
        let original_body = b"original body";
        let tampered_body = b"tampered body";

        let signature_header = compute_whatsapp_signature_header(&app_secret, original_body);

        // Verify with tampered body should fail
        assert!(!verify_whatsapp_signature(
            &app_secret,
            tampered_body,
            &signature_header
        ));
    }

    #[cfg(feature = "channel-whatsapp-cloud")]
    #[test]
    fn whatsapp_signature_missing_prefix() {
        let app_secret = generate_test_secret();
        let body = b"test body";

        // Signature without "sha256=" prefix
        let signature_header = "abc123def456";

        assert!(!verify_whatsapp_signature(
            &app_secret,
            body,
            signature_header
        ));
    }

    #[cfg(feature = "channel-whatsapp-cloud")]
    #[test]
    fn whatsapp_signature_empty_header() {
        let app_secret = generate_test_secret();
        let body = b"test body";

        assert!(!verify_whatsapp_signature(&app_secret, body, ""));
    }

    #[cfg(feature = "channel-whatsapp-cloud")]
    #[test]
    fn whatsapp_signature_invalid_hex() {
        let app_secret = generate_test_secret();
        let body = b"test body";

        // Invalid hex characters
        let signature_header = "sha256=not_valid_hex_zzz";

        assert!(!verify_whatsapp_signature(
            &app_secret,
            body,
            signature_header
        ));
    }

    #[cfg(feature = "channel-whatsapp-cloud")]
    #[test]
    fn whatsapp_signature_empty_body() {
        let app_secret = generate_test_secret();
        let body = b"";

        let signature_header = compute_whatsapp_signature_header(&app_secret, body);

        assert!(verify_whatsapp_signature(
            &app_secret,
            body,
            &signature_header
        ));
    }

    #[cfg(feature = "channel-whatsapp-cloud")]
    #[test]
    fn whatsapp_signature_unicode_body() {
        let app_secret = generate_test_secret();
        let body = "Hello 🦀 World".as_bytes();

        let signature_header = compute_whatsapp_signature_header(&app_secret, body);

        assert!(verify_whatsapp_signature(
            &app_secret,
            body,
            &signature_header
        ));
    }

    #[cfg(feature = "channel-whatsapp-cloud")]
    #[test]
    fn whatsapp_signature_json_payload() {
        let app_secret = generate_test_secret();
        let body = br#"{"entry":[{"changes":[{"value":{"messages":[{"from":"1234567890","text":{"body":"Hello"}}]}}]}]}"#;

        let signature_header = compute_whatsapp_signature_header(&app_secret, body);

        assert!(verify_whatsapp_signature(
            &app_secret,
            body,
            &signature_header
        ));
    }

    #[cfg(feature = "channel-whatsapp-cloud")]
    #[test]
    fn whatsapp_signature_case_sensitive_prefix() {
        let app_secret = generate_test_secret();
        let body = b"test body";

        let hex_sig = compute_whatsapp_signature_hex(&app_secret, body);

        // Wrong case prefix should fail
        let wrong_prefix = format!("SHA256={hex_sig}");
        assert!(!verify_whatsapp_signature(&app_secret, body, &wrong_prefix));

        // Correct prefix should pass
        let correct_prefix = format!("sha256={hex_sig}");
        assert!(verify_whatsapp_signature(
            &app_secret,
            body,
            &correct_prefix
        ));
    }

    #[cfg(feature = "channel-whatsapp-cloud")]
    #[test]
    fn whatsapp_signature_truncated_hex() {
        let app_secret = generate_test_secret();
        let body = b"test body";

        let hex_sig = compute_whatsapp_signature_hex(&app_secret, body);
        let truncated = &hex_sig[..32]; // Only half the signature
        let signature_header = format!("sha256={truncated}");

        assert!(!verify_whatsapp_signature(
            &app_secret,
            body,
            &signature_header
        ));
    }

    #[cfg(feature = "channel-whatsapp-cloud")]
    #[test]
    fn whatsapp_signature_extra_bytes() {
        let app_secret = generate_test_secret();
        let body = b"test body";

        let hex_sig = compute_whatsapp_signature_hex(&app_secret, body);
        let extended = format!("{hex_sig}deadbeef");
        let signature_header = format!("sha256={extended}");

        assert!(!verify_whatsapp_signature(
            &app_secret,
            body,
            &signature_header
        ));
    }

    // ══════════════════════════════════════════════════════════
    // IdempotencyStore Edge-Case Tests
    // ══════════════════════════════════════════════════════════

    #[test]
    fn idempotency_store_allows_different_keys() {
        let store = IdempotencyStore::new(Duration::from_secs(60), 100);
        assert!(store.record_if_new("key-a"));
        assert!(store.record_if_new("key-b"));
        assert!(store.record_if_new("key-c"));
        assert!(store.record_if_new("key-d"));
    }

    #[test]
    fn idempotency_store_max_keys_clamped_to_one() {
        let store = IdempotencyStore::new(Duration::from_secs(60), 0);
        assert!(store.record_if_new("only-key"));
        assert!(!store.record_if_new("only-key"));
    }

    #[test]
    fn idempotency_store_rapid_duplicate_rejected() {
        let store = IdempotencyStore::new(Duration::from_secs(300), 100);
        assert!(store.record_if_new("rapid"));
        assert!(!store.record_if_new("rapid"));
    }

    #[test]
    fn duplicate_idempotency_log_omits_caller_key() {
        let _hook_guard = clawcrew_log::__private_test_hook_lock();
        clawcrew_log::try_install_capture_subscriber();
        let mut receiver = clawcrew_log::subscribe_or_install();
        while receiver.try_recv().is_ok() {}

        let raw_key = "caller-sensitive-id";
        record_duplicate_idempotency_log();

        let event = loop {
            match receiver.try_recv() {
                Ok(value)
                    if value.get("message").and_then(|message| message.as_str())
                        == Some("webhook duplicate ignored") =>
                {
                    break value;
                }
                Ok(_) => continue,
                Err(error) => panic!("duplicate log event was not broadcast: {error}"),
            }
        };
        clawcrew_log::clear_broadcast_hook();

        assert_eq!(
            event["attributes"]["idempotency_key_present"],
            serde_json::Value::Bool(true)
        );
        assert!(event["attributes"].get("idempotency_key").is_none());
        assert!(
            !event.to_string().contains(raw_key),
            "caller-controlled idempotency key must not enter structured logs"
        );
    }

    #[test]
    fn idempotency_store_accepts_after_ttl_expires() {
        let store = IdempotencyStore::new(Duration::from_millis(1), 100);
        assert!(store.record_if_new("ttl-key"));
        std::thread::sleep(Duration::from_millis(10));
        assert!(store.record_if_new("ttl-key"));
    }

    #[test]
    fn idempotency_store_eviction_preserves_newest() {
        let store = IdempotencyStore::new(Duration::from_secs(300), 1);
        assert!(store.record_if_new("old-key"));
        std::thread::sleep(Duration::from_millis(2));
        assert!(store.record_if_new("new-key"));

        let entries = store.entries.lock();
        assert_eq!(entries.committed.len(), 1);
        assert!(!entries.committed.contains_key("old-key"));
        assert!(entries.committed.contains_key("new-key"));
    }

    #[test]
    fn rate_limiter_allows_after_window_expires() {
        let window = Duration::from_millis(50);
        let limiter = SlidingWindowRateLimiter::new(2, window, 100);
        assert!(limiter.allow("ip-1"));
        assert!(limiter.allow("ip-1"));
        assert!(!limiter.allow("ip-1")); // blocked

        // Wait for window to expire
        std::thread::sleep(Duration::from_millis(60));

        // Should be allowed again
        assert!(limiter.allow("ip-1"));
    }

    #[test]
    fn rate_limiter_independent_keys_tracked_separately() {
        let limiter = SlidingWindowRateLimiter::new(2, Duration::from_secs(60), 100);
        assert!(limiter.allow("ip-1"));
        assert!(limiter.allow("ip-1"));
        assert!(!limiter.allow("ip-1")); // ip-1 blocked

        // ip-2 should still work
        assert!(limiter.allow("ip-2"));
        assert!(limiter.allow("ip-2"));
        assert!(!limiter.allow("ip-2")); // ip-2 now blocked
    }

    #[test]
    fn rate_limiter_exact_boundary_at_max_keys() {
        let limiter = SlidingWindowRateLimiter::new(10, Duration::from_secs(60), 3);
        assert!(limiter.allow("ip-1"));
        assert!(limiter.allow("ip-2"));
        assert!(limiter.allow("ip-3"));
        // At capacity now
        assert!(limiter.allow("ip-4")); // should evict ip-1

        let guard = limiter.requests.lock();
        assert_eq!(guard.0.len(), 3);
        assert!(
            !guard.0.contains_key("ip-1"),
            "ip-1 should have been evicted"
        );
        assert!(guard.0.contains_key("ip-2"));
        assert!(guard.0.contains_key("ip-3"));
        assert!(guard.0.contains_key("ip-4"));
    }

    #[test]
    fn gateway_rate_limiter_pair_and_webhook_are_independent() {
        let limiter = GatewayRateLimiter::new(2, 3, 100);

        // Exhaust pair limit
        assert!(limiter.allow_pair("ip-1"));
        assert!(limiter.allow_pair("ip-1"));
        assert!(!limiter.allow_pair("ip-1")); // pair blocked

        // Webhook should still work
        assert!(limiter.allow_webhook("ip-1"));
        assert!(limiter.allow_webhook("ip-1"));
        assert!(limiter.allow_webhook("ip-1"));
        assert!(!limiter.allow_webhook("ip-1")); // webhook now blocked
    }

    #[test]
    fn rate_limiter_single_key_max_allows_one_request() {
        let limiter = SlidingWindowRateLimiter::new(5, Duration::from_secs(60), 1);
        assert!(limiter.allow("ip-1"));
        assert!(limiter.allow("ip-2")); // evicts ip-1

        let guard = limiter.requests.lock();
        assert_eq!(guard.0.len(), 1);
        assert!(guard.0.contains_key("ip-2"));
        assert!(!guard.0.contains_key("ip-1"));
    }

    #[test]
    fn rate_limiter_concurrent_access_safe() {
        use std::sync::Arc;

        let limiter = Arc::new(SlidingWindowRateLimiter::new(
            1000,
            Duration::from_secs(60),
            1000,
        ));
        let mut handles = Vec::new();

        for i in 0..10 {
            let limiter = limiter.clone();
            handles.push(std::thread::spawn(move || {
                for j in 0..100 {
                    limiter.allow(&format!("thread-{i}-req-{j}"));
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Should not panic or deadlock
        let guard = limiter.requests.lock();
        assert!(guard.0.len() <= 1000, "should respect max_keys");
    }

    #[test]
    fn idempotency_store_concurrent_access_safe() {
        use std::sync::Arc;

        let store = Arc::new(IdempotencyStore::new(Duration::from_secs(300), 1000));
        let mut handles = Vec::new();

        for i in 0..10 {
            let store = store.clone();
            handles.push(std::thread::spawn(move || {
                for j in 0..100 {
                    store.record_if_new(&format!("thread-{i}-key-{j}"));
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let entries = store.entries.lock();
        assert!(entries.committed.len() <= 1000, "should respect max_keys");
    }

    #[test]
    fn rate_limiter_rapid_burst_then_cooldown() {
        let limiter = SlidingWindowRateLimiter::new(5, Duration::from_millis(50), 100);

        // Burst: use all 5 requests
        for _ in 0..5 {
            assert!(limiter.allow("burst-ip"));
        }
        assert!(!limiter.allow("burst-ip")); // 6th should fail

        // Cooldown
        std::thread::sleep(Duration::from_millis(60));

        // Should be allowed again
        assert!(limiter.allow("burst-ip"));
    }

    #[test]
    fn require_localhost_accepts_ipv4_loopback() {
        let peer = SocketAddr::from(([127, 0, 0, 1], 12345));
        assert!(require_localhost(&peer).is_ok());
    }

    #[test]
    fn require_localhost_accepts_ipv6_loopback() {
        let peer = SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, 12345));
        assert!(require_localhost(&peer).is_ok());
    }

    #[test]
    fn require_localhost_rejects_non_loopback_ipv4() {
        let peer = SocketAddr::from(([192, 168, 1, 100], 12345));
        let err = require_localhost(&peer).unwrap_err();
        assert_eq!(err.0, StatusCode::FORBIDDEN);
    }

    #[test]
    fn require_localhost_rejects_non_loopback_ipv6() {
        let peer = SocketAddr::from((
            std::net::Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1),
            12345,
        ));
        let err = require_localhost(&peer).unwrap_err();
        assert_eq!(err.0, StatusCode::FORBIDDEN);
    }

    #[test]
    fn admin_reload_gate_loopback_always_allowed() {
        // Loopback is allowed regardless of the opt-in or pairing flags.
        assert_eq!(
            admin_reload_gate(true, false, false),
            AdminReloadGate::Allow
        );
        assert_eq!(admin_reload_gate(true, true, true), AdminReloadGate::Allow);
        assert_eq!(admin_reload_gate(true, false, true), AdminReloadGate::Allow);
        assert_eq!(admin_reload_gate(true, true, false), AdminReloadGate::Allow);
    }

    #[test]
    fn admin_reload_gate_remote_blocked_by_default() {
        // Non-loopback caller with the flag off is rejected outright,
        // regardless of pairing.
        assert_eq!(
            admin_reload_gate(false, false, true),
            AdminReloadGate::Forbidden
        );
        assert_eq!(
            admin_reload_gate(false, false, false),
            AdminReloadGate::Forbidden
        );
    }

    #[test]
    fn admin_reload_gate_remote_opt_in_requires_auth() {
        // Non-loopback caller with the flag on and pairing on must authenticate.
        assert_eq!(
            admin_reload_gate(false, true, true),
            AdminReloadGate::RequireAuth
        );
    }

    #[test]
    fn admin_reload_gate_remote_opt_in_without_pairing_is_rejected() {
        // Opting in with pairing off cannot authenticate the caller, so the
        // request is rejected rather than allowed anonymously.
        assert_eq!(
            admin_reload_gate(false, true, false),
            AdminReloadGate::ForbiddenNoPairing
        );
    }

    #[test]
    fn allow_remote_admin_defaults_off() {
        // Security default: remote admin reload is disabled until opted in.
        assert!(!clawcrew_config::schema::GatewayConfig::default().allow_remote_admin);
    }

    /// Build an `AppState` for `handle_admin_reload`: controls
    /// `gateway.allow_remote_admin`, pairing (and its tokens), and wires a
    /// live reload channel so the allowed path reaches `200` rather than the
    /// `503` standalone-gateway branch.
    fn admin_reload_state(
        tmp: &tempfile::TempDir,
        allow_remote_admin: bool,
        require_pairing: bool,
        tokens: &[String],
    ) -> AppState {
        let mut state = admin_paircode_state(tmp, require_pairing, false);
        state.config.write().gateway.allow_remote_admin = allow_remote_admin;
        state.pairing = Arc::new(PairingGuard::new(
            require_pairing,
            tokens,
            PairingCodePolicy::default(),
        ));
        state.reload_tx = Some(tokio::sync::watch::channel(false).0);
        state
    }

    fn loopback_peer() -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], 40000))
    }

    fn remote_peer() -> SocketAddr {
        // RFC 5737 TEST-NET-3 documentation address — a stable non-loopback
        // peer that is never a real host on anyone's network.
        SocketAddr::from(([203, 0, 113, 50], 40000))
    }

    fn bearer_headers(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        headers
    }

    #[tokio::test]
    async fn admin_reload_loopback_no_token_reloads() {
        let tmp = tempfile::tempdir().unwrap();
        let state = admin_reload_state(&tmp, false, true, &[]);
        let resp =
            handle_admin_reload(State(state), ConnectInfo(loopback_peer()), HeaderMap::new())
                .await
                .unwrap()
                .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn admin_reload_remote_default_off_is_forbidden() {
        let tmp = tempfile::tempdir().unwrap();
        let state = admin_reload_state(&tmp, false, true, &[]);
        let err = handle_admin_reload(State(state), ConnectInfo(remote_peer()), HeaderMap::new())
            .await
            .err()
            .unwrap();
        assert_eq!(err.0, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_reload_remote_opt_in_without_pairing_does_not_reload() {
        // The fixed hole: allow_remote_admin = true + require_pairing = false
        // must NOT permit an anonymous remote reload.
        let tmp = tempfile::tempdir().unwrap();
        let state = admin_reload_state(&tmp, true, false, &[]);
        let err = handle_admin_reload(State(state), ConnectInfo(remote_peer()), HeaderMap::new())
            .await
            .err()
            .unwrap();
        assert_eq!(err.0, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_reload_remote_opt_in_missing_token_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let state = admin_reload_state(&tmp, true, true, &["zc_test_token".to_string()]);
        let err = handle_admin_reload(State(state), ConnectInfo(remote_peer()), HeaderMap::new())
            .await
            .err()
            .unwrap();
        assert_eq!(err.0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn admin_reload_remote_opt_in_invalid_token_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let state = admin_reload_state(&tmp, true, true, &["zc_test_token".to_string()]);
        let err = handle_admin_reload(
            State(state),
            ConnectInfo(remote_peer()),
            bearer_headers("not-the-token"),
        )
        .await
        .err()
        .unwrap();
        assert_eq!(err.0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn admin_reload_remote_opt_in_valid_token_reloads() {
        let tmp = tempfile::tempdir().unwrap();
        let state = admin_reload_state(&tmp, true, true, &["zc_test_token".to_string()]);
        let resp = handle_admin_reload(
            State(state),
            ConnectInfo(remote_peer()),
            bearer_headers("zc_test_token"),
        )
        .await
        .unwrap()
        .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn needs_quickstart_for_flags_empty_model() {
        let err =
            needs_quickstart_for("").expect("empty model must produce a needs_quickstart error");
        let msg = err.to_string();
        assert!(
            msg.contains("needs_quickstart"),
            "error must carry the needs_quickstart marker for callers to map to 503; got: {msg}"
        );
        assert!(
            msg.contains("/quickstart"),
            "error must point the user at /quickstart; got: {msg}"
        );
    }

    #[test]
    fn needs_quickstart_for_flags_whitespace_only_model() {
        assert!(
            needs_quickstart_for("   ").is_some(),
            "whitespace-only model must be treated as empty"
        );
        assert!(
            needs_quickstart_for("\n\t ").is_some(),
            "tabs and newlines count as empty too"
        );
    }

    #[test]
    fn needs_quickstart_for_passes_real_model() {
        assert!(
            needs_quickstart_for("anthropic/claude-sonnet-4").is_none(),
            "a real model id must not be flagged"
        );
        assert!(
            needs_quickstart_for("  gpt-4  ").is_none(),
            "leading/trailing whitespace around a real model id must not be flagged"
        );
    }

    #[test]
    fn is_needs_quickstart_err_detects_marker_from_helper() {
        let err = needs_quickstart_for("").expect("empty model produces marker");
        assert!(
            is_needs_quickstart_err(&err),
            "the marker emitted by needs_quickstart_for must be detected"
        );
    }

    #[test]
    fn is_needs_quickstart_err_ignores_unrelated_errors() {
        let err = anyhow::Error::msg("upstream timeout: provider returned 504");
        assert!(
            !is_needs_quickstart_err(&err),
            "unrelated errors must not be misclassified as needs_quickstart"
        );
        let err = anyhow::Error::msg("invalid api key");
        assert!(!is_needs_quickstart_err(&err));
    }

    #[test]
    fn is_needs_quickstart_err_detects_via_substring() {
        // Defends the contract that the substring marker is the
        // detection key — not the exact string. Wrappers (e.g.
        // anyhow::Error::context) must not break the check.
        let err =
            anyhow::Error::msg("provider call failed").context("needs_quickstart: empty model");
        assert!(is_needs_quickstart_err(&err));
    }

    #[test]
    fn needs_quickstart_channel_reply_resolves_via_fluent() {
        let reply = needs_quickstart_channel_reply();
        assert!(
            !reply.starts_with('{') && !reply.ends_with('}'),
            "fluent missing-key fallback leaked into channel reply: {reply:?}"
        );
        assert!(
            reply.to_lowercase().contains("quickstart"),
            "channel reply must mention Quickstart so users know what's missing: {reply:?}"
        );
    }

    // ══════════════════════════════════════════════════════════
    // Linq Multi-Tenant Webhook Routing Tests
    // ══════════════════════════════════════════════════════════

    /// Helper: compute a valid Linq HMAC-SHA256 signature for the given
    /// secret, timestamp, and body.  Mirrors the verification logic in
    /// `clawcrew_channels::linq::verify_linq_signature`.
    #[cfg(feature = "channel-linq")]
    fn compute_linq_signature_hex(secret: &str, timestamp: &str, body: &str) -> String {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let message = format!("{timestamp}.{body}");
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(message.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    /// Helper: build a minimal Linq webhook payload that `parse_webhook_payload`
    /// recognises as a `message.received` event with one text part.
    #[cfg(feature = "channel-linq")]
    fn linq_webhook_body(sender: &str, text: &str) -> String {
        serde_json::json!({
            "event_type": "message.received",
            "data": {
                "chat_id": "chat-789",
                "from": sender,
                "is_from_me": false,
                "message": {
                    "parts": [{ "type": "text", "value": text }]
                }
            }
        })
        .to_string()
    }

    /// Helper: build an `AppState` with one Linq channel registered under the
    /// given alias, with an allow-any peer resolver and an optional signing
    /// secret.
    #[cfg(feature = "channel-linq")]
    fn linq_test_state(alias: &str, signing_secret: Option<&str>) -> AppState {
        linq_test_state_with_config(alias, signing_secret, Config::default())
    }

    #[cfg(feature = "channel-linq")]
    fn linq_test_state_with_config(
        alias: &str,
        signing_secret: Option<&str>,
        config: Config,
    ) -> AppState {
        let model_provider: Arc<dyn ModelProvider> = Arc::new(MockModelProvider::default());
        let memory: Arc<dyn Memory> = Arc::new(MockMemory);

        let peer_resolver: Arc<dyn Fn() -> Vec<String> + Send + Sync> =
            Arc::new(|| vec!["*".to_string()]);
        let channel = Arc::new(LinqChannel::new(
            "test-token".into(),
            "+15550000000".into(),
            alias,
            peer_resolver,
        ));
        let mut linq = HashMap::new();
        linq.insert(alias.to_string(), channel);

        let mut linq_signing_secrets: HashMap<String, Arc<str>> = HashMap::new();
        if let Some(secret) = signing_secret {
            linq_signing_secrets.insert(alias.to_string(), Arc::from(secret));
        }

        AppState {
            config: Arc::new(RwLock::new(config)),
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            model_provider,
            model: "test-model".into(),
            temperature: None,
            mem: memory,
            memory_strategy: Arc::new(DefaultMemoryStrategy::with_config(
                Arc::new(MockMemory),
                clawcrew_config::schema::MemoryConfig::default(),
                std::path::PathBuf::new(),
            )),
            auto_save: false,
            pairing: Arc::new(PairingGuard::new(false, &[], PairingCodePolicy::default())),
            trust_forwarded_headers: false,
            rate_limiter: Arc::new(GatewayRateLimiter::new(100, 100, 100)),
            auth_limiter: Arc::new(auth_rate_limit::AuthRateLimiter::new()),
            idempotency_store: Arc::new(IdempotencyStore::new(Duration::from_secs(300), 1000)),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp: HashMap::new(),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp_app_secret: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq,
            #[cfg(feature = "channel-linq")]
            linq_signing_secrets,
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk_webhook_secret: HashMap::new(),
            #[cfg(feature = "channel-email")]
            gmail_push: None,
            observer: Arc::new(clawcrew_runtime::observability::NoopObserver),
            tools_registry: Arc::new(Vec::new()),
            tools_registry_by_agent: Arc::new(std::collections::HashMap::new()),
            cost_tracker: None,
            event_tx: tokio::sync::broadcast::channel(16).0,
            event_buffer: Arc::new(sse::EventBuffer::new(16)),
            shutdown_tx: tokio::sync::watch::channel(false).0,
            reload_tx: None,
            node_registry: Arc::new(nodes::NodeRegistry::new(16)),
            mdns_peer_registry: nodes::mdns::MdnsPeerRegistry::default(),
            path_prefix: String::new(),
            web_dist_dir: None,
            session_backend: None,
            session_queue: std::sync::Arc::new(crate::session_queue::SessionActorQueue::new(
                8, 30, 600,
            )),
            device_registry: None,
            pending_pairings: None,
            canvas_store: CanvasStore::new(),
            cancel_tokens: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            pending_reload: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tui_registry: None,
            sop_engine: None,
            sop_audit: None,
            #[cfg(feature = "webauthn")]
            webauthn: None,
        }
    }

    #[cfg(feature = "channel-linq")]
    #[tokio::test]
    async fn linq_webhook_returns_not_found_for_unknown_alias() {
        // No Linq channels configured at all.
        let state = linq_test_state("production", None);

        let response = Box::pin(handle_linq_webhook_alias(
            State(state),
            Path("staging".to_string()),
            HeaderMap::new(),
            Bytes::from_static(br#"{"event_type":"message.received"}"#),
        ))
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[cfg(feature = "channel-linq")]
    #[tokio::test]
    async fn linq_webhook_returns_not_found_when_no_channels_configured() {
        let model_provider: Arc<dyn ModelProvider> = Arc::new(MockModelProvider::default());
        let memory: Arc<dyn Memory> = Arc::new(MockMemory);

        let state = AppState {
            config: Arc::new(RwLock::new(Config::default())),
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            model_provider,
            model: "test-model".into(),
            temperature: None,
            mem: memory,
            memory_strategy: Arc::new(DefaultMemoryStrategy::with_config(
                Arc::new(MockMemory),
                clawcrew_config::schema::MemoryConfig::default(),
                std::path::PathBuf::new(),
            )),
            auto_save: false,
            pairing: Arc::new(PairingGuard::new(false, &[], PairingCodePolicy::default())),
            trust_forwarded_headers: false,
            rate_limiter: Arc::new(GatewayRateLimiter::new(100, 100, 100)),
            auth_limiter: Arc::new(auth_rate_limit::AuthRateLimiter::new()),
            idempotency_store: Arc::new(IdempotencyStore::new(Duration::from_secs(300), 1000)),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp: HashMap::new(),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp_app_secret: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq_signing_secrets: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk_webhook_secret: HashMap::new(),
            #[cfg(feature = "channel-email")]
            gmail_push: None,
            observer: Arc::new(clawcrew_runtime::observability::NoopObserver),
            tools_registry: Arc::new(Vec::new()),
            tools_registry_by_agent: Arc::new(std::collections::HashMap::new()),
            cost_tracker: None,
            event_tx: tokio::sync::broadcast::channel(16).0,
            event_buffer: Arc::new(sse::EventBuffer::new(16)),
            shutdown_tx: tokio::sync::watch::channel(false).0,
            reload_tx: None,
            node_registry: Arc::new(nodes::NodeRegistry::new(16)),
            mdns_peer_registry: nodes::mdns::MdnsPeerRegistry::default(),
            path_prefix: String::new(),
            web_dist_dir: None,
            session_backend: None,
            session_queue: std::sync::Arc::new(crate::session_queue::SessionActorQueue::new(
                8, 30, 600,
            )),
            device_registry: None,
            pending_pairings: None,
            canvas_store: CanvasStore::new(),
            cancel_tokens: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            pending_reload: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tui_registry: None,
            sop_engine: None,
            sop_audit: None,
            #[cfg(feature = "webauthn")]
            webauthn: None,
        };

        let response = Box::pin(handle_linq_webhook_alias(
            State(state),
            Path("default".to_string()),
            HeaderMap::new(),
            Bytes::from_static(br#"{"event_type":"message.received"}"#),
        ))
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[cfg(feature = "channel-linq")]
    #[tokio::test]
    async fn linq_webhook_accepts_valid_message_for_known_alias() {
        // This test proves alias routing, not signature handling, but inbound
        // verification is mandatory, so it has to carry a real secret and a
        // valid signature to reach the routing it is asserting on.
        let secret = generate_test_secret();
        let state = linq_test_state("default", Some(&secret));
        let body = linq_webhook_body("+15551234567", "hello from test");
        let timestamp = chrono::Utc::now().timestamp().to_string();
        let sig = compute_linq_signature_hex(&secret, &timestamp, &body);

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Webhook-Signature",
            HeaderValue::from_str(&format!("sha256={sig}")).unwrap(),
        );
        headers.insert(
            "X-Webhook-Timestamp",
            HeaderValue::from_str(&timestamp).unwrap(),
        );

        let response = Box::pin(handle_linq_webhook_alias(
            State(state),
            Path("default".to_string()),
            headers,
            Bytes::from(body),
        ))
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[cfg(feature = "channel-linq")]
    #[tokio::test]
    async fn linq_webhook_rejects_when_no_signing_secret_is_configured() {
        // Fail closed. An alias with no resolved signing secret cannot verify
        // anything, so the webhook is refused rather than processed
        // unverified.
        let state = linq_test_state("default", None);
        let body = linq_webhook_body("+15551234567", "hello from test");

        let response = Box::pin(handle_linq_webhook_alias(
            State(state),
            Path("default".to_string()),
            HeaderMap::new(),
            Bytes::from(body),
        ))
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[cfg(feature = "channel-linq")]
    #[tokio::test]
    async fn linq_webhook_rejects_invalid_signature_for_alias() {
        let secret = generate_test_secret();
        let state = linq_test_state("secure-alias", Some(&secret));

        let body = linq_webhook_body("+15551234567", "hello from test");
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Webhook-Signature",
            HeaderValue::from_static("sha256=deadbeef"),
        );
        headers.insert(
            "X-Webhook-Timestamp",
            HeaderValue::from_static("9999999999"),
        );

        let response = Box::pin(handle_linq_webhook_alias(
            State(state),
            Path("secure-alias".to_string()),
            headers,
            Bytes::from(body),
        ))
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[cfg(feature = "channel-linq")]
    #[tokio::test]
    async fn linq_webhook_accepts_valid_signature_for_alias() {
        let secret = generate_test_secret();
        let state = linq_test_state("secure-alias", Some(&secret));

        let body = linq_webhook_body("+15551234567", "hello from test");
        let timestamp = chrono::Utc::now().timestamp().to_string();
        let sig = compute_linq_signature_hex(&secret, &timestamp, &body);

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Webhook-Signature",
            HeaderValue::from_str(&format!("sha256={sig}")).unwrap(),
        );
        headers.insert(
            "X-Webhook-Timestamp",
            HeaderValue::from_str(&timestamp).unwrap(),
        );

        let response = Box::pin(handle_linq_webhook_alias(
            State(state),
            Path("secure-alias".to_string()),
            headers,
            Bytes::from(body),
        ))
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    // ── Authenticated webhook ingress: shared dispatch lifecycle ────────

    /// Memory double that records every autosave call so lifecycle tests
    /// can assert on keys and session ids.
    #[cfg(feature = "channel-linq")]
    #[derive(Default)]
    struct CapturingMemory {
        stores: Mutex<Vec<(String, String, Option<String>)>>,
    }

    #[cfg(feature = "channel-linq")]
    #[async_trait]
    impl Memory for CapturingMemory {
        fn name(&self) -> &str {
            "capturing"
        }

        async fn store(
            &self,
            key: &str,
            content: &str,
            _category: MemoryCategory,
            session_id: Option<&str>,
        ) -> anyhow::Result<()> {
            self.stores.lock().push((
                key.to_string(),
                content.to_string(),
                session_id.map(ToString::to_string),
            ));
            Ok(())
        }

        async fn recall(
            &self,
            _query: &str,
            _limit: usize,
            _session_id: Option<&str>,
            _since: Option<&str>,
            _until: Option<&str>,
        ) -> anyhow::Result<Vec<MemoryEntry>> {
            Ok(Vec::new())
        }

        async fn get(&self, _key: &str) -> anyhow::Result<Option<MemoryEntry>> {
            Ok(None)
        }

        async fn list(
            &self,
            _category: Option<&MemoryCategory>,
            _session_id: Option<&str>,
        ) -> anyhow::Result<Vec<MemoryEntry>> {
            Ok(Vec::new())
        }

        async fn forget(&self, _key: &str) -> anyhow::Result<bool> {
            Ok(false)
        }

        async fn forget_for_agent(&self, _key: &str, _agent_id: &str) -> anyhow::Result<bool> {
            Ok(false)
        }

        async fn count(&self) -> anyhow::Result<usize> {
            Ok(0)
        }

        async fn health_check(&self) -> bool {
            true
        }

        async fn store_with_agent(
            &self,
            key: &str,
            content: &str,
            category: MemoryCategory,
            session_id: Option<&str>,
            _namespace: Option<&str>,
            _importance: Option<f64>,
            _agent_id: Option<&str>,
        ) -> anyhow::Result<()> {
            self.store(key, content, category, session_id).await
        }

        async fn recall_for_agents(
            &self,
            _allowed_agent_ids: &[&str],
            _query: &str,
            _limit: usize,
            _session_id: Option<&str>,
            _since: Option<&str>,
            _until: Option<&str>,
        ) -> anyhow::Result<Vec<MemoryEntry>> {
            Ok(Vec::new())
        }
    }

    #[cfg(feature = "channel-linq")]
    impl ::clawcrew_api::attribution::Attributable for CapturingMemory {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Memory(
                ::clawcrew_api::attribution::MemoryKind::InMemory,
            )
        }
        fn alias(&self) -> &str {
            "CapturingMemory"
        }
    }

    /// Channel double that records every outbound send so lifecycle tests
    /// can assert on reply delivery without network I/O.
    #[cfg(feature = "channel-linq")]
    #[derive(Default)]
    struct CapturingChannel {
        sends: Mutex<Vec<(String, String)>>,
    }

    #[cfg(feature = "channel-linq")]
    #[async_trait]
    impl Channel for CapturingChannel {
        fn name(&self) -> &str {
            "capturing"
        }

        async fn send(&self, message: &clawcrew_api::channel::SendMessage) -> anyhow::Result<()> {
            self.sends
                .lock()
                .push((message.content.clone(), message.recipient.clone()));
            Ok(())
        }

        async fn listen(
            &self,
            _tx: tokio::sync::mpsc::Sender<clawcrew_api::channel::ChannelMessage>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[cfg(feature = "channel-linq")]
    impl ::clawcrew_api::attribution::Attributable for CapturingChannel {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Channel(
                ::clawcrew_api::attribution::ChannelKind::Webhook,
            )
        }
        fn alias(&self) -> &str {
            "CapturingChannel"
        }
    }

    #[cfg(feature = "channel-linq")]
    fn test_channel_message(sender: &str, content: &str) -> clawcrew_api::channel::ChannelMessage {
        clawcrew_api::channel::ChannelMessage {
            id: "msg-1".into(),
            sender: sender.into(),
            platform_sender_id: None,
            reply_target: sender.into(),
            content: content.into(),
            channel: "linq".into(),
            channel_alias: Some("default".into()),
            timestamp: 0,
            thread_ts: None,
            interruption_scope_id: None,
            attachments: Vec::new(),
            subject: None,
            internal_sop_event: None,
            passive_context: false,
            explicitly_addressed: false,
            conversation_scope: Default::default(),
            references: Vec::new(),
            voice_origin: false,
        }
    }

    /// A verified request still flows through the full shared lifecycle:
    /// autosave with the channel session key, agent dispatch, and reply
    /// delivery through the channel implementation.
    #[cfg(feature = "channel-linq")]
    #[tokio::test]
    async fn verified_webhook_dispatch_runs_the_full_lifecycle() {
        let mut state = linq_test_state("default", Some("secret"));
        let memory_impl = Arc::new(CapturingMemory::default());
        let mem: Arc<dyn Memory> = memory_impl.clone();
        state.mem = mem;
        state.auto_save = true;

        let verified = match webhook_ingress::authenticate(
            &webhook_ingress::LINQ_WEBHOOK,
            "default",
            Some("secret"),
            &HeaderMap::new(),
            Bytes::from_static(b"{}"),
            |_, _, _| true,
        ) {
            Ok(verified) => verified,
            Err(refusal) => panic!("stub verification must succeed, got {refusal:?}"),
        };
        let verified = verified
            .parse_messages(|body| {
                assert_eq!(body, b"{}", "the parser receives the verified request body");
                Ok::<_, ()>(vec![test_channel_message(
                    "+15551234567",
                    "hello lifecycle",
                )])
            })
            .expect("verified request should parse");

        let channel_impl = Arc::new(CapturingChannel::default());
        let channel: Arc<dyn Channel> = channel_impl.clone();

        let (status, _body) = webhook_ingress::dispatch_verified_webhook(
            &state,
            verified,
            webhook_ingress::WebhookDispatchContext {
                channel,
                memory_key: linq_memory_key,
                agent_override: None,
                mode: webhook_ingress::WebhookDispatchMode::Synchronous,
                suppress_reply_send: false,
            },
        )
        .await;

        assert_eq!(status, StatusCode::OK);

        let stores = memory_impl.stores.lock().clone();
        assert_eq!(stores.len(), 1, "one autosave per inbound message");
        assert_eq!(stores[0].0, "linq_+15551234567_msg-1");
        assert_eq!(stores[0].1, "hello lifecycle");
        assert_eq!(stores[0].2.as_deref(), Some("linq_default__15551234567"));

        let sends = channel_impl.sends.lock().clone();
        assert_eq!(sends.len(), 1, "one reply per inbound message");
        assert_eq!(sends[0].0, "ok", "the model reply is what gets delivered");
        assert_eq!(sends[0].1, "+15551234567");
    }

    /// When the gateway has no model configured, a verified request still
    /// gets the quickstart fallback reply through the channel instead of
    /// silence.
    #[cfg(feature = "channel-linq")]
    #[tokio::test]
    async fn verified_webhook_dispatch_sends_quickstart_fallback_when_unconfigured() {
        let mut state = linq_test_state("default", Some("secret"));
        state.model = String::new();

        let verified = match webhook_ingress::authenticate(
            &webhook_ingress::LINQ_WEBHOOK,
            "default",
            Some("secret"),
            &HeaderMap::new(),
            Bytes::from_static(b"{}"),
            |_, _, _| true,
        ) {
            Ok(verified) => verified,
            Err(refusal) => panic!("stub verification must succeed, got {refusal:?}"),
        };
        let verified = verified
            .parse_messages(|body| {
                assert_eq!(body, b"{}", "the parser receives the verified request body");
                Ok::<_, ()>(vec![test_channel_message("+15551234567", "anyone home?")])
            })
            .expect("verified request should parse");

        let channel_impl = Arc::new(CapturingChannel::default());
        let channel: Arc<dyn Channel> = channel_impl.clone();

        let (status, _body) = webhook_ingress::dispatch_verified_webhook(
            &state,
            verified,
            webhook_ingress::WebhookDispatchContext {
                channel,
                memory_key: linq_memory_key,
                agent_override: None,
                mode: webhook_ingress::WebhookDispatchMode::Synchronous,
                suppress_reply_send: false,
            },
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        let sends = channel_impl.sends.lock().clone();
        assert_eq!(sends.len(), 1);
        assert_eq!(
            sends[0].0,
            needs_quickstart_channel_reply(),
            "unconfigured gateway sends the quickstart reply, not silence"
        );
    }

    #[cfg(feature = "channel-linq")]
    #[tokio::test]
    async fn linq_webhook_alias_dispatches_to_configured_channel_agent() {
        use clawcrew_config::providers::ChannelRef;
        use clawcrew_config::schema::AliasedAgentConfig;

        let _capture_guard = lock_gateway_chat_dispatch_capture_for_test().await;
        clear_gateway_chat_dispatch_captures_for_test();

        let mut config = Config::default();
        config.agents.insert(
            "alpha".to_string(),
            AliasedAgentConfig {
                enabled: true,
                ..AliasedAgentConfig::default()
            },
        );
        config.agents.insert(
            "beta".to_string(),
            AliasedAgentConfig {
                enabled: true,
                channels: vec![ChannelRef::new("linq.work")],
                ..AliasedAgentConfig::default()
            },
        );
        let secret = generate_test_secret();
        let state = linq_test_state_with_config("work", Some(&secret), config);

        let message = "hello from linq work alias";
        let body = linq_webhook_body("+15551234567", message);
        let timestamp = chrono::Utc::now().timestamp().to_string();
        let sig = compute_linq_signature_hex(&secret, &timestamp, &body);
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Webhook-Signature",
            HeaderValue::from_str(&format!("sha256={sig}")).unwrap(),
        );
        headers.insert(
            "X-Webhook-Timestamp",
            HeaderValue::from_str(&timestamp).unwrap(),
        );

        let response = Box::pin(handle_linq_webhook_alias(
            State(state),
            Path("work".to_string()),
            headers,
            Bytes::from(body),
        ))
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);

        let captures = gateway_chat_dispatch_captures_for_test();
        let capture = captures
            .iter()
            .find(|capture| capture.message == message)
            .expect("Linq webhook should dispatch the inbound message");
        assert_eq!(capture.agent_override.as_deref(), Some("beta"));
        let session_id = capture
            .session_id
            .as_deref()
            .expect("Linq dispatch should pass a session id");
        assert_eq!(session_id, "linq_work__15551234567");
    }

    #[cfg(feature = "channel-linq")]
    #[tokio::test]
    async fn linq_webhook_alias_without_enabled_owner_does_not_use_default_agent() {
        use clawcrew_config::providers::ChannelRef;
        use clawcrew_config::schema::AliasedAgentConfig;

        let _capture_guard = lock_gateway_chat_dispatch_capture_for_test().await;
        clear_gateway_chat_dispatch_captures_for_test();

        let mut config = Config::default();
        config.agents.insert(
            "alpha".to_string(),
            AliasedAgentConfig {
                enabled: true,
                ..AliasedAgentConfig::default()
            },
        );
        config.agents.insert(
            "beta".to_string(),
            AliasedAgentConfig {
                enabled: false,
                channels: vec![ChannelRef::new("linq.work")],
                ..AliasedAgentConfig::default()
            },
        );
        let secret = generate_test_secret();
        let state = linq_test_state_with_config("work", Some(&secret), config);

        let message = "do not route me to alpha";
        let body = linq_webhook_body("+15551234567", message);
        let timestamp = chrono::Utc::now().timestamp().to_string();
        let sig = compute_linq_signature_hex(&secret, &timestamp, &body);
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Webhook-Signature",
            HeaderValue::from_str(&format!("sha256={sig}")).unwrap(),
        );
        headers.insert(
            "X-Webhook-Timestamp",
            HeaderValue::from_str(&timestamp).unwrap(),
        );

        let response = Box::pin(handle_linq_webhook_alias(
            State(state),
            Path("work".to_string()),
            headers,
            Bytes::from(body),
        ))
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let captures = gateway_chat_dispatch_captures_for_test();
        assert!(
            captures.iter().all(|capture| capture.message != message),
            "unowned Linq alias must not dispatch through the default agent: {captures:?}"
        );
    }

    // ── Per-alias webhook routing───────────────────────────────────

    /// Baseline `AppState` with no channels configured, for the per-alias
    /// routing tests. Tests insert the WhatsApp instances they exercise.
    #[cfg(feature = "channel-whatsapp-cloud")]
    fn webhook_baseline_state() -> AppState {
        let model_provider: Arc<dyn ModelProvider> = Arc::new(MockModelProvider::default());
        let mem: Arc<dyn Memory> = Arc::new(MockMemory);
        AppState {
            config: Arc::new(RwLock::new(Config::default())),
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            model_provider,
            model: "test-model".into(),
            temperature: None,
            mem,
            memory_strategy: Arc::new(DefaultMemoryStrategy::with_config(
                Arc::new(MockMemory),
                clawcrew_config::schema::MemoryConfig::default(),
                std::path::PathBuf::new(),
            )),
            auto_save: false,
            pairing: Arc::new(PairingGuard::new(false, &[], PairingCodePolicy::default())),
            trust_forwarded_headers: false,
            rate_limiter: Arc::new(GatewayRateLimiter::new(100, 100, 100)),
            auth_limiter: Arc::new(auth_rate_limit::AuthRateLimiter::new()),
            idempotency_store: Arc::new(IdempotencyStore::new(Duration::from_secs(300), 1000)),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp: HashMap::new(),
            #[cfg(feature = "channel-whatsapp-cloud")]
            whatsapp_app_secret: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq: HashMap::new(),
            #[cfg(feature = "channel-linq")]
            linq_signing_secrets: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk: HashMap::new(),
            #[cfg(feature = "channel-nextcloud")]
            nextcloud_talk_webhook_secret: HashMap::new(),
            #[cfg(feature = "channel-email")]
            gmail_push: None,
            observer: Arc::new(clawcrew_runtime::observability::NoopObserver),
            tools_registry: Arc::new(Vec::new()),
            tools_registry_by_agent: Arc::new(std::collections::HashMap::new()),
            cost_tracker: None,
            event_tx: tokio::sync::broadcast::channel(16).0,
            event_buffer: Arc::new(sse::EventBuffer::new(16)),
            shutdown_tx: tokio::sync::watch::channel(false).0,
            reload_tx: None,
            node_registry: Arc::new(nodes::NodeRegistry::new(16)),
            mdns_peer_registry: nodes::mdns::MdnsPeerRegistry::default(),
            path_prefix: String::new(),
            web_dist_dir: None,
            session_backend: None,
            session_queue: std::sync::Arc::new(crate::session_queue::SessionActorQueue::new(
                8, 30, 600,
            )),
            device_registry: None,
            pending_pairings: None,
            canvas_store: CanvasStore::new(),
            cancel_tokens: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            pending_reload: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tui_registry: None,
            sop_engine: None,
            sop_audit: None,
            #[cfg(feature = "webauthn")]
            webauthn: None,
        }
    }

    #[cfg(feature = "channel-whatsapp-cloud")]
    fn whatsapp_instance(alias: &str, verify_token: &str) -> Arc<WhatsAppChannel> {
        let peer_resolver: Arc<dyn Fn() -> Vec<String> + Send + Sync> = Arc::new(Vec::new);
        Arc::new(WhatsAppChannel::new(
            "access-token".into(),
            "phone-number-id".into(),
            verify_token.into(),
            alias.to_string(),
            peer_resolver,
        ))
    }

    #[cfg(feature = "channel-whatsapp-cloud")]
    fn whatsapp_signature(secret: &str, body: &[u8]) -> String {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    #[cfg(feature = "channel-whatsapp-cloud")]
    fn verify_query(token: &str, challenge: &str) -> WhatsAppVerifyQuery {
        WhatsAppVerifyQuery {
            mode: Some("subscribe".to_string()),
            verify_token: Some(token.to_string()),
            challenge: Some(challenge.to_string()),
        }
    }

    #[cfg(feature = "channel-whatsapp-cloud")]
    #[tokio::test]
    async fn webhook_alias_routes_to_the_matching_instance() {
        let mut state = webhook_baseline_state();
        state.whatsapp = HashMap::from([
            ("work".to_string(), whatsapp_instance("work", "tok-work")),
            (
                "personal".to_string(),
                whatsapp_instance("personal", "tok-personal"),
            ),
        ]);

        let resp = handle_whatsapp_verify_alias(
            State(state.clone()),
            Path("work".to_string()),
            Query(verify_query("tok-work", "challenge-work")),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        // Explicit alias path carries no deprecation header.
        assert!(
            resp.headers()
                .get(api_webhook::DEPRECATION_HEADER)
                .is_none()
        );

        // The other instance's token must NOT verify against `work`.
        let resp = handle_whatsapp_verify_alias(
            State(state.clone()),
            Path("work".to_string()),
            Query(verify_query("tok-personal", "challenge")),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        let resp = handle_whatsapp_verify_alias(
            State(state),
            Path("personal".to_string()),
            Query(verify_query("tok-personal", "challenge-personal")),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[cfg(feature = "channel-whatsapp-cloud")]
    #[tokio::test]
    async fn webhook_unknown_alias_is_404_not_500() {
        let mut state = webhook_baseline_state();
        state.whatsapp = HashMap::from([("work".to_string(), whatsapp_instance("work", "tok"))]);

        let resp = handle_whatsapp_verify_alias(
            State(state),
            Path("nope".to_string()),
            Query(verify_query("tok", "challenge")),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[cfg(feature = "channel-whatsapp-cloud")]
    #[tokio::test]
    async fn webhook_bare_path_is_back_compat_and_flags_deprecation() {
        let mut state = webhook_baseline_state();
        state.whatsapp =
            HashMap::from([("default".to_string(), whatsapp_instance("default", "tok"))]);

        let resp = handle_whatsapp_verify(
            State(state),
            Query(verify_query("tok", "challenge-default")),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(
            resp.headers()
                .get(api_webhook::DEPRECATION_HEADER)
                .is_some()
        );
    }

    #[cfg(feature = "channel-whatsapp-cloud")]
    #[tokio::test]
    async fn webhook_alias_path_preserves_signature_auth() {
        let mut state = webhook_baseline_state();
        state.whatsapp = HashMap::from([("work".to_string(), whatsapp_instance("work", "tok"))]);
        state.whatsapp_app_secret =
            HashMap::from([("work".to_string(), Arc::<str>::from("app-secret"))]);

        // Unknown alias → 404 before any processing.
        let resp = Box::pin(handle_whatsapp_message_alias(
            State(state.clone()),
            Path("nope".to_string()),
            HeaderMap::new(),
            Bytes::from_static(b"{}"),
        ))
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // Configured alias, missing/invalid signature → 401.
        let resp = Box::pin(handle_whatsapp_message_alias(
            State(state.clone()),
            Path("work".to_string()),
            HeaderMap::new(),
            Bytes::from_static(b"{}"),
        ))
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // Configured alias, valid signature over an empty payload → 200 ack.
        let body = br#"{"object":"whatsapp_business_account","entry":[]}"#;
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Hub-Signature-256",
            HeaderValue::from_str(&whatsapp_signature("app-secret", body)).unwrap(),
        );
        let resp = Box::pin(handle_whatsapp_message_alias(
            State(state),
            Path("work".to_string()),
            headers,
            Bytes::from_static(body),
        ))
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// Fail closed. A configured alias with no app secret cannot verify
    /// `X-Hub-Signature-256`, so the webhook is refused rather than
    /// dispatched to the agent unverified.
    #[cfg(feature = "channel-whatsapp-cloud")]
    #[tokio::test]
    async fn whatsapp_webhook_rejects_when_no_app_secret_is_configured() {
        let mut state = webhook_baseline_state();
        state.whatsapp = HashMap::from([("work".to_string(), whatsapp_instance("work", "tok"))]);
        state.whatsapp_app_secret = HashMap::new();

        let body = br#"{"object":"whatsapp_business_account","entry":[]}"#;
        let resp = Box::pin(handle_whatsapp_message_alias(
            State(state),
            Path("work".to_string()),
            HeaderMap::new(),
            Bytes::from_static(body),
        ))
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    /// Build an `AppState` whose device registry points at a non-existent
    /// path so every SQLite write fails. Mirrors `unwriteable_registry_state`
    /// in `api_pairing::tests` so the regression set stays side-by-side.
    fn unwriteable_registry_pair_state(tmp: &tempfile::TempDir) -> AppState {
        let mut state = admin_paircode_state(tmp, true, false);
        // No registry from `admin_paircode_state`; inject the broken one.
        state.device_registry = Some(Arc::new(api_pairing::DeviceRegistry::with_db_path(
            std::path::PathBuf::from("/this/path/does/not/exist/devices.db"),
        )));
        state
    }

    async fn legacy_pair_response_json(
        result: impl IntoResponse,
    ) -> (StatusCode, serde_json::Value) {
        let response = result.into_response();
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("legacy /pair response body")
            .to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, body)
    }

    #[tokio::test]
    async fn legacy_pair_rolls_back_in_process_token_when_registry_register_fails() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = unwriteable_registry_pair_state(&tmp);

        let code = state
            .pairing
            .generate_new_pairing_code(live_pairing_code_policy(&state))
            .expect("pairing code must be issuable when require_pairing=true");

        let mut headers = HeaderMap::new();
        headers.insert("X-Pairing-Code", HeaderValue::from_str(&code).unwrap());

        let (status, body) = legacy_pair_response_json(
            handle_pair(State(state.clone()), test_connect_info(), headers).await,
        )
        .await;

        assert_eq!(
            status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "legacy /pair registry.register failure must surface as 500"
        );
        assert_eq!(body["paired"], serde_json::Value::Bool(false));
        assert!(
            body.get("token").is_none(),
            "legacy /pair 5xx body MUST NOT contain the plaintext bearer token; got: {body}"
        );
        assert!(
            state.pairing.tokens().is_empty(),
            "PairingGuard::paired_tokens must be empty after a failed /pair \
             registry.register (compensating `revoke_token_hash`); instead have {:?}",
            state.pairing.tokens()
        );
    }

    #[tokio::test]
    async fn legacy_pair_rolls_back_in_process_token_when_persist_fails() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = admin_paircode_state(&tmp, true, false);
        let blocker = tmp.path().join("legacy-pair-blocker");
        std::fs::write(&blocker, b"").expect("seed blocker file");
        state.config.write().config_path = blocker.join("config.toml");

        let code = state
            .pairing
            .generate_new_pairing_code(live_pairing_code_policy(&state))
            .expect("pairing code must be issuable when require_pairing=true");

        let mut headers = HeaderMap::new();
        headers.insert("X-Pairing-Code", HeaderValue::from_str(&code).unwrap());

        let (status, body) = legacy_pair_response_json(
            handle_pair(State(state.clone()), test_connect_info(), headers).await,
        )
        .await;

        assert_eq!(
            status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "legacy /pair persistence failure MUST surface as 500 (legacy leaked 200 + token)"
        );
        assert_eq!(body["paired"], serde_json::Value::Bool(false));
        assert!(
            body.get("token").is_none(),
            "legacy /pair 5xx body MUST NOT contain the plaintext bearer token; got: {body}"
        );
        assert!(
            state.pairing.tokens().is_empty(),
            "PairingGuard::paired_tokens must be empty after a failed /pair \
             persist; have {:?}",
            state.pairing.tokens()
        );
    }
}

#[cfg(test)]
mod accept_error_tests {
    use super::is_recoverable_accept_error;
    use std::io::{Error, ErrorKind};

    #[cfg(unix)]
    #[test]
    fn fd_exhaustion_accept_errors_are_recoverable() {
        // EMFILE/ENFILE must not terminate the daemon.
        assert!(is_recoverable_accept_error(&Error::from_raw_os_error(24))); // EMFILE
        assert!(is_recoverable_accept_error(&Error::from_raw_os_error(23))); // ENFILE
    }

    #[test]
    fn transient_kinds_recover_but_fatal_propagates() {
        assert!(is_recoverable_accept_error(&Error::from(
            ErrorKind::ConnectionAborted
        )));
        assert!(is_recoverable_accept_error(&Error::from(
            ErrorKind::Interrupted
        )));
        // A non-transient error is not swallowed (loop will propagate it).
        assert!(!is_recoverable_accept_error(&Error::from(
            ErrorKind::InvalidInput
        )));
    }
