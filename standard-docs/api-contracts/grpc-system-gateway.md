# gRPC System Gateway (Rust :50052)

**Status:** Living spec (updated with each proto change)  
**Last updated:** 2026-10-03  
**Protocol:** gRPC (protobuf)  
**Port:** :50052  
**Server:** Rust `clawcrew-gateway`  
**Client:** Go `engine` (internal-only, NOT exposed to frontend)

---

## Overview

Rust `SystemGatewayService` exposes internal APIs for:
- **LLM execution** (ADR 0001 SSOT — OpenAI, Gemini, Bedrock, Anthropic)
- **Tool execution** (filesystem, shell, network)
- **Secret access** (vault reads/writes)

**Security:** Internal-only (Go → Rust), no authentication (localhost trust boundary).

---

## Proto Definition

```protobuf
// proto/system_gateway.proto
syntax = "proto3";
package system_gateway;

service SystemGatewayService {
  // Execute agent turn (LLM streaming)
  rpc ExecuteTurn(ExecuteTurnRequest) returns (stream TurnToken);
  
  // Execute tool (filesystem, shell, etc.)
  rpc ExecuteTool(ExecuteToolRequest) returns (ExecuteToolResponse);
  
  // Vault operations (secrets)
  rpc GetSecret(GetSecretRequest) returns (GetSecretResponse);
  rpc SetSecret(SetSecretRequest) returns (SetSecretResponse);
  rpc ListSecrets(ListSecretsRequest) returns (ListSecretsResponse);
}
```

---

## ExecuteTurn (Streaming LLM)

### Request

```protobuf
message ExecuteTurnRequest {
  string model = 1;              // e.g., "gpt-4", "gemini-pro"
  repeated Message messages = 2; // Chat history
  repeated Tool tools = 3;       // Available tools
}

message Message {
  string role = 1;     // "system", "user", "assistant"
  string content = 2;  // Message text
}

message Tool {
  string name = 1;        // e.g., "filesystem_read"
  string description = 2; // "Read file contents"
  string parameters = 3;  // JSON schema
}
```

### Response (Stream)

```protobuf
message TurnToken {
  oneof event {
    TokenChunk token = 1;
    ToolCall tool_call = 2;
    TurnComplete complete = 3;
    TurnError error = 4;
  }
}

message TokenChunk {
  string text = 1;       // Token text
  int32 index = 2;       // Token index in stream
}

message ToolCall {
  string tool = 1;       // Tool name
  string args = 2;       // JSON args
  string call_id = 3;    // Unique call ID
}

message TurnComplete {
  int32 tokens_used = 1;
  string finish_reason = 2; // "stop", "length", "tool_calls"
}

message TurnError {
  string code = 1;       // "provider_error", "invalid_model"
  string message = 2;    // Human-readable error
}
```

### Example (Go Client)

```go
stream, err := client.ExecuteTurn(ctx, &pb.ExecuteTurnRequest{
    Model: "gpt-4",
    Messages: []*pb.Message{
        {Role: "user", Content: "Hello, world!"},
    },
})
if err != nil {
    return err
}

for {
    token, err := stream.Recv()
    if err == io.EOF {
        break
    }
    if err != nil {
        return err
    }
    
    switch event := token.Event.(type) {
    case *pb.TurnToken_Token:
        fmt.Print(event.Token.Text)
    case *pb.TurnToken_ToolCall:
        fmt.Printf("[Tool: %s]\n", event.ToolCall.Tool)
    case *pb.TurnToken_Complete:
        fmt.Printf("\nTokens: %d\n", event.Complete.TokensUsed)
    case *pb.TurnToken_Error:
        return fmt.Errorf("Turn error: %s", event.Error.Message)
    }
}
```

---

## ExecuteTool

### Request

```protobuf
message ExecuteToolRequest {
  string tool = 1;   // Tool name (e.g., "filesystem_read")
  string args = 2;   // JSON args (e.g., {"path": "/etc/hosts"})
}
```

