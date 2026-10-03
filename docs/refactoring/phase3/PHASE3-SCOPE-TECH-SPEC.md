# Phase 3 — Tech Spec: Multi-Fleet ExecuteNativeTool + Orphaned API Handlers

> Status: DRAFT — awaiting Backend/Frontend feasibility feedback, then Achmad approval before coding starts.
> Author: squad-lead. Date: 2026-10-02.
> Supersedes nothing; this is a NEW doc. The existing `docs/refactoring/phase3/{tool-calling,provider-routing,action-plan}/`
> tree is a DIFFERENT, earlier-dated (2026-09-28) Phase 3 scope (Governed Tool Runtime + Claw Model Gateway) and is
> out of scope here — do not merge the two.

---

## 0. What I checked before writing this (ponytail rung 1-2: does it exist already?)

- `proto/agent_service.proto` — read in full. `ToolCallRequest` currently has only `tool_name` + `arguments_json`.
  No `workspace_path` / `timeout_seconds` / `exit_code` fields exist.
- `PR_PHASE2B_TEMPLATE.md` — Phase 2b is **merged** (commit `5a2c8dc0`, branch `phase2-c3-e2` → `main`). It records a
  **locked proto decision**: *"Option A (YAGNI): defer multi-fleet scope to Phase 3/4"* — explicitly naming
  `workspace_path`, `timeout_seconds`, `exit_code` as the deferred fields, approved by Lead Squad + Product Owner
  consensus on 2026-10-02. **This request is that deferral coming due, not a new idea** — framing it as "extension"
  below, not "new feature we're guessing at."
- `crates/clawcrew-gateway/src/grpc_system_gateway.rs` — `SystemGatewayService::execute_native_tool` has ONE
  construction-time-bound sandbox root (no per-request workspace param anywhere in the struct or method).
- `engine/pkg/client/system_gateway.go` — `ExecuteNativeTool(ctx, toolName, argumentsJSON)` — no workspace/timeout args.
- `web-2/src/utils/apiClient.ts` (full file) cross-referenced against every `server.RegisterRouteFunc("/api/...")` in
  `engine/src/**/delivery.go` (8 delivery files, 24 registered routes total). Grep-verified, not guessed.
- `docs/refactoring-phase2/task-breakdown.md` — item **C4** ("Samakan kontrak route frontend↔engine") is marked `[ ]`
  (not done) in that doc, but the PR template says C4 route audit for "7 engine modules" already happened in Phase 2b.
  Treating the PR template's audit as the real C4 pass; this doc's 6-endpoint list is the audit's output, written up
  here for the first time.

## 1. Correction to the request's premise

The request lists 6 orphaned endpoints. I grep-verified all 6 against `web-2/src/utils/apiClient.ts` and every
registered Go route. Result: **5 of 6 are real orphans, 1 does not exist as named, and there is a 7th orphan the
request missed.**

| # | Endpoint (as requested) | Found in apiClient.ts? | Registered in any `delivery.go`? | Verdict |
|---|---|---|---|---|
| 1 | `GET /api/system/metrics` | ✅ line 107 | ❌ no match | **Real orphan** |
| 2 | `GET /api/engine/processes` | ✅ line 206 | ❌ no match | **Real orphan** |
| 3 | `POST /api/engine/execute` | ✅ line 229 | ❌ no match | **Real orphan** |
| 4 | `GET /api/health` | ✅ line 270 | ❌ no match | **Real orphan** |
| 5 | `GET /api/system/network` | ✅ line 275 | ❌ no match | **Real orphan** |
| 6 | `GET /api/providers/ollama/status` | ✅ line 280 | ❌ no match | **Real orphan** |
| 7 | `POST /api/providers/ollama/generate` | ✅ `RemoteAccessModal.tsx:59` (bypasses apiClient directly) | ❌ no match | **Extra orphan, not in the request's list — add to scope** |

