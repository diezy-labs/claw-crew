use anyhow::Result;
use std::net::SocketAddr;

use crate::state::GatewaySupervision;

/// Binds the `SystemGateway` gRPC server on `127.0.0.1:50052` and serves it
/// until the process exits. D1: lets the Go engine call back into Rust for
/// native-tool execution (bash/git/file I/O). Loopback-only — this is an
/// internal engine<->gateway channel, never exposed on a public interface.
pub async fn start_grpc_server() -> Result<()> {
    use crate::grpc_system_gateway::system_gateway_server::SystemGatewayServer;

    let addr: SocketAddr = "127.0.0.1:50052".parse().expect("valid loopback addr");
    ::clawcrew_log::record!(
        INFO,
        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
        "ClawCrew SystemGateway gRPC server listening on {addr}"
    );

    tonic::transport::Server::builder()
        .add_service(SystemGatewayServer::new(
            crate::grpc_system_gateway::SystemGatewayService::new(None),
        ))
        .serve(addr)
        .await?;
    Ok(())
}

/// Run the HTTP gateway using axum with proper HTTP/1.1 compliance.
pub async fn run_gateway(
    host: &str,
    port: u16,
    config: crate::Config,
    external_event_tx: Option<tokio::sync::broadcast::Sender<serde_json::Value>>,
    reload_controls: Option<clawcrew_runtime::daemon::GatewayReloadControls>,
    tui_registry: Option<std::sync::Arc<clawcrew_runtime::rpc::tui_identity::TuiRegistry>>,
    canvas_store: Option<crate::CanvasStore>,
    sop_engine: Option<std::sync::Arc<std::sync::Mutex<clawcrew_runtime::sop::SopEngine>>>,
    sop_audit: Option<std::sync::Arc<clawcrew_runtime::sop::SopAuditLogger>>,
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
            std::sync::Arc::new(clawcrew_api::webhook::PluginWebhookRegistry::new()),
        ),
    ))
    .await
}

/// Run the supervised gateway with the daemon generation's channel-plugin
/// webhook registry.
#[allow(clippy::too_many_lines)]
pub async fn run_gateway_with_plugin_webhooks(
    host: &str,
    port: u16,
    config: crate::Config,
    external_event_tx: Option<tokio::sync::broadcast::Sender<serde_json::Value>>,
    reload_controls: Option<clawcrew_runtime::daemon::GatewayReloadControls>,
    tui_registry: Option<std::sync::Arc<clawcrew_runtime::rpc::tui_identity::TuiRegistry>>,
    canvas_store: Option<crate::CanvasStore>,
    sop_engine: Option<std::sync::Arc<std::sync::Mutex<clawcrew_runtime::sop::SopEngine>>>,
    sop_audit: Option<std::sync::Arc<clawcrew_runtime::sop::SopAuditLogger>>,
    supervision: GatewaySupervision,
) -> Result<()> {
    // Implementation is in lib.rs - gateway.rs provides the module interface
    // and extracted types. The full implementation is kept in lib.rs to avoid
    // breaking the build during the edit-only refactoring phase.
    unimplemented!("Gateway implementation kept in lib.rs for now")
}
