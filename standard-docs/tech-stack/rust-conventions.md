# Rust Conventions

**Status:** Normative (code MUST follow this)  
**Last updated:** 2026-10-03  
**Rust edition:** 2024  
**MSRV:** 1.82+

---

## Cargo Workspace Structure

```
crates/
├── clawcrew-gateway/         # gRPC server, vault, secret management
├── clawcrew-runtime/         # Agent turn execution, tool dispatch
├── clawcrew-providers/       # LLM vendor APIs (SSOT ADR 0001)
├── clawcrew-channel-*/       # Channel plugins (Discord, Telegram, etc.)
├── clawcrew-tools-*/         # Native tools (filesystem, shell, network)
└── [25 total crates]
```

**Root `Cargo.toml`:**
```toml
[workspace]
members = [
    "crates/clawcrew-gateway",
    "crates/clawcrew-runtime",
    "crates/clawcrew-providers",
    "crates/clawcrew-channel-*",
    "crates/clawcrew-tools-*",
]
resolver = "2"

[workspace.package]
edition = "2024"
rust-version = "1.82"
license = "Apache-2.0"

[workspace.dependencies]
# Shared versions (single SSOT for deps)
tokio = { version = "1.40", features = ["full"] }
tonic = "0.12"
serde = { version = "1.0", features = ["derive"] }
```

**Convention:** New workspace dependencies go in `[workspace.dependencies]`, crates reference with `dep.workspace = true`.

---

## Crate Boundaries (Clean Architecture)

### Gateway Crate (`clawcrew-gateway`)

**Owns:**
- gRPC server `SystemGatewayService` :50052
- Vault (secrets encryption/decryption)
- WebSocket server (real-time message streaming)
- mDNS A2A discovery

**Exports:**
```rust
pub struct Gateway {
    vault: Arc<Vault>,
    runtime: Arc<Runtime>, // clawcrew-runtime
}

pub trait VaultService {
    async fn get_secret(&self, key: &str) -> Result<String>;
    async fn set_secret(&self, key: &str, value: &str) -> Result<()>;
}
```

**Dependencies:**
```toml
[dependencies]
clawcrew-runtime.workspace = true
clawcrew-providers.workspace = true
tonic.workspace = true
tokio.workspace = true
```

---

### Runtime Crate (`clawcrew-runtime`)

**Owns:**
- Agent turn execution (`execute_turn`)
- Tool dispatch (`ToolDispatcher`)
- Message streaming
- WASI plugin runtime

**Exports:**
```rust
pub async fn execute_turn(
    model: &str,
    messages: Vec<Message>,
    tools: Vec<Tool>,
) -> Result<TurnResponse>;

pub trait ToolExecutor {
    async fn execute(&self, tool: &str, args: Value) -> Result<Value>;
}
```

**Dependencies:**
```toml
[dependencies]
clawcrew-providers.workspace = true
clawcrew-tools-shell.workspace = true
clawcrew-tools-filesystem.workspace = true
tokio.workspace = true
serde_json.workspace = true
```

---

### Providers Crate (`clawcrew-providers`)

**Owns:**
- LLM vendor APIs (OpenAI, Gemini, Bedrock, Anthropic) — **SSOT (ADR 0001)**
- Provider routing + fallback
- Token streaming
- Circuit breaker + retry logic

**Exports:**
```rust
pub trait Provider {
    async fn stream_chat(
        &self,
        messages: Vec<Message>,
        tools: Vec<Tool>,
    ) -> Result<TokenStream>;
}

pub struct ProviderDispatch {
    openai: OpenAIProvider,
    gemini: GeminiProvider,
    bedrock: BedrockProvider,
    anthropic: AnthropicProvider,
}

impl ProviderDispatch {
    pub fn dispatch(&self, model: &str) -> Result<&dyn Provider>;
}
```

**Dependencies:**
```toml
[dependencies]
reqwest = { version = "0.12", features = ["json", "stream"] }
tokio.workspace = true
serde.workspace = true
# NO dependency on clawcrew-runtime (providers = leaf crate)
```

**Convention:** Providers crate is **leaf** (no internal crate deps), runtime calls it.

---

### Channel Crates (`clawcrew-channel-*`)

**Pattern:** One crate per channel (Discord, Telegram, Slack, etc.)

