//! End-to-end LOAD proof for the nostr pilot channel plugin.
//!
//! Minimal counterpart to `channel_plugin_e2e.rs`: it builds the pilot fixture
//! (a workspace member, so `--locked` resolves from the root lockfile) into a
//! separate target directory, admits it through the real `PluginHost`, and
//! instantiates `WasmChannel::from_wasm`. That proves the pilot is actually
//! LOADED by the host adapter — not merely that its wasm compiles — and that
//! its guest-reported capabilities surface through the host channel.

#![cfg(feature = "plugins-wasm-cranelift")]

mod support;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use clawcrew_api::attribution::Attributable;
use clawcrew_api::channel::Channel;
use clawcrew_plugins::component::PluginLimits;
use clawcrew_plugins::config::{PluginConfigResolver, resolve_plugin_config};
use clawcrew_plugins::endpoint::PluginChannelEndpoint;
use clawcrew_plugins::instance::PluginInstanceScope;
use clawcrew_plugins::services::PluginHostServices;
use clawcrew_plugins::wasm_channel::WasmChannel;
use clawcrew_plugins::{PluginCapability, PluginManifest, PluginPermission};

use support::admit_fixture;

/// Build the pilot fixture into its own target dir once, mirroring the
/// harness's isolated nested-Cargo build so it cannot contend with the host
/// test process's build lock.
fn fixture() -> PathBuf {
    static FIXTURE: OnceLock<PathBuf> = OnceLock::new();
    FIXTURE
        .get_or_init(|| {
            let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/channel-nostr-pilot");
            let target_dir =
                PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("channel-nostr-pilot");
            let status = Command::new(env!("CARGO"))
                .current_dir(&fixture_dir)
                .args([
                    "build",
                    "--locked",
                    "--quiet",
                    "--package",
                    "clawcrew-channel-nostr-pilot",
                    "--target",
                    "wasm32-wasip2",
                    "--target-dir",
                ])
                .arg(&target_dir)
                .status()
                .expect("run Cargo for the nostr pilot channel fixture");
            assert!(
                status.success(),
                "nostr pilot fixture must build; install the wasm32-wasip2 target"
            );

            let wasm =
                target_dir.join("wasm32-wasip2/debug/clawcrew_channel_nostr_pilot.wasm");
            assert!(wasm.is_file(), "nostr pilot fixture WASM was not produced");
            wasm
        })
        .clone()
}

fn limits() -> PluginLimits {
    PluginLimits {
        call_fuel: 1_000_000_000,
        max_memory_bytes: 64 * 1024 * 1024,
        max_table_elements: 10_000,
        max_instances: 32,
        call_timeout: Duration::from_secs(30),
    }
}

/// Manifest mirroring the pilot's `plugin-manifest.toml` config schema:
/// `relay_url` (public) + `relay_secret` (x-secret). `config_read` grants both
/// `config.get` and `secrets.get`; channels have no outbound-HTTP surface.
fn manifest() -> PluginManifest {
    PluginManifest {
        name: "channel-nostr-pilot".to_string(),
        version: "0.0.0".to_string(),
        description: None,
        author: None,
        wasm_path: Some("channel-nostr-pilot.wasm".to_string()),
        wasm_sha256: None,
        capabilities: vec![PluginCapability::Channel],
        permissions: vec![PluginPermission::ConfigRead],
        config_schema: Some(serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "required": ["relay_url", "relay_secret"],
            "additionalProperties": false,
            "properties": {
                "relay_url": {"type": "string", "minLength": 1},
                "relay_secret": {"type": "string", "minLength": 1, "x-secret": true}
            }
        })),
        signature: None,
        publisher_key: None,
        egress: Default::default(),
    }
}

type InstanceConfig = HashMap<String, String>;
type CanonicalConfig = Arc<RwLock<HashMap<String, InstanceConfig>>>;

fn canonical_config(binding: &str) -> CanonicalConfig {
    let values = HashMap::from([
        ("relay_url".to_string(), "wss://relay.example".to_string()),
        ("relay_secret".to_string(), format!("secret-{binding}")),
    ]);
    Arc::new(RwLock::new(HashMap::from([(binding.to_string(), values)])))
}

fn host_services(config: CanonicalConfig) -> PluginHostServices {
    let manifest = manifest();
    let resolver = PluginConfigResolver::new(move |scope| {
        let configured = config.read().expect("lock canonical pilot config");
        let values = configured.get(scope.id().binding()).ok_or_else(|| {
            clawcrew_plugins::error::PluginError::InvalidConfig(
                "missing canonical pilot binding".to_string(),
            )
        })?;
        resolve_plugin_config(&manifest, scope, Some(values))
    });
    PluginHostServices::new(resolver)
}

async fn build_channel(binding: &str, services: &PluginHostServices) -> WasmChannel {
    let manifest = manifest();
    let scope = PluginInstanceScope::from_manifest(
        &manifest,
        PluginCapability::Channel,
        binding,
        manifest.permissions.iter().copied(),
    )
    .expect("admit pilot scope");
    let endpoint = PluginChannelEndpoint::new(scope, "plugin").expect("bind pilot endpoint");

    let component = admit_fixture(&fixture(), &manifest);
    WasmChannel::from_wasm(endpoint, &component, services, limits(), None)
        .await
        .expect("instantiate pilot channel")
}

/// The pilot's wasm is BUILT, ADMITTED by the real `PluginHost`, and INSTANTIATED
/// as a live `WasmChannel`. Its guest-reported identity and webhook ingress
/// surface are then observed through the host channel — proving the component is
/// loaded end to end, not merely compiled.
#[tokio::test]
async fn nostr_pilot_channel_loads_and_exposes_webhook_ingress() {
    let config = canonical_config("main");
    let services = host_services(config);
    let channel = build_channel("main", &services).await;

    // Host endpoint name + admitted alias (the guest-reported `name()` is the
    // channel's own identity; the host surfaces the endpoint name "plugin").
    assert_eq!(channel.name(), "plugin");
    assert_eq!(channel.alias(), "main");

    // The pilot declares WEBHOOK_INGRESS and a webhook path; both must surface
    // through the loaded host channel.
    assert!(channel.has_webhook_ingress());
    assert_eq!(
        channel.webhook_path().await.expect("query pilot webhook path"),
        Some("nostr-pilot".to_string())
    );
}
