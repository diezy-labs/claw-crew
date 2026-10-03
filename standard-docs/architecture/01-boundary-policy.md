# Tier Boundary Policy

**Status:** Normative (code MUST follow this)  
**Last updated:** 2026-10-03  
**Enforcement:** PR review gate + architecture audit

---

## Purpose

4-tier architecture only works if **boundaries are clear and enforced**. This document defines what each tier OWNS, what is FORBIDDEN, and how cross-tier communication happens.

Violating boundaries creates:
- Duplicate SSOT (same logic in 2+ tiers)
- Leaky abstractions (frontend knows backend internals)
- Security holes (UI bypasses Go gateway to call Rust directly)
- Maintenance cost (change requires editing 3 tiers)

**Golden rule:** Each fact has ONE authoritative source. Derive, don't duplicate.

---

## Tier 1: Desktop Shell (Tauri)

### Owns
- OS integration: window lifecycle, system tray, native dialogs, notifications, file picker
- Desktop packaging: `.exe`, `.dmg`, `.AppImage` bundling
- IPC command registration: `#[tauri::command]` functions

### Forbidden
- **Business logic** (crew policies, risk-tier calculations, approval rules)
- **API logic** (HTTP handlers, gRPC services)
- **Persistence** (SQLite writes, file writes outside Tauri-managed dirs)
- **Crew state** (active turns, memory, artifacts)
- **Secret management** (API keys, tokens — use Rust vault via IPC)

### Communication
- **Outbound:** Tauri IPC `invoke` → Go HTTP :9090 (NOT gRPC :50051, which is internal)
- **Inbound:** Tauri `emit` events from Rust backend (window lifecycle only)

### Examples

✅ **Correct:**
```rust
#[tauri::command]
async fn get_system_metrics() -> Result<SystemMetrics, String> {
    let response = engine_get("system/metrics").await?; // HTTP → Go :9090
    Ok(response.json().await?)
}
```

❌ **Wrong (fake data):**
```rust
#[tauri::command]
fn get_system_metrics() -> SystemMetrics {
    SystemMetrics {
        gateway_latency_ms: 8, // Hardcoded fake data (ADR 0003 violation)
        memory_db_mb: 18.4,
    }
}
```

❌ **Wrong (business logic in Tauri):**
```rust
#[tauri::command]
fn should_auto_approve(risk: &str) -> bool {
    risk == "low" || risk == "minor" // Policy logic belongs to Go engine
}
```

---

## Tier 2: System Core (Rust)

### Owns
- **Security microkernel:** Sandbox, secrets vault, credential scrubbing
- **Native tools:** Filesystem, shell, network, OS telemetry
- **LLM execution:** ADR 0001 SSOT — all vendor API calls (OpenAI, Gemini, Bedrock, Anthropic)
- **Channel plugins:** Discord, Telegram, Slack, WhatsApp websockets
- **WASI plugin runtime:** Load, sandbox, execute `.wasm` components
- **mDNS A2A discovery:** Peer fleet discovery on local network

### Forbidden
- **Fleet governance** (crew policies, approval workflows — belongs to Go)
- **Crew orchestration** (turn lifecycle, task assignment — belongs to Go)
- **RAG memory** (vector embeddings, ChromaDB — belongs to Go)
- **Business rules** (risk-tier thresholds, auto-approval criteria — belongs to Go)