**Example: `clawcrew-channel-discord`**
```rust
pub struct DiscordChannel {
    client: Arc<DiscordClient>,
}

impl Channel for DiscordChannel {
    async fn send_message(&self, msg: &Message) -> Result<()>;
    async fn receive_messages(&self) -> Result<MessageStream>;
}
```

**Naming:** `clawcrew-channel-{service}` (e.g., `clawcrew-channel-telegram`, `clawcrew-channel-slack`)

**New channels:** Use `galleon-channel-*` prefix (ADR: brand consistency).

---

### Tool Crates (`clawcrew-tools-*`)

**Pattern:** One crate per tool domain (filesystem, shell, network, etc.)

**Example: `clawcrew-tools-shell`**
```rust
pub struct ShellTool;

impl ToolExecutor for ShellTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        let command = args["command"].as_str().ok_or("Missing command")?;
        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .output()
            .await?;
        
        Ok(json!({
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
            "exit_code": output.status.code(),
        }))
    }
}
```

**Naming:** `clawcrew-tools-{domain}` (e.g., `clawcrew-tools-filesystem`, `clawcrew-tools-network`)

---

## Trait Boundaries

### Why Traits?

- **Extensibility:** New provider/channel/tool = implement trait, no core change
- **Testability:** Mock implementations for unit tests
- **Plugin architecture:** WASI components implement same traits

### Example: Provider Trait

```rust
// clawcrew-providers/src/lib.rs
#[async_trait]
pub trait Provider: Send + Sync {
    async fn stream_chat(
        &self,
        messages: Vec<Message>,
        tools: Vec<Tool>,
    ) -> Result<TokenStream>;
    
    fn name(&self) -> &str;
    fn supports_tools(&self) -> bool { true }
}

// clawcrew-providers/src/openai.rs
pub struct OpenAIProvider {
    api_key: String,
    client: reqwest::Client,
}

#[async_trait]
impl Provider for OpenAIProvider {
    async fn stream_chat(
        &self,
        messages: Vec<Message>,
        tools: Vec<Tool>,
    ) -> Result<TokenStream> {
        let request = self.build_request(messages, tools)?;
        let response = self.client.post("https://api.openai.com/v1/chat/completions")
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .await?;
        
        Ok(self.parse_stream(response).await?)
    }
    
    fn name(&self) -> &str { "openai" }
}
```

**Conventions:**
- Traits use `#[async_trait]` for async methods
- Traits are `Send + Sync` (thread-safe)
- Default implementations for optional methods

---

## gRPC Server Patterns

### Proto Definition

```protobuf
// proto/system_gateway.proto
service SystemGatewayService {
    rpc ExecuteTurn(ExecuteTurnRequest) returns (stream TurnToken);
    rpc ExecuteTool(ExecuteToolRequest) returns (ExecuteToolResponse);
    rpc GetSecret(GetSecretRequest) returns (GetSecretResponse);
}
```

### Server Implementation

```rust
// clawcrew-gateway/src/grpc_server.rs
use tonic::{Request, Response, Status};
use proto::system_gateway_service_server::SystemGatewayService;

pub struct SystemGatewayServer {
    runtime: Arc<Runtime>,
    vault: Arc<Vault>,
}

#[tonic::async_trait]
impl SystemGatewayService for SystemGatewayServer {
    type ExecuteTurnStream = ReceiverStream<Result<TurnToken, Status>>;
    
    async fn execute_turn(
        &self,
        request: Request<ExecuteTurnRequest>,
    ) -> Result<Response<Self::ExecuteTurnStream>, Status> {
        let req = request.into_inner();
        
        // Delegate to runtime
        let stream = self.runtime
            .execute_turn(&req.model, req.messages, req.tools)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        
        // Convert to gRPC stream
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        tokio::spawn(async move {
            while let Some(token) = stream.next().await {
                let _ = tx.send(Ok(token)).await;
            }
        });
        
        Ok(Response::new(ReceiverStream::new(rx)))
    }
    
    async fn get_secret(
        &self,
        request: Request<GetSecretRequest>,
    ) -> Result<Response<GetSecretResponse>, Status> {
        let key = request.into_inner().key;
        
        let value = self.vault
            .get_secret(&key)
            .await
            .map_err(|e| Status::not_found(e.to_string()))?;
        
        Ok(Response::new(GetSecretResponse { value }))
    }
}
```

**Conventions:**
- Streaming RPCs return `type FooStream = ReceiverStream<Result<T, Status>>`
- Convert domain errors to `tonic::Status` with appropriate codes (`internal`, `not_found`, `invalid_argument`)
- Spawn tokio task for async stream forwarding

