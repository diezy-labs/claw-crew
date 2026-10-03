# Architecture Overview

**Status:** Living document  
**Last updated:** 2026-10-03  
**Replaces:** `.gemini/architecture.md` (merged here for single SSOT)

---

## System Identity

**Galleon** is a 4-tier AI agent platform with strict boundary enforcement:

| Tier | Runtime | Port/Interface | Responsibility |
|------|---------|----------------|----------------|
| **Desktop Shell** | Tauri v2 (Rust) | IPC | OS integration (window, tray, dialog, packaging) |
| **System Core** | Rust 2024 (25 crates) | gRPC :50052 | Security microkernel, sandbox, tools, LLM execution, channels |
| **AI Orchestrator** | Go 1.25+ | gRPC :50051 + HTTP :9090 | Agent brain, fleet governance, RAG, persistensi |
| **UI Layer** | React 19 + TS | HTTP reverse-proxy | Rendering, view-model, **zero backend logic** |

---

## Tier Boundaries (STRICT)

### 1. Desktop Shell (Tauri)
- **Owned:** Window lifecycle, system tray, OS dialogs, native notifications, `.exe` packaging
- **Forbidden:** Business logic, API logic, persistence, crew state
- **Protocol:** Tauri IPC (`invoke`) → Go HTTP :9090 (NOT gRPC :50051, which is internal Go↔Rust only)

### 2. System Core (Rust)
- **Owned:** Security microkernel (sandbox, secrets, vault), native tools, LLM provider execution (ADR 0001 SSOT), channel plugins (Discord, Telegram, Slack), WASI plugin runtime, mDNS A2A discovery
- **Forbidden:** Fleet governance, crew orchestration, RAG memory (belongs to Go engine)
- **Protocol:** gRPC server `SystemGatewayService` :50052 (called by Go engine)

### 3. AI Orchestrator (Go)
- **Owned:** Agent brain (crew, squad, task), fleet policies (risk-tier, approval), vector memory + RAG, workflow engine, persistence (SQLite/Postgres)
- **Forbidden:** LLM API calling (delegated to Rust ADR 0001), tool execution (delegated to Rust), channel websockets (delegated to Rust)
- **Protocol:** 
  - gRPC client → Rust :50052 (tool + LLM execution)
  - gRPC server `AgentEngineService` :50051 (internal control plane, NOT exposed to frontend)
  - HTTP server :9090 (frontend-facing REST API)

### 4. UI Layer (web-2)
- **Owned:** React components, Tailwind styling, client-side routing, `apiClient.ts` HTTP calls
- **Forbidden:** Backend logic, business rules, persistence, QR generation, OS telemetry (ADR 0002)
- **Protocol:** HTTP GET/POST → Go :9090 or Tauri IPC

**Temporary violations (ADR 0002 deferred):**
- `web-2/server.ts` currently has QR generation + system telemetry (should be Go HTTP endpoints + Nginx auto-start)

---

## Data Flow (Concrete Example: Crew Turn Execution)

```
1. User clicks "Run crew" in UI
   ↓ HTTP POST /api/fleet/crews/{id}/run
2. Go HTTP handler (engine/src/fleet/handlers.go)
   ↓ calls crew.Services.StartTurn()
3. Go crew orchestrator (engine/src/crew/services.go)
   ↓ builds turn context (messages, tools, memory)
   ↓ gRPC call → Rust SystemGatewayService.ExecuteTurn (ADR 0001)
4. Rust turn executor (crates/clawcrew-runtime/src/agent/turn.rs)
   ↓ calls clawcrew-providers (OpenAI/Gemini/Bedrock/Anthropic)
   ↓ streams tokens back to Go
5. Go receives completion
   ↓ saves to memory (vector + SQLite)
   ↓ HTTP 200 response → UI
6. UI updates chat messages
```

**Key invariant:** UI never calls Rust gRPC :50052 directly (no credential, no route). Go engine = gateway.

---

## Persistence Layer