Already-registered and NOT orphaned (so excluded, do not rebuild): `/api/system/executive-briefing`,
`/api/providers/harbor`, `/api/fleet/{metrics,deck-bell,seed}`, all `/api/v1/*` CRUD routes, `/api/turn`, `/api/query`.

**Recommendation:** fold `/api/providers/ollama/generate` into endpoint #6's work item (same tier, same file, same
PR) rather than opening a 7th item — it's the same provider surface, split by GET/POST only.

## 2. Primary Track — Multi-Fleet `ExecuteNativeTool` Extension

### 2.1 Proto change (`proto/agent_service.proto`)

```proto
message ToolCallRequest {
    string tool_name = 1;
    string arguments_json = 2;
    optional string workspace_path = 3;   // NEW — absolute path, defaults to construction-time sandbox root if unset
    optional int32 timeout_seconds = 4;    // NEW — per-request override, defaults to gateway's existing hardcoded timeout if unset
}

message ToolCallResponse {
    bool success = 1;
    string output = 2;
    string error = 3;
    optional int32 exit_code = 4;          // NEW — populated for shell-exec-style tools (bash, git); absent for read_file/write_file
}
```

Both request fields are `optional` (proto3 explicit presence) — a Go caller or Rust callee that doesn't set them
sees the field as unset, not zero-value `""`/`0`, so there is no silent-timeout-becomes-0 footgun. This is the
backward-compat mechanism: Phase 2b callers that never set these fields behave byte-identically to today.

### 2.2 Go Engine — workspace routing (`engine/pkg/client/system_gateway.go`, `engine/src/tool/builtin_*.go`)

Current: `ExecuteNativeTool(ctx, toolName, argumentsJSON) (string, error)` — single sandbox, implicit.

New signature:
```go
ExecuteNativeTool(ctx context.Context, toolName, argumentsJSON, workspacePath string, timeoutSeconds int32) (string, int32, error)
```

Routing logic lives in the Go client wrapper, NOT in a new abstraction: `workspacePath == ""` → omit the proto field
(nil pointer / proto3 optional unset) → Rust gateway falls back to its existing construction-time root exactly as
today. A non-empty `workspacePath` is passed through to Rust untouched; **Go does not validate or resolve it** —
that is Rust's job (§2.3), because Rust owns the sandbox boundary (Landlock) and is the only side that can safely
canonicalize+contain a path. Go validating it too would be a second, driftable copy of the same security check.

### 2.3 Rust Systemgateway — workspace + timeout enforcement (`crates/clawcrew-gateway/src/grpc_system_gateway.rs`)

This is where the actual new logic lives — everything upstream is plumbing.

- `execute_native_tool` reads `req.workspace_path`. If present: canonicalize it, then reuse the **existing**
  `validated_path` / sandbox-containment check already in this file (the one `write_file`/`read_file` already call)
  — do not write a second path-validation routine. If the canonicalized path escapes the allowed fleet-workspace
  root set (see §2.4), return `success: false, error: "workspace_path outside allowed fleet roots"` — never silently
  clamp to the default root, because that would make an authz bug look like a success.
- `req.timeout_seconds`, if present, wraps the tool execution in `tokio::time::timeout(Duration::from_secs(n), ...)`
  instead of whatever fixed timeout `execute_bash`/`execute_git` use today — reuse `tokio::time::timeout`, it's
  already a dependency via tonic/tokio, no new crate.
  - **Overflow handling:** `timeout_seconds` is `i32` in proto. Clamp to `[1, 300]` server-side (1s floor, 5min
    ceiling) before constructing the `Duration` — a client sending `0` or a negative number must not produce an
    instant-timeout DoS-on-self, and a client sending `i32::MAX` must not block the gateway thread for 68 years.
    Reject (not clamp-silently) if the overflow is extreme (e.g. negative) — return `error: "timeout_seconds out of range [1,300]"`.
- `exit_code` is only meaningful for `bash`/`git` (anything that shells out). `read_file`/`write_file` leave it unset.
  Populate it from the existing `std::process::Command`/`tokio::process::Command` output's `.status.code()` — this
  is already captured by `execute_bash`, just not currently surfaced on the response; it is NOT new capture logic,
  only new wiring to put an existing value on the wire.