---

## Error Handling

### Error Types

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RuntimeError {
    #[error("Provider not found: {0}")]
    ProviderNotFound(String),
    
    #[error("Tool execution failed: {0}")]
    ToolFailed(#[from] ToolError),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
}

pub type Result<T> = std::result::Result<T, RuntimeError>;
```

**Conventions:**
- Use `thiserror` for error enums (derive `Error`, not hand-written `Display`)
- `#[from]` for auto-conversion from upstream errors
- `Result<T>` type alias at crate root
- Error messages are sentence case ("Provider not found", not "provider not found")

### Propagate Errors (Don't Panic)

```rust
// ✅ Correct: propagate error
pub async fn execute_turn(model: &str, messages: Vec<Message>) -> Result<TurnResponse> {
    let provider = get_provider(model)?; // ? propagates error
    let response = provider.stream_chat(messages).await?;
    Ok(response)
}

// ❌ Wrong: panic on error
pub async fn execute_turn(model: &str, messages: Vec<Message>) -> TurnResponse {
    let provider = get_provider(model).unwrap(); // Panics if provider not found
    provider.stream_chat(messages).await.unwrap() // Panics if HTTP fails
}
```

**Convention:** `unwrap()` / `expect()` only in:
- Tests
- `main()` setup (crash early if config invalid)
- Documented invariants (add comment why panic is safe)

---

## Async Patterns

### Use Tokio Runtime

```rust
#[tokio::main]
async fn main() -> Result<()> {
    let gateway = Gateway::new().await?;
    gateway.run().await?;
    Ok(())
}
```

**Convention:** Always use `#[tokio::main]` in binaries, `#[tokio::test]` in tests.

### Spawn Background Tasks

```rust
// ✅ Correct: spawn task, handle errors
tokio::spawn(async move {
    if let Err(e) = background_work().await {
        tracing::error!("Background task failed: {}", e);
    }
});

// ❌ Wrong: spawn without error handling
tokio::spawn(async move {
    background_work().await.unwrap(); // Panics in background thread
});
```

**Convention:** Always log or handle errors in spawned tasks (panics are silent).

### Select Multiple Futures

```rust
use tokio::select;

async fn process_with_timeout(work: impl Future<Output = Result<()>>) -> Result<()> {
    select! {
        result = work => result,
        _ = tokio::time::sleep(Duration::from_secs(30)) => {
            Err(RuntimeError::Timeout)
        }
    }
}
```

**Convention:** Use `select!` for timeouts, graceful shutdown, multi-future racing.

---

## Logging (tracing)

### Instrument Functions

```rust
use tracing::{info, error, instrument};

#[instrument(skip(self))] // Skip large/non-Debug params
pub async fn execute_turn(&self, model: &str, messages: Vec<Message>) -> Result<TurnResponse> {
    info!("Starting turn execution");
    
    let provider = self.get_provider(model)?;
    let response = provider.stream_chat(messages).await?;
    
    info!(tokens = response.tokens, "Turn completed");
    Ok(response)
}
```

**Conventions:**
- `#[instrument]` on public async functions (auto-adds span with args)
- `skip(self)` to avoid logging `self` (too verbose)
- `info!` for normal flow, `error!` for errors, `debug!` for verbose detail
- Log structured fields: `info!(field = value, "message")`

### Initialize Tracing

```rust
// main.rs
use tracing_subscriber::{fmt, EnvFilter};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env()) // RUST_LOG=debug
        .json() // JSON output (structured logs)
        .init();
    
    // ...
}
```

---

## Testing

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_execute_turn_success() {
        let provider = MockProvider::new();
        let runtime = Runtime::new(provider);
        
        let response = runtime.execute_turn("gpt-4", vec![]).await.unwrap();
        assert!(!response.tokens.is_empty());
    }
    
    #[tokio::test]
    async fn test_execute_turn_provider_not_found() {
        let runtime = Runtime::new(MockProvider::new());
        
        let result = runtime.execute_turn("invalid-model", vec![]).await;
        assert!(matches!(result, Err(RuntimeError::ProviderNotFound(_))));
    }
}
```

**Conventions:**
- `#[cfg(test)]` module in same file
- `#[tokio::test]` for async tests
- Mock external dependencies (HTTP, filesystem, gRPC)
- Test both success and error paths

### Integration Tests