### Communication
- **Inbound:** gRPC server `SystemGatewayService` :50052 (called by Go engine)
- **Outbound:** None (Rust is leaf service, doesn't call Go)

### Examples

✅ **Correct:**
```rust
// Rust executes LLM call, Go orchestrates turn
impl SystemGatewayService for Gateway {
    async fn execute_turn(&self, req: ExecuteTurnRequest) -> Result<TurnResponse> {
        let provider = self.providers.dispatch(&req.model)?;
        let stream = provider.stream_chat(req.messages, req.tools).await?;
        Ok(TurnResponse { stream })
    }
}
```

❌ **Wrong (Rust does crew orchestration):**
```rust
// Crew turn lifecycle belongs to Go, not Rust
async fn start_crew_turn(crew_id: &str, task: &str) -> Result<CrewTurn> {
    let crew = load_crew(crew_id)?; // Fleet config read — Go's job
    let policy = get_risk_tier(&crew)?; // Policy logic — Go's job
    // ...
}
```

---

## Tier 3: AI Orchestrator (Go)

### Owns
- **Agent brain:** Crew, squad, task orchestration
- **Fleet policies:** Risk-tier, approval workflows, governance rules
- **Vector memory + RAG:** Embeddings, ChromaDB/Qdrant, memory search
- **Workflow engine:** Goal loops, multi-phase execution
- **Persistence:** SQLite/Postgres writes (fleet config, chat history, artifacts)

### Forbidden
- **LLM API calling** (delegated to Rust ADR 0001 — use gRPC `SystemGatewayService.ExecuteTurn`)
- **Tool execution** (delegated to Rust — use gRPC `SystemGatewayService.ExecuteTool`)
- **Channel websockets** (delegated to Rust — Go HTTP only)
- **OS-level operations** (file reads outside `engine/data/`, shell exec without Rust tool)

### Communication
- **Outbound:** gRPC client → Rust :50052 (tool + LLM execution)
- **Inbound (internal):** gRPC server `AgentEngineService` :50051 (NOT exposed to frontend, placeholder)
- **Inbound (external):** HTTP server :9090 (frontend-facing REST API)

### Examples

✅ **Correct:**
```go
// Go orchestrates, Rust executes
func (s *CrewService) StartTurn(ctx context.Context, req *StartTurnRequest) (*TurnResponse, error) {
    // 1. Go builds turn context (memory, tools, policies)
    messages := s.memory.GetHistory(req.CrewID)
    tools := s.fleet.GetCrewTools(req.CrewID)
    
    // 2. Delegate LLM execution to Rust
    stream, err := s.gateway.ExecuteTurn(ctx, &pb.ExecuteTurnRequest{
        Model:    req.Model,
        Messages: messages,
        Tools:    tools,
    })
    if err != nil {
        return nil, err
    }
    
    // 3. Go saves result to memory
    result := s.consumeStream(stream)
    s.memory.Save(req.CrewID, result)
    
    return &TurnResponse{Result: result}, nil
}
```

❌ **Wrong (Go calls OpenAI directly):**
```go
// LLM calling = Rust SSOT (ADR 0001), Go should delegate
func (s *CrewService) StartTurn(ctx context.Context, req *StartTurnRequest) (*TurnResponse, error) {
    client := openai.NewClient(s.config.APIKey) // Direct vendor call — ADR 0001 violation
    resp, err := client.CreateChatCompletion(ctx, openai.ChatCompletionRequest{
        Model:    req.Model,
        Messages: req.Messages,
    })
    // ...
}
```

---

## Tier 4: UI Layer (web-2)

### Owns
- **React components:** View rendering, Tailwind styling
- **Client-side routing:** React Router
- **HTTP client:** `apiClient.ts` calls to Go :9090
- **View models:** Transform API responses for display

### Forbidden
- **Backend logic** (HTTP handlers, business rules, persistence)
- **QR code generation** (ADR 0002 — should be Go HTTP endpoint)
- **OS telemetry** (ADR 0002 — should be Go HTTP endpoint)
- **Secret management** (API keys, tokens — never in frontend)

### Communication
- **Outbound:** HTTP GET/POST → Go :9090 or Tauri IPC
- **Inbound:** None (UI is leaf tier)

### Examples

✅ **Correct:**
```typescript
// UI calls Go HTTP API
export async function getFleetMetrics(): Promise<FleetMetrics> {
    const response = await fetch('http://localhost:9090/api/system/metrics');
    return response.json();
}
```

❌ **Wrong (backend logic in Express server):**
```typescript
// web-2/server.ts — ADR 0002 violation (temporary)
app.get('/api/network/qrcode', (req, res) => {
    const qr = qrcode.generate(req.query.data); // Business logic in UI tier
    res.send(qr);
});
```

❌ **Wrong (UI calls Rust gRPC directly):**
```typescript
// Rust gRPC :50052 is internal-only, UI MUST route through Go :9090
const client = new SystemGatewayClient('localhost:50052');
const response = await client.executeTurn(request); // Security bypass
```

---

## Cross-Tier Data Flow Rules

### Rule 1: Frontend → Go → Rust (Never Frontend → Rust)

**Why?** Go engine = gateway with policy enforcement (risk-tier, approval, rate limiting). Bypassing Go = bypassing governance.

```
UI → Go HTTP :9090 → Go gRPC client → Rust gRPC :50052
```

**Forbidden:** `UI → Rust :50052` (no credential, no policy check)

### Rule 2: Each Tier Owns Its Persistence

**No shared database.** Cross-tier data access = API call, not direct DB read.

| Tier | Storage | Access |
|------|---------|--------|
| Tauri | `~/.config/galleon/window.json` | Tauri `fs` API (sandboxed) |
| Rust | `~/.config/galleon/vault/` | Rust `VaultService` only |
| Go | `engine/data/*.db` | Go `store` packages only |

**Forbidden:** Go reads Rust vault directly, Tauri writes Go SQLite directly.

### Rule 3: Secrets = Rust Vault Only

API keys, tokens, credentials stored in Rust `~/.config/galleon/vault/` (AES-256-GCM encrypted).

**Access path:**
```
Go needs API key
  → gRPC call `SystemGatewayService.GetSecret`
  → Rust reads vault, returns decrypted key
  → Go uses key, never persists it
```

**Forbidden:** Go stores API keys in `engine/data/config.json`, Tauri stores tokens in `localStorage`.

### Rule 4: Policies = Go SSOT

Risk-tier thresholds, approval workflows, auto-approval criteria live in Go `fleet/policies.go`.

**Access path:**
```
Tauri needs approval decision
  → HTTP GET /api/fleet/policies
  → Go returns policy
  → Tauri displays approval UI
```

**Forbidden:** Tauri hardcodes `if risk == "low" then auto_approve` (policy logic duplicated).

---

## Enforcement

### Code Review Checklist

- [ ] New Tauri command delegates to Go HTTP (no business logic)
- [ ] New Go HTTP handler delegates LLM/tool execution to Rust gRPC (no direct vendor call)
- [ ] New Rust gRPC handler does NOT read Go SQLite (no shared DB)
- [ ] New UI component calls Go HTTP :9090 (not Rust :50052)
- [ ] Secrets accessed via Rust vault API (not filesystem read)
- [ ] Policies queried from Go endpoint (not hardcoded in UI/Tauri)

### Architecture Audit

Run quarterly audit (grep + manual check):
```bash
# Detect boundary violations
grep -r "openai.NewClient" engine/src/crew/    # Go direct LLM call (should delegate to Rust)
grep -r "qrcode.generate" web-2/server.ts      # Backend logic in UI (ADR 0002)
grep -r "SystemGatewayClient" web-2/src/       # UI calling Rust directly (forbidden)
grep -r "sqlite3.Open" crates/                 # Rust reading Go DB (forbidden)
```

### Exceptions (Document When Breaking Rules)

If a boundary violation is intentional (temporary workaround, performance optimization, legacy code), document it:

1. Add `// TODO(boundary): <reason>` comment in code
2. Create ADR if pattern will persist (e.g., ADR 0002 web-2 temp violations)
3. Add exception to this document under "Known Violations" section

---

## Known Violations (Temporary)

| File | Violation | ADR | Plan |
|------|-----------|-----|------|
| `web-2/server.ts` lines 53-95 | QR generation + OS telemetry in UI tier | ADR 0002 | Move to Go HTTP + Nginx (next phase) |
| `apps/tauri-2/src/main.rs` line 111 | Fake metrics hardcoded | ADR 0003 | Read from Go `/api/system/metrics` |
| `engine/src/llm/provider.go` | Go calls OpenAI directly | ADR 0001 | Delegate to Rust gRPC (Phase 1 implementation) |

---

## Related Documents

- [Architecture Overview](./00-overview.md) — 4-tier factual description
- [ADR 0001: LLM SSOT Rust](../decisions/0001-llm-ssot-rust.md) — Go delegates LLM to Rust
- [ADR 0002: web-2 Backend Policy](../decisions/0002-web2-backend-policy.md) — Zero logic in UI tier
- [ADR 0003: Tauri Metrics Real API](../decisions/0003-tauri-metrics-real-api.md) — No fake data
