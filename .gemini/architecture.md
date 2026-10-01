# Galleon Fleet AI: Architecture & Layering Specification

This document defines the canonical 3-tier architecture, system boundaries, interaction flows, and coding standards for the Galleon Fleet AI repository.

---

## 1. High-Level Architecture & Clean Separation of Concerns

The codebase is strictly structured into three distinct operational tiers:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           TIER 1: INTERFACE LAYER                           │
│           Web App / PWA (`web-2/`) & Desktop Webview (`apps/tauri-2`)       │
│                                                                             │
│   • React 19 + TypeScript + Tailwind CSS                                    │
│   • State Management: Zustand (`useFleetStore`)                             │
│   • Client Bridge: `apiClient.ts` (gRPC-Web / HTTP Proxy / Tauri IPC)       │
│   • STRICT RULE: Pure presentation. Zero backend logic. Zero data mocking.  │
└───────────────────────┬─────────────────────────────────┬───────────────────┘
                        │ HTTP / gRPC                     │ Tauri Native IPC
                        │ (Fleet Operations & AI)         │ (Core OS Execution)
                        ▼                                 ▼
┌─────────────────────────────────────────┐  ┌────────────────────────────────┐
│       TIER 2: GO ORCHESTRATOR           │  │      TIER 3: RUST CORE         │
│               (`engine/`)               │  │ (`crates/`, `apps/tauri-2/src`)│
│                                         │  │                                │
│ • High-level Agent Engine & Brain       │  │ • Native Host Execution & PTY  │
│ • Fleet Missions, Quests, & Workflows   │  │ • Security Sandbox (Landlock)  │
│ • Crew, Squad, & Ship Governance        │  │ • Host Hardware Sysinfo (OS)   │
│ • Memory Indexing & Vector Search       │  │ • Encrypted Secret Vault       │
│ • Persistence (`DiskStore` / JSON DB)   │  │ • Tauri v2 Desktop Wrapper     │
│ • Services: gRPC (:50051), HTTP (:9090) │  │ • Service: `SystemGateway`     │
└────────────────────┬────────────────────┘  └────────────────┬───────────────┘
                     │                                        ▲
                     │   gRPC: `SystemGateway.ExecuteTool`    │
                     └────────────────────────────────────────┘