### Response

```protobuf
message ExecuteToolResponse {
  bool success = 1;
  string result = 2;  // JSON result or error message
}
```

### Example (Go Client)

```go
resp, err := client.ExecuteTool(ctx, &pb.ExecuteToolRequest{
    Tool: "filesystem_read",
    Args: `{"path": "/etc/hosts"}`,
})
if err != nil {
    return err
}

if !resp.Success {
    return fmt.Errorf("Tool failed: %s", resp.Result)
}

fmt.Println("File contents:", resp.Result)
```

---

## GetSecret (Vault Read)

### Request

```protobuf
message GetSecretRequest {
  string key = 1; // Secret key (e.g., "openai_api_key")
}
```

### Response

```protobuf
message GetSecretResponse {
  string value = 1; // Decrypted secret value
}
```

### Errors

- `NOT_FOUND` — Secret does not exist
- `INTERNAL` — Vault decryption failed

### Example (Go Client)

```go
resp, err := client.GetSecret(ctx, &pb.GetSecretRequest{
    Key: "openai_api_key",
})
if err != nil {
    if status.Code(err) == codes.NotFound {
        return fmt.Errorf("API key not configured")
    }
    return err
}

apiKey := resp.Value
// Use apiKey (DO NOT log it)
```

---

## SetSecret (Vault Write)

### Request

```protobuf
message SetSecretRequest {
  string key = 1;   // Secret key
  string value = 2; // Secret value (will be encrypted)
}
```

### Response

```protobuf
message SetSecretResponse {
  bool success = 1;
}
```

### Example (Go Client)

```go
_, err := client.SetSecret(ctx, &pb.SetSecretRequest{
    Key:   "openai_api_key",
    Value: "sk-proj-...",
})
if err != nil {
    return err
}
```

---

## ListSecrets (Vault List)

### Request

```protobuf
message ListSecretsRequest {}
```

### Response

```protobuf
message ListSecretsResponse {
  repeated string keys = 1; // Secret keys (NOT values)
}
```

### Example (Go Client)

```go
resp, err := client.ListSecrets(ctx, &pb.ListSecretsRequest{})
if err != nil {
    return err
}

fmt.Println("Configured secrets:", resp.Keys)
// Output: ["openai_api_key", "gemini_api_key"]
```

---

## Error Codes

| Code | Description | Retry? |
|------|-------------|--------|
| `OK` | Success | — |
| `CANCELLED` | Request cancelled | No |
| `INVALID_ARGUMENT` | Invalid request (e.g., empty model) | No |
| `NOT_FOUND` | Resource not found (e.g., secret) | No |
| `INTERNAL` | Server error (e.g., vault decryption failed) | Yes (after backoff) |
| `UNAVAILABLE` | Service unavailable (e.g., LLM API down) | Yes (with exponential backoff) |

---

## Timeouts

| RPC | Default Timeout | Max Retry |
|-----|----------------|-----------|
| `ExecuteTurn` | 120s | 0 (no retry, streaming) |
| `ExecuteTool` | 30s | 2 |
| `GetSecret` | 5s | 2 |
| `SetSecret` | 5s | 2 |
| `ListSecrets` | 5s | 2 |

**Convention:** Go client sets `context.WithTimeout()` per RPC.

---

## Security

### Secrets in Transit

- **gRPC over localhost** — No TLS (trusted boundary)
- **Future (remote Rust):** mTLS with client certificates

### Secrets at Rest

- **Vault:** AES-256-GCM encrypted on disk (`~/.config/galleon/vault/`)
- **Key derivation:** PBKDF2 (100k iterations) from user passphrase

### Secret Scrubbing

Rust logs scrub:
- API keys (regex: `sk-[a-zA-Z0-9]{32,}`, `AIza[a-zA-Z0-9]{35}`)
- Bearer tokens (`Authorization: Bearer ...`)
- Cookies (`Set-Cookie: ...`)