| Data | Owner | Storage | Access |
|------|-------|---------|--------|
| Fleet config (crews, policies) | Go | `engine/data/fleet.db` (SQLite) | Go `fleet/store.go` |
| Chat history | Go | `engine/data/chats.db` (SQLite) | Go `crew/store.go` |
| Vector embeddings | Go | `engine/data/vectors/` (ChromaDB/Qdrant) | Go `memory/rag.go` |
| Secrets (API keys, tokens) | Rust | `~/.config/galleon/vault/` (encrypted) | Rust `clawcrew-gateway/vault.rs` |
| Tauri window state | Tauri | `~/.config/galleon/window.json` | Tauri `apps/tauri-2/main.rs` |

**No shared database** — each tier owns its persistence boundary.

---

## Network Ports

| Port | Service | Access |
|------|---------|--------|
| :50051 | Go gRPC `AgentEngineService` | **Internal only** (Go ↔ Rust) |
| :50052 | Rust gRPC `SystemGatewayService` | **Internal only** (Go client) |
| :9090 | Go HTTP REST API | **Frontend-facing** (web-2, Tauri) |
| :5173 | Vite dev server (web-2) | **Dev only** (prod = static bundle in Tauri) |

**Production:** Tauri bundles web-2 as static assets, no separate web server. All HTTP calls → `http://localhost:9090` (Go engine).

---

## Crate Structure (Rust)

See `docs/book/src/architecture/crates.md` (archived KiroCrew upstream) for detailed dependency graph. Key abstractions:

- `clawcrew-gateway`: gRPC server, vault, secret management
- `clawcrew-runtime`: Agent turn execution, tool dispatch, message streaming
- `clawcrew-providers`: LLM vendor APIs (OpenAI, Gemini, Bedrock, Anthropic) — **SSOT (ADR 0001)**
- `clawcrew-channel-*`: Channel plugins (Discord, Telegram, Slack, etc.)
- `clawcrew-tools-*`: Native tool implementations (filesystem, shell, network)

**Naming:** New crates use `galleon-*` prefix (ADR: brand consistency). Legacy `clawcrew-*` crates remain unchanged unless refactored.

---

## Module Structure (Go)

```
engine/
├── cmd/agent-engine/main.go    # Entrypoint, Wire DI
├── app/app.go                  # gRPC + HTTP server lifecycle
├── src/
│   ├── fleet/                  # Fleet policies, crew registry
│   ├── crew/                   # Agent orchestration, turn lifecycle
│   ├── memory/                 # RAG, vector store, embeddings
│   ├── workflow/               # Workflow engine
│   ├── llm/                    # [DEPRECATED ADR 0001] Remove after Rust delegation
│   └── tools/                  # [DEPRECATED ADR 0001] Remove after Rust delegation
└── data/                       # SQLite databases, config files
```

**Wire DI:** `app/wire.go` + `//go:generate wire` generates `wire_gen.go` (dependency injection, no reflection).

---

## Communication Protocols

### Go ↔ Rust (Internal)

**gRPC with protobuf** (`engine/proto/*.proto`):
- `SystemGatewayService` (Rust server, Go client): Tool execution, secret access, **LLM execution (ADR 0001)**
- `AgentEngineService` (Go server, Rust client): Fleet policy queries, crew state (not yet implemented, placeholder)

**Why gRPC?** Type safety, streaming, backward compatibility (protobuf versioning).

### Frontend ↔ Backend (External)

**HTTP REST** (Go `engine/src/fleet/handlers.go`, `crew/handlers.go`):
- `/api/fleet/*` — Fleet management (crews, policies, roster)
- `/api/crews/*` — Crew operations (start turn, history, artifacts)
- `/api/system/*` — System metrics, health, telemetry
- `/api/network/*` — [TEMP ADR 0002] QR code generation (should be Nginx)

**WebSocket** (Rust `clawcrew-gateway/ws.rs`):
- `/ws/chat` — Real-time message streaming (LLM tokens, tool events)
- Handled by Rust (Go has no WebSocket logic)

---

## Plugin Architecture

**WASI Components** (Rust `clawcrew-runtime/plugin/`):
- Tools, channels, and memory plugins as `.wasm` modules
- Sandboxed execution (WASI preview2, no host filesystem access by default)
- Discovery: `~/.config/galleon/plugins/` at runtime