```rust
// tests/integration_test.rs
use clawcrew_runtime::execute_turn;

#[tokio::test]
async fn test_turn_execution_e2e() {
    // Real provider (requires API key in env)
    let response = execute_turn("gpt-4", vec![], vec![]).await.unwrap();
    assert!(!response.content.is_empty());
}
```

**Conventions:**
- `tests/*.rs` for integration tests (separate from `src/`)
- Use real dependencies (HTTP, filesystem), not mocks
- Gate expensive tests with `#[ignore]` (run with `cargo test -- --ignored`)

---

## Naming Conventions

| Type | Pattern | Example |
|------|---------|---------|
| Crate | `snake_case` | `clawcrew_gateway`, `clawcrew_providers` |
| Module | `snake_case` | `mod grpc_server;`, `mod vault;` |
| Struct | `PascalCase` | `Gateway`, `OpenAIProvider` |
| Trait | `PascalCase` | `Provider`, `ToolExecutor` |
| Function | `snake_case` | `execute_turn`, `get_secret` |
| Constant | `UPPER_SNAKE_CASE` | `MAX_RETRIES`, `DEFAULT_TIMEOUT` |
| Type alias | `PascalCase` | `Result<T>`, `TokenStream` |

---

## Code Organization

### Crate Layout

```
clawcrew-gateway/
├── src/
│   ├── lib.rs            # Public API exports
│   ├── grpc_server.rs    # gRPC server implementation
│   ├── vault.rs          # Vault service
│   ├── ws.rs             # WebSocket server
│   └── mdns.rs           # mDNS discovery
├── tests/
│   └── integration.rs    # Integration tests
├── Cargo.toml
└── README.md
```

**Convention:** `lib.rs` exports public API, internal modules are private by default.

### Re-exports

```rust
// lib.rs
mod grpc_server;
mod vault;

pub use grpc_server::SystemGatewayServer;
pub use vault::{Vault, VaultService};
```

**Convention:** Re-export public types at crate root (users import from crate, not internal modules).

---

## Performance

### Use Arc for Shared State

```rust
pub struct Gateway {
    vault: Arc<Vault>,      // Shared across tasks
    runtime: Arc<Runtime>,  // Shared across tasks
}

impl Gateway {
    pub fn new(vault: Vault, runtime: Runtime) -> Self {
        Self {
            vault: Arc::new(vault),
            runtime: Arc::new(runtime),
        }
    }
}
```

**Convention:** Wrap shared state in `Arc` (cheap clone, thread-safe reference counting).

### Avoid Cloning Large Data

```rust
// ✅ Correct: pass by reference
pub async fn process_messages(messages: &[Message]) -> Result<()> {
    for msg in messages {
        // ...
    }
    Ok(())
}

// ❌ Wrong: clone entire vec
pub async fn process_messages(messages: Vec<Message>) -> Result<()> {
    // Clones vec on every call
    Ok(())
}
```

**Convention:** Pass `&T` or `&[T]` for read-only access, `T` only when taking ownership.

---

## Security

### Scrub Secrets from Logs

```rust
use tracing::info;

pub async fn call_llm(api_key: &str, prompt: &str) -> Result<String> {
    // ❌ Wrong: logs full API key
    info!("Calling LLM with key: {}", api_key);
    
    // ✅ Correct: redact secret
    info!("Calling LLM with key: {}***", &api_key[..8]);
    
    // ...
}
```

**Convention:** Never log full secrets (API keys, tokens, passwords). Redact or skip.

### Validate Input at Boundaries

```rust
pub async fn execute_turn(model: &str, messages: Vec<Message>) -> Result<TurnResponse> {
    // Validate model name
    if model.is_empty() {
        return Err(RuntimeError::InvalidInput("Model cannot be empty".into()));
    }
    
    // Validate messages
    if messages.is_empty() {
        return Err(RuntimeError::InvalidInput("Messages cannot be empty".into()));
    }
    
    // ...
}
```

**Convention:** Validate inputs at public API boundaries (reject invalid early).

---

## Related Documents

- [Architecture Overview](../architecture/00-overview.md) — 4-tier system design
- [Boundary Policy](../architecture/01-boundary-policy.md) — What Rust owns vs delegates
- [ADR 0001: LLM SSOT Rust](../decisions/0001-llm-ssot-rust.md) — Rust executes LLM calls
- [Go Conventions](./go-conventions.md) — Go patterns (Wire DI, HTTP)