### 2.4 "Multi-fleet" — what this actually requires, not what it implies

The request's heading says "Multi-Fleet," but reading the actual Phase 2b decision and the current single-process
`SystemGatewayService`, there is **no multi-process/multi-instance fleet routing in this repo today** — one Rust
gateway process serves one Go engine over one gRPC channel on `:50052`. "Multi-fleet routing" in this phase is NOT
"spin up N gateway instances and route between them" (that would be a real architecture change, one-way-door, and
is explicitly NOT what Phase 2b deferred — Phase 2b deferred three *fields*, not a topology).

**What it actually is:** today's gateway enforces one sandbox root per process. The extension lets ONE gateway
process accept a per-request `workspace_path` scoped to an allow-list of fleet-workspace directories it already
knows about (e.g. each Ship's worktree), still within one process, one Landlock boundary set at startup to the
union of allowed roots. This is YAGNI-correct: it solves "which Ship's files does this tool call touch" without
inventing a routing/discovery layer nobody has asked for yet. If true multi-process fleet routing is wanted later,
that is a separate, bigger proposal — flag it, don't build it now.

**Open question for Achmad (needs a decision before Backend starts, not a Lead guess):** where does the allow-list
of fleet-workspace roots come from — a static config file, or `engine/data/ships.json` (already seed data per B2)?
I'd default to reading it from the Ships collection already seeded in Go and passing the resolved allow-list to Rust
at gateway startup (one more gRPC call or shared config file), since re-deriving it from scratch would duplicate
SSOT the project already fixed in B2/B3. Confirm before Backend commits to an approach.

### 2.5 Frontend E4 — new field UI (`web-2/src/components/features/`)

Only once Rust/Go accept the fields: expose `workspace_path` as a dropdown (populated from the same Ships list,
not free text — free text here is a path-traversal UI footgun) and `timeout_seconds` as a bounded numeric input
(`min=1 max=300`, matching the server clamp exactly so the UI never promises something the gateway will reject).
`exit_code` is read-only, shown in whatever terminal/output view already renders `ToolCallResponse.output`.

## 3. Secondary Track — 6 (+1) Orphaned API Handlers

All six (seven) are **read-mostly system/engine/provider introspection endpoints that already have a frontend
contract** (the TS caller + its expected JSON shape is the spec — I read it instead of inventing a new one).

| # | Route | Tier | Go package | Depends on | Notes from reading the TS caller |
|---|---|---|---|---|---|
| 1 | `GET /api/system/metrics` | system | `engine/src/fleet` (sibling of existing `handleMetrics`/`handleBriefing`) | none | Tauri-first: frontend tries `invoke('get_system_metrics')` before falling back to this HTTP route — Go handler is the **fallback path for non-Tauri (web) runs only**, so under-building it is low-risk; nothing load-bearing breaks on Tauri desktop. |
| 2 | `GET /api/engine/processes` | engine | new `engine/src/engineroom` or extend `fleet` | none | TS already defines the exact shape it maps FROM: `{pid, name, memory, cpu}[]`. Handler must return exactly that shape — the TS-side field renaming (`memory`→`memoryMB` etc.) happens client-side; do not rename server-side to match the UI type, that duplicates the mapping in two places. |
| 3 | `POST /api/engine/execute` | engine | same package as #2 | **MUST reuse the approval-gate pattern already in `engine/src/approval/**` and `engine/src/tool` policies_test.go (Phase 2b "Learning Governance")** — this is "execute arbitrary command," the single highest-risk item in this list. | Request body is `{command: string}` (seen in `apiClient.ts:229` POST body), response `{stdout, exitCode, duration}` (seen in TS return type). **Do NOT build a second approval mechanism** — call into the existing policy/approval service C5 already wired, same as D1's builtin tool approval flow. |
| 4 | `GET /api/health` | system | top-level `engine/src/health` or wherever `HealthCheckRequest`/`HealthCheckResponse` from the gRPC proto already live | none | The gRPC `HealthCheck` RPC already exists in `agent_service.proto` (`AgentEngine.HealthCheck`) — this HTTP handler should be a thin wrapper calling that existing RPC/service, not a parallel health implementation. Response needs `{uptime, version, status}` per the request; gRPC `HealthCheckResponse` only has `status` today, so uptime/version are two small additions to that existing message, not a new one. |
| 5 | `GET /api/system/network` | system | `engine/src/fleet` or new `engine/src/diagnostics` (same place C3 "GetDiagnostics" lives — this is the same kind of signal) | C3 (`GetDiagnostics`) is `[~]` in-progress per task-breakdown.md — **coordinate, don't duplicate**: if C3's diagnostics service already gathers host signals, add network info there instead of a sibling service. | TS return type is `Record<string, unknown>` (untyped) — low commitment on exact shape; still must return real data (ponytail: "not lazy about" doesn't apply to shape-looseness, but DOES apply to faking the numbers — C1's lesson about `GetMetrics` reading real state, not hardcoded fallbacks, applies here too). |
| 6+7 | `GET /api/providers/ollama/status`, `POST /api/providers/ollama/generate` | provider | `engine/src/fleet` next to existing `handleHarborProviders` (same tier/package, Ollama is just another provider in that same list) | none structurally, but needs an actual Ollama HTTP client call (`GET http://localhost:11434/api/tags` for status, `POST .../api/generate` for generate) | `RemoteAccessModal.tsx` shows the real expected shape: `{available, host, models?}` for status; `{response}` for generate passthrough. This is a thin proxy to local Ollama, not new business logic — do not add a provider abstraction layer for one provider (YAGNI; Harbor pattern already shows the project's provider-list convention, follow it, don't abstract further). |

### 3.1 Interface spec — request/response contracts (locked from existing frontend code, not invented)

```
GET  /api/system/metrics          -> 200 Record<string, unknown>              (shape: whatever get_system_metrics Tauri cmd returns, for parity)
GET  /api/engine/processes        -> 200 Array<{pid:int, name:string, memory:int, cpu:number}>
POST /api/engine/execute          -> body {command:string} -> 200 {stdout:string, exitCode:int, duration:string} | 403 (approval required/denied)
GET  /api/health                  -> 200 {status:string, uptime:string, version:string}
GET  /api/system/network          -> 200 Record<string, unknown>
GET  /api/providers/ollama/status -> 200 {available:bool, host:string, models?:Array<{name:string,...}>}
POST /api/providers/ollama/generate -> body {model:string, prompt:string, stream:bool} -> 200 {response:string}
```

### 3.2 Error handling (all 7)

- Standard shape, matching what's already used elsewhere in `delivery.go` (read one existing handler's error path
  before writing a new error type — do not invent a new error envelope for these 7).