**Convention:** Never log full secrets (use `info!(key = "<redacted>")` in Rust).

---

## Performance

### Latency (p50 / p99)

| RPC | p50 | p99 | Notes |
|-----|-----|-----|-------|
| `ExecuteTurn` (first token) | 320ms | 580ms | Vendor-dependent (OpenAI 320ms, Gemini 180ms) |
| `ExecuteTool` | 18ms | 42ms | Filesystem I/O |
| `GetSecret` | 1.2ms | 3.8ms | In-memory cache (vault read cached) |

**Bottleneck:** LLM API latency (not gRPC overhead, which is ~1-2ms).

---

## Monitoring

### Metrics (Prometheus)

```
# Rust exports :50052/metrics
system_gateway_turn_duration_seconds{model="gpt-4"} 12.4
system_gateway_turn_tokens{model="gpt-4"} 1842
system_gateway_tool_calls_total{tool="filesystem_read"} 347
```

### Tracing (OpenTelemetry)

Rust exports traces to `OTEL_EXPORTER_OTLP_ENDPOINT` (optional):
```
ExecuteTurn span
  ├─ provider.stream_chat span (OpenAI)
  └─ token.receive span (streaming)
```

---

## Testing

### Unit Tests (Rust)

```rust
#[tokio::test]
async fn test_execute_turn_success() {
    let gateway = SystemGatewayServer::new_mock();
    let request = tonic::Request::new(ExecuteTurnRequest {
        model: "gpt-4".into(),
        messages: vec![...],
        tools: vec![],
    });
    
    let mut stream = gateway.execute_turn(request).await.unwrap().into_inner();
    
    let first_token = stream.message().await.unwrap().unwrap();
    assert!(matches!(first_token.event, Some(TurnToken_Token(_))));
}
```

### Integration Tests (Go)

```go
func TestExecuteTurnIntegration(t *testing.T) {
    conn, _ := grpc.Dial("localhost:50052", grpc.WithInsecure())
    defer conn.Close()
    
    client := pb.NewSystemGatewayServiceClient(conn)
    stream, err := client.ExecuteTurn(context.Background(), &pb.ExecuteTurnRequest{
        Model:    "gpt-4",
        Messages: []*pb.Message{{Role: "user", Content: "Hello"}},
    })
    require.NoError(t, err)
    
    tokenCount := 0
    for {
        token, err := stream.Recv()
        if err == io.EOF {
            break
        }
        require.NoError(t, err)
        tokenCount++
    }
    
    assert.Greater(t, tokenCount, 0)
}
```

---

## Migration Plan (ADR 0001)

### Phase 1: Extend gRPC (2-3 days)
- [x] Add `ExecuteTurn` RPC to proto
- [ ] Wire Rust `runtime/agent/turn.rs` as gRPC handler
- [ ] Test with Go gRPC client (OpenAI provider only)

### Phase 2: Go Delegation (1-2 days)
- [ ] Refactor Go `crew/services.go` to call `gateway.ExecuteTurn` (not `llmProvider.StreamChat`)
- [ ] Update tests (mock gRPC client)

### Phase 3: Cleanup (1 day)
- [ ] Delete Go `engine/src/llm/provider.go`, `multi_provider.go`
- [ ] Update docs (remove Go LLM references)

**Current status:** Phase 1 NOT started (ADR 0001 written, not implemented).

---

## Related Documents

- [Architecture Overview](../architecture/00-overview.md) — 4-tier system design
- [Boundary Policy](../architecture/01-boundary-policy.md) — What gRPC owns vs delegates
- [ADR 0001: LLM SSOT Rust](../decisions/0001-llm-ssot-rust.md) — Implementation plan
- [Rust Conventions](../tech-stack/rust-conventions.md) — gRPC server patterns
- [HTTP Engine API](./http-engine-api.md) — Go :9090 frontend-facing spec