```

---

## 2. In-Depth Comparison: `apps/tauri-2` vs `crates/` vs `engine/`

| Dimensi | `apps/tauri-2` (Desktop Shell) | `crates/` (Rust System Core) | `engine/` (Go Orchestrator) |
|---|---|---|---|
| **Peran Utama** | Desktop Window Shell & OS Wrapper | Security Microkernel & Native Tools | AI Brain & Workflow Orchestrator |
| **Bahasa & Runtime** | Rust (Tauri v2 + WRY/Webview2) | Rust (2024 Edition, Tokio async) | Go 1.25+ / Go 1.27 (Google Wire DI) |
| **Lokasi Kode** | `apps/tauri-2/src/main.rs` | `crates/clawcrew-*`, `crates/galleon-*` | `engine/src/`, `engine/cmd/agent-engine/` |
| **Tugas Riil** | 1. Membungkus UI `web-2` jadi native `.exe` / desktop window.<br>2. Tray icon, system tray menu di taskbar.<br>3. Native OS dialogs (file picker, alert) & notification.<br>4. Forwarding low-level OS command ke local desktop. | 1. Sandboxing OS dengan Linux Landlock LSM / Windows integrity.<br>2. Eksekusi native tools (bash, terminal PTY, disk read/write).<br>3. Hardware telemetry (CPU/RAM/GPU kernel metrics).<br>4. Network discovery & A2A mesh via mDNS (`clawcrew-gateway`).<br>5. External channel adapters (Discord, Slack, Matrix).<br>6. WASI/Wasmtime sandboxed plugins. | 1. Otak keputusan AI & Agent state machine (`StartTurn`).<br>2. Perencanaan misi, Quests breakdown, Map steps.<br>3. Fleet governance: Ships, Squads, Crew berths, Approvals.<br>4. Semantic memory, vector embeddings, SQLite FTS5 RAG.<br>5. Persistensi kanonikal data armada via `DiskStore`.<br>6. Treasury cost tracking & token expenditure. |
| **Jalur Komunikasi (Inbound)** | Tauri IPC `invoke('cmd', payload)` dari Webview. | gRPC Server `SystemGateway` (:50052) dipanggil oleh Go; atau direct CLI. | 1. gRPC Server `AgentEngine` (:50051).<br>2. HTTP Gateway (:9090) dipanggil via reverse proxy `web-2`. |
| **Jalur Komunikasi (Outbound)** | IPC response / Webview event emit. | Mengembalikan ToolCallResponse / Exit Code ke Go Orchestrator. | 1. gRPC Client ke Rust `SystemGateway` (:50052) untuk run tools.<br>2. Stream TurnResponse ke Frontend/UI. |
| **Yang DILARANG di sini** | Dilarang menaruh business logic orkestrasi agen atau data fleet. | Dilarang menduplikasi logic quest/crew planning yang milik Go. | Dilarang mengakses kernel/hardware tanpa melalui sandbox Rust Core. |

---

## 3. Tier Boundaries & Responsibilities

### Tier 1: Interface Layer (`web-2/`)
- **Location:** `web-2/src/`
- **Responsibilities:**
  - Rendering UI views (Quarterdeck, Mission Board, Crew, Ships, Artifacts, Harbor, Crow's Nest, Engine Room).
  - Client-side navigation, theme toggling, and input capture.
  - Calling backend APIs via `apiClient.ts`.
- **Strict Invariants:**
  - **NO backend logic or file persistence inside `server.ts` or `.tsx`.** `web-2/server.ts` acts solely as a static asset server and reverse proxy forwarding `/api/*` to the Go Orchestrator.
  - **NO client-side data mocking or hardcoded HTML arrays.**
  - **All dynamic data must be fetched from Go Orchestrator or Rust Core.**

### Tier 2: Orchestrator Layer (`engine/`)
- **Location:** `engine/`
- **Language:** Go 1.25+ / Go 1.27
- **Responsibilities:**
  - Agent Engine lifecycle (`StartTurn`, `QuickQuery`).
  - Fleet state, Quests, Squads, Crew members, Approvals, Treasury, Artifacts.
  - Executive briefing, Crow's Nest doctor diagnostics, disaster recovery snapshots.
  - Persistent storage in JSON/JSONL via `DiskStore`.
  - Exposes gRPC server on `:50051` and HTTP REST endpoints on `:9090` (proxied by web gateway).
  - Invokes Rust Core via `SystemGateway` gRPC client whenever native shell or tool execution is needed.

### Tier 3: System Core Layer (`crates/`, `apps/tauri-2/`)
- **Location:** `crates/clawcrew-*`, `apps/tauri-2/src/`
- **Language:** Rust (2024 edition)
- **Responsibilities:**
  - Low-level OS execution, terminal processes, and PTY sessions.
  - Kernel-level sandboxing (Landlock LSM).
  - Host OS metrics (raw CPU, RAM, Windows/Linux kernel metrics).
  - Encrypted secret vault and credentials.
  - Desktop native windowing shell (Tauri v2).
  - Exposes `SystemGateway` gRPC service for Go and desktop IPC handlers for local webview.

---

## 3. Communication & Data Flow

### A. Agent Turn & Fleet Execution Flow
1. User enters task/prompt in `web-2/` UI (Quartermaster Office or Mission Board).
2. UI calls `apiClient.chatQuartermaster(content)` or `createQuest(quest)`.
3. Request is routed via HTTP/gRPC to **Go Orchestrator (`engine/`)**.
4. Go Orchestrator analyzes the objective, plans task execution, and checks policies.
5. If the agent needs to run a shell command or read disk files, Go calls **Rust Core (`SystemGateway.ExecuteNativeTool`)** via gRPC.
6. Rust Core executes the tool inside the Landlock OS sandbox, enforcing security policies, and returns the result.
7. Go Orchestrator records the output, updates memory, and streams events back to UI.

### B. Fleet Persistence Flow
1. User creates or modifies a quest, crew berth, ship charter, or policy directive in UI.
2. UI triggers action in `useFleetStore`, which calls `apiClient.saveCollection(name, data)`.
3. Go Orchestrator receives the update at `/api/collections/{name}`, validates the schema, and saves it to disk via Go's persistence layer.
4. Changes are durable and synchronized across all interface views.

---

## 4. Coding Standards

### Go Standards (`engine/`)
- **Package Layout:** Clean architecture (`engine/cmd/`, `engine/app/`, `engine/core/`, `engine/src/<domain>/`, `engine/pkg/`).
- **Standard Library First:** Favor `net/http`, `log/slog`, `context`, `sync`, and `encoding/json` before external dependencies.
- **Error Handling:** Explicit error checking and propagation using `github.com/diezy-labs/claw-crew/engine/core/errors`. Zero unhandled errors.
- **Dependency Injection:** Wire (`github.com/google/wire`) providers in `wire.go`.
- **Logging:** Structured logging only via `slog.Info`, `slog.Error`, `slog.Warn`.

### Rust Standards (`crates/`, `apps/tauri-2/`)
- **Error Propagation:** Always return `Result<T, E>` and use `?`. Do not use `unwrap()` or `expect()` in production paths.
- **Safety & Isolation:** External actions must default closed. Sandboxed execution must respect Landlock boundaries.
- **Integration Tests:** Move integration tests to `tests/` directories; keep inline `#[test]` minimal.

### TypeScript / Frontend Standards (`web-2/`)
- **Strict Typing:** All props, stores, and API clients must be strictly typed matching `src/types/index.ts`. No `any` escapes.
- **Zero Mocking In UI:** Components must never contain fallback mock arrays. All state originates from `useFleetStore` backed by real API calls.
- **Component Hygiene:** Reusable components follow atomic design (`components/common/` for UI primitives, `components/features/` for domain views).