- `/api/engine/execute`: approval-denied → `403` with the existing approval-rejection body shape from C5, not a
  bespoke message.
- Ollama unreachable (`/api/providers/ollama/status|generate`): `503` with `{available: false, error: "..."}` for
  status (never crash the handler because the LAN-local Ollama daemon isn't running — this is expected, not
  exceptional, for anyone without Ollama installed).
- `/api/system/network`/`metrics`: if a host-level syscall fails (e.g. permission denied reading `/proc`), return
  partial data with an `error` field per-metric rather than failing the whole response — matches C1's "metrics
  reflect real state" principle; a partially-unreadable host is still real state, not an excuse to fall back to
  fake numbers.

### 3.3 Backward compatibility

All 7 are net-new routes; nothing existing changes shape. Zero compat risk on this track by construction.

## 4. Risk Assessment — do the two tracks conflict?

**No file-level overlap, and only one soft coordination point:**

- Multi-fleet touches: `proto/agent_service.proto`, `engine/pkg/client/system_gateway.go`,
  `engine/src/tool/builtin_*.go`, `crates/clawcrew-gateway/src/grpc_system_gateway.rs`,
  new FE components for E4.
- Orphaned handlers touch: `engine/src/fleet/delivery.go` (+ possibly a new `engine/src/diagnostics` package),
  existing `engine/src/approval/**` (read-only, calling into it), `web-2/src/utils/apiClient.ts` is already correct
  (it's the frontend that's ahead of the backend here — no FE changes needed on this track except error-state UI).
