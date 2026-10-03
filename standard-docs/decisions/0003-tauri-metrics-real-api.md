# ADR 0003: Tauri System Metrics — Real API Read, Not Fake Data

**Status:** Accepted  
**Date:** 2026-10-03  
**Deciders:** Achmad (Product Owner), squad-lead (Tech Lead)

---

## Context

Tauri v2 desktop shell command `get_system_metrics()` currently **hardcodes fake display data**:

```rust
serde_json::json!({
    "gateway_latency_ms": 8,              // FAKE — always 8
    "active_threads": threads,             // REAL — os.available_parallelism()
    "isolation_mode": "Landlock & Tauri Sandboxed",  // FAKE — string constant
    "memory_db_mb": 18.4                  // FAKE — always 18.4
})
```

Comment in `apps/tauri-2/src/main.rs` line 111 says: _"Host-local telemetry (genuine local measurement, not fleet-domain data — not an engine SSOT concern)."_

However:
- `gateway_latency_ms` 8 is **NOT** a measurement — it's a fake placeholder
- `memory_db_mb` 18.4 is **NOT** measured — it's a UI mockup constant
- Only `active_threads` is real (reads `std::thread::available_parallelism()`)

Per Achmad: **"Tauri harus nya tidak fake metrics, itu hanya mock desain, baik nya ambil dari Api real."**

---

## Decision

Tauri command `get_system_metrics()` **MUST read real data from Go engine API** `/api/system/metrics`, NOT hardcode fake constants.

The command will become:

```rust
#[tauri::command]
async fn get_system_metrics() -> Result<Value, String> {
    engine_get("system/metrics").await  // Delegate to Go engine
}
```

---

## Rationale

### 1. SSOT Consistency

Go engine **already exposes** `/api/system/metrics` via HTTP :9090 (currently served by `web-2/server.ts` as temporary workaround — will move to Go per ADR 0002). Tauri reading **its own fake constants** creates:

- **Duplicate telemetry** (Tauri fake vs Go real)
- **Inconsistent UI** (Tauri desktop shows 8ms latency, web shows real 23ms)
- **No real observability** (fake data cannot diagnose performance issues)

### 2. Mockup vs Production

Comment justifies fake data as "genuine local measurement, not fleet-domain". But:

- **Fake is not measurement** — hardcode 8ms/18.4MB is UI placeholder, not telemetry
- **Tauri IS production** — desktop shell is not a mockup, it's a shipping SKU
- **Real measurements exist** — Go engine `/api/system/metrics` is the SSOT, already implemented

### 3. Tier Boundary Clarity

Per `.gemini/architecture.md` (corrected):

- **Tauri = Desktop Shell** — window mgmt, tray, dialog, IPC — **NOT** metrics backend
- **Go engine = AI Orchestrator + Metrics SSOT** — all fleet-domain + system telemetry

Tauri hardcoding metrics violates tier separation (it becomes a partial metrics backend).

---

## Consequences

### Positive

- **Real telemetry** — Tauri UI shows actual gateway latency, memory usage (debuggable)
- **Consistent SSOT** — all metrics read from Go `/api/system/metrics` (web + desktop show same data)
- **Simpler Tauri code** — `get_system_metrics()` becomes 1-line proxy, no fake constants
- **Production-ready** — desktop shell reports real system state, not UI placeholders

### Negative

- **Network dependency** — Tauri command now requires Go engine reachable at `:9090`
  - Mitigation: engine is localhost-only, fast (<1ms)
  - Fallback: return error JSON if engine unreachable (graceful degradation)

### Neutral

- **UI rendering unchanged** — frontend expects same `{ gateway_latency_ms, active_threads, isolation_mode, memory_db_mb }` JSON shape

---

## Implementation Plan

### Phase 1: Go Engine Metrics Endpoint (Depends on ADR 0002 Phase 1)

- [ ] Move `/api/system/metrics` from `web-2/server.ts` to Go `fleet/delivery.go`
- [ ] Add real measurements:
  - `gateway_latency_ms` — measure gRPC round-trip to Rust :50052
  - `memory_db_mb` — read Go runtime `runtime.ReadMemStats()`
  - `active_threads` — read `runtime.GOMAXPROCS(0)` or OS thread count
  - `isolation_mode` — read from Rust gRPC `GetSandboxMode()` if available

### Phase 2: Tauri Command Refactor (1 hour)

- [ ] Replace `get_system_metrics()` body with `engine_get("system/metrics").await`
- [ ] Remove fake constants (gateway_latency_ms: 8, memory_db_mb: 18.4, isolation_mode string)
- [ ] Keep `active_threads` local read as fallback (if engine call fails)
- [ ] Test: open Tauri app, verify metrics panel shows real data

### Phase 3: Error Handling (1 hour)

- [ ] Add graceful degradation if Go engine unreachable:
  ```rust
  async fn get_system_metrics() -> Result<Value, String> {
      match engine_get("system/metrics").await {
          Ok(metrics) => Ok(metrics),
          Err(e) => Ok(serde_json::json!({
              "error": "Engine unreachable",
              "message": e,
              "active_threads": std::thread::available_parallelism()
                  .map(|n| n.get() as u32 * 2)
                  .unwrap_or(16)
          }))
      }
  }
  ```
- [ ] Test: kill Go engine, verify Tauri shows error state (not blank)

---

## Current Status

**Temporarily ACCEPTED as-is** for Phase 1 standard docs. Fake metrics are **UI mockup technical debt**, not a standard to preserve.

When writing `standard-docs/tech-stack/tauri-conventions.md`, document the **target state** (delegate to Go API), not the current placeholder.

---

## Related

- ADR 0001: LLM SSOT is Rust (Go engine focuses on orchestration + metrics SSOT)
- ADR 0002: web-2 Backend Policy (system telemetry moves from TS to Go)
- `standard-docs/architecture/01-boundary-policy.md` (Tauri = shell only, no backend logic)