See `docs/book/src/plugins/` (archived upstream) for plugin authoring guide.

---

## Security Model

### Secrets (ADR: Rust = SSOT)

- API keys stored in `~/.config/galleon/vault/` (AES-256-GCM encrypted)
- Rust `VaultService` = single accessor (Go requests via gRPC, never reads vault directly)
- Secret scrubbing in logs (credential regex filter, exfiltration URL detection)

### Sandboxing

- Tauri IPC: allowlist-only (commands explicitly registered in `tauri.conf.json`)
- WASI plugins: no network, no filesystem by default (explicit capability grants)
- Go HTTP: CORS restricted to `localhost` (no external origin in dev/prod)

---

## Testing Strategy

| Layer | Test Type | Location | CI Gate |
|-------|-----------|----------|---------|
| Rust crates | Unit + integration | `crates/*/tests/` | `cargo test --all` |
| Go packages | Unit + integration | `engine/src/*/services_test.go` | `go test ./...` |
| Tauri commands | E2E (WebDriver) | `apps/tauri-2/tests/` | `cargo test --features e2e` |
| UI components | Vitest + React Testing Library | `web-2/src/**/*.test.tsx` | `npm test` |

**Full CI:** `./dev/ci.sh all` (runs all gates + linters).

---

## Deployment Targets

- **Desktop:** `.exe` (Windows), `.dmg` (macOS), `.AppImage` (Linux) via Tauri bundler
- **Server (dev):** `docker compose up` (Go engine + Rust gateway as separate containers)
- **Production (self-hosted):** Binary release (`galleon-engine` Go binary + `galleon-gateway` Rust binary), systemd services

---

## Performance Characteristics

| Metric | Target | Measured (2026-10-03) |
|--------|--------|----------------------|
| Cold start (Tauri) | <3s | 2.1s (M3 Mac, 1.8s Windows 11) |
| HTTP latency (Go) | <50ms | 18ms p50, 42ms p99 |
| gRPC overhead (Go→Rust) | <5ms | 1.2ms p50, 3.8ms p99 |
| LLM streaming (Rust) | First token <500ms | 320ms (OpenAI gpt-4), 180ms (Gemini) |

**Bottleneck:** LLM API latency (vendor-dependent, not system overhead).

---

## Observability

- **Logs:** Structured JSON (`tracing` in Rust, `zap` in Go)
- **Metrics:** Prometheus-compatible `/metrics` endpoint (Go HTTP :9090)
- **Tracing:** OpenTelemetry spans (optional, `OTEL_EXPORTER_OTLP_ENDPOINT` env var)

---

## Migration Notes (from KiroCrew upstream)

Galleon forked from `clawcrew` (KiroCrew upstream). Key differences:

| Aspect | KiroCrew | Galleon |
|--------|----------|---------|
| Branding | ClawCrew | **Galleon** (ship metaphor, fleet = teams) |
| LLM SSOT | Go `engine/src/llm/` | **Rust `clawcrew-providers`** (ADR 0001) |
| Frontend | Single-page dashboard | **Tauri desktop app** + web dashboard |
| Persistence | Postgres-only | **SQLite (dev) + Postgres (prod)** |

Archived upstream docs → `docs/_archive/book-kirocrew-upstream/`.

---

## Related Documents

- [ADR 0001: LLM SSOT Rust](../decisions/0001-llm-ssot-rust.md) — Why Rust executes LLM calls, not Go
- [ADR 0002: web-2 Backend Policy](../decisions/0002-web2-backend-policy.md) — Zero logic in Express server
- [ADR 0003: Tauri Metrics Real API](../decisions/0003-tauri-metrics-real-api.md) — Fake data → real API read
- [Tech Stack: Go Conventions](../tech-stack/go-conventions.md) — Wire DI, gRPC patterns
- [Tech Stack: Rust Conventions](../tech-stack/rust-conventions.md) — Cargo workspace, trait boundaries
- [API Contracts: gRPC System Gateway](../api-contracts/grpc-system-gateway.md) — Rust :50052 spec
- [API Contracts: HTTP Engine API](../api-contracts/http-engine-api.md) — Go :9090 REST routes
