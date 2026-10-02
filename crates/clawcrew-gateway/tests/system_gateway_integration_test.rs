//! Integration test for `SystemGateway` (D1): boots the REAL `tonic`
//! server used in production (`clawcrew_gateway::start_grpc_server`'s own
//! `SystemGatewayService`, wired the same way) on a loopback TCP port, then
//! drives it with a real gRPC client over the wire — the same path the Go
//! engine's `pkg/client.SystemGatewayClient` exercises in production.
//!
//! This crate is server-only (`build_client(false)` in `build.rs`: the Go
//! engine owns the client), so there's no generated Rust client to import.
//! Rather than flip that crate-wide decision just for a test, this test
//! drives the server with `tonic::client::Grpc` directly against the
//! already-generated request/response prost types and the service's own
//! documented gRPC path (`clawcrew.agent.SystemGateway/<Method>`) — proving
//! the real network+codec round trip without adding a client dependency.

use clawcrew_gateway::grpc_system_gateway::{
    system_gateway_server::SystemGatewayServer, SystemGatewayService, ToolCallRequest,
    ToolCallResponse,
};
use std::net::SocketAddr;
use std::time::Duration;
use tonic::codec::ProstCodec;
use tonic::transport::{Channel, Endpoint, Server};

/// Boots the real server on an OS-assigned loopback port and returns a
/// connected channel plus the server's shutdown handle.
async fn spawn_server() -> (Channel, tokio::task::JoinHandle<()>, SocketAddr) {
    // Bind on port 0 first just to let the OS pick a free port, then hand
    // that exact address to `Server::serve` (which does its own bind) —
    // avoids pulling in tokio-stream's `net` feature for serve_with_incoming.
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let addr = probe.local_addr().expect("local addr");
    drop(probe);

    let handle = tokio::spawn(async move {
        Server::builder()
            .add_service(SystemGatewayServer::new(SystemGatewayService::new(None)))
            .serve(addr)
            .await
            .expect("gRPC server exited unexpectedly");
    });

    // Give the listener a moment to actually start accepting.
    tokio::time::sleep(Duration::from_millis(100)).await;

    let channel = Endpoint::from_shared(format!("http://{addr}"))
        .expect("valid endpoint")
        .connect()
        .await
        .expect("connect to the server we just booted");

    (channel, handle, addr)
}

/// Minimal unary gRPC call against `SystemGateway/ExecuteNativeTool`,
/// without pulling in a generated client — see module docs.
async fn execute_native_tool(
    channel: Channel,
    tool_name: &str,
    arguments_json: &str,
) -> ToolCallResponse {
    let mut grpc = tonic::client::Grpc::new(channel);
    let path = http::uri::PathAndQuery::from_static(
        "/clawcrew.agent.SystemGateway/ExecuteNativeTool",
    );
    grpc.ready().await.expect("channel ready");
    let request = tonic::Request::new(ToolCallRequest {
        tool_name: tool_name.to_string(),
        arguments_json: arguments_json.to_string(),
    });
    grpc.unary(request, path, ProstCodec::default())
        .await
        .expect("unary call succeeds")
        .into_inner()
}

#[tokio::test]
async fn go_callable_grpc_surface_executes_real_bash_command() {
    let (channel, server, _addr) = spawn_server().await;

    let resp = execute_native_tool(channel, "bash", r#"{"command":"echo roundtrip-ok"}"#).await;

    assert!(resp.success, "server rejected the call: {}", resp.error);
    assert!(
        resp.output.contains("roundtrip-ok"),
        "expected real bash stdout over the wire, got: {}",
        resp.output
    );

    server.abort();
}

#[tokio::test]
async fn go_callable_grpc_surface_rejects_tool_outside_allowlist() {
    let (channel, server, _addr) = spawn_server().await;

    let resp = execute_native_tool(channel, "rm_rf", "{}").await;

    assert!(!resp.success);
    assert!(resp.error.contains("not in the allowlist"));

    server.abort();
}