- **Soft overlap:** item #5 (`/api/system/network`) and the in-progress C3 (`GetDiagnostics`) both want to live near
  host-signal gathering. Not a file conflict (C3's owner hasn't shipped yet per task-breakdown `[~]` status) but a
  **sequencing** one: whoever picks up #5 should check C3's current branch state first, or the two land as
  duplicate host-introspection code. Flagging this explicitly rather than letting two crews discover it mid-review.
- No shared-file-write collision between the two tracks → **they can run in parallel**, assigned to different
  people, with the one coordination note above communicated to whoever takes #5.

## 5. Effort, Phasing, Test Strategy

**Effort (re-derived from what was actually found, not restated from the request):**
- Multi-fleet: proto (0.5h) + Go plumbing (1.5h) + Rust enforcement incl. timeout/path validation reuse (3h) +
  FE E4 (2h) + tests (1.5h) ≈ **8.5h** — in the 8-10h band the request estimated; confirmed reasonable.
- Orphaned handlers: #1/#4 are near-trivial wrappers (~0.5h each). #2 is a straightforward process-list syscall
  wrapper (~1h). #5 needs the C3-coordination check first (~1h + coordination overhead). #6/#7 are a thin Ollama
  HTTP proxy (~1.5h for both, same handler file). #3 (`execute_execute`) is the one with real work because it must
  wire into the existing approval gate correctly (~2h, plus its own test for the approval-denied path specifically).
  Total ≈ **7h**, slightly above the request's 4-6h estimate — mainly because of #3's approval-gate wiring and the
  explicit C3-coordination step; flagging the delta rather than quietly absorbing it.
- **Revised total: ~15.5h** (vs request's 12-16h) — within range, call it 16h to be safe.

**Phasing: run in parallel, not sequential days-1-2/days-3-4** — §4 found no file conflict, so splitting into two
sequential days wastes a day. Assign multi-fleet to Backend (it is backend+Rust-gateway heavy and single-threaded
through the proto/Rust layer anyway) and the 7 orphaned handlers to a second Backend thread or Frontend-adjacent
dev (they're Go-only, no Rust). Frontend's E4 work for multi-fleet starts once the proto fields exist (small
dependency, not a full-phase gate).

**Test strategy:**
- Multi-fleet: (a) backward-compat test — existing `ExecuteNativeTool` call with no new fields set behaves
  byte-identically (reuse `execute_native_tool_accepts_allowlisted_tool` test, add a variant asserting unset-field
  defaults); (b) workspace_path outside allow-list → rejected, not clamped (new test, reusing existing sandbox
  validation helper); (c) timeout_seconds overflow/negative → rejected with range error (new unit test, no process
  spawn needed); (d) timeout_seconds triggers real timeout on a deliberately slow test command (integration test,
  reuse the `system_gateway_integration_test.rs` harness).
- Orphaned handlers: one handler test per route following whatever pattern `policies_test.go`/`integration_test.go`
  already use for the Phase 2b builtin handlers (table-driven Go tests) — do not introduce a new test framework for
  7 small handlers.

## 6. Open Items Requiring Achmad's Decision (not Lead's to guess)

1. Fleet-workspace allow-list source (static config vs. derived from `ships.json`) — §2.4.
2. Confirm folding `/api/providers/ollama/generate` into item #6's scope (7 items total) rather than leaving it
   unassigned, since it wasn't in the original 6.
3. Who owns #5 vs who owns in-flight C3 — needs a name assigned to avoid duplicate diagnostics code.
