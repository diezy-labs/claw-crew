# ADR 0002: web-2 Backend Logic Policy — Zero Logic, Nginx + Go API

**Status:** Accepted  
**Date:** 2026-10-03  
**Deciders:** Achmad (Product Owner), squad-lead (Tech Lead)

---

## Context

`web-2/server.ts` currently contains **backend business logic**:

1. **QR code generation** (`/api/network/qrcode`) — uses `qrcode` NPM library, generates data URL for remote device access
2. **System telemetry** (`/api/system/metrics`) — reads `os.cpus()`, `os.totalmem()`, calculates uptime

This **violates** `.gemini/architecture.md` claim: "web-2 = TS, React 19, rendering, view-model, **zero backend logic**."

The original intent (per Achmad): **"QR itu menggunakan Nginx jadi ketika server di nyalakan, web-2 otomatis menyalakan Nginx untuk bisa digunakan di device lain."**

However:
- `web-2/nginx.conf` exists but is **manual install** (comment: "Place this inside /etc/nginx/sites-available") — not auto-started
- QR code logic currently lives in `server.ts` as a **temporary workaround**

---

## Decision

**web-2 MUST NOT contain backend business logic.** Its **only** role is:

1. **Serve static assets** (React SPA build output)
2. **Reverse-proxy `/api/*` to Go engine** `:9090` (already implemented correctly)
3. **WebSocket upgrade proxy** for HMR + live streaming (already in `nginx.conf`)

All fleet-domain logic (QR, telemetry, metrics, policies) **belongs in Go engine HTTP API** or Nginx static serve.

---

## Rationale

### 1. Separation of Concerns

- **web-2 = View layer** (UI rendering + HTTP transport) — should be **stateless passthrough**
- **Go engine = AI Orchestrator + Fleet API** (all business logic, metrics, policies)
- **Nginx = Static asset CDN + TLS termination** (QR serve, gzip, cache headers)

Mixing business logic into `server.ts` creates:
- **Duplicate SSOT** (telemetry in Go `/api/system/metrics` AND TS `server.ts` line 73)
- **Unclear boundary** (is web-2 a backend or not?)
- **Harder testing** (need to test TS business logic separately from Go API)

### 2. Multi-Device Access via Nginx

Per original intent: **Nginx serves static QR code** for remote device access. Benefits:
- QR is **cacheable static asset** (no compute per-request)
- Nginx `gzip` + `expires 7d` already configured in `nginx.conf`
- **Zero TS code** — QR generation happens at **build time or Go API**, TS just proxies

### 3. Production Deployment Consistency

Current `web-2/server.ts` logic (QR + telemetry) is **development-only convenience**. In production:
- Nginx serves frontend (not Node.js `server.ts`)
- All `/api/*` routes **reverse-proxy to Go :9090**
- No TS backend process runs

So `server.ts` logic **only works in dev**, breaks in prod. Moving to Go/Nginx makes dev==prod consistent.

---

## Consequences

### Positive

- **Clear tier boundary** — web-2 is pure view, Go is pure API (no confusion)
- **Consistent SSOT** — all metrics/telemetry in Go, no duplicate in TS
- **Simpler deployment** — Nginx + Go engine, no Node.js backend
- **Better caching** — QR + static assets served by Nginx (fast, CDN-ready)

### Negative

- **Requires Nginx setup** — dev env needs `nginx` installed + configured (one-time cost)
- **Go HTTP API must expose QR endpoint** — `GET /api/network/qrcode?url=...` (new route)

### Neutral

- **Dev experience unchanged** — `npm run dev` still works, just proxies QR to Go instead of generating in TS

---

## Implementation Plan (Next Phase — Not Blocker for Standard Docs)

### Phase 1: Go Engine QR Endpoint (1 day)
- [ ] Add `GET /api/network/qrcode?url=...` to Go `fleet/delivery.go`
- [ ] Use Go QR library (e.g. `github.com/skip2/go-qrcode`)
- [ ] Return JSON `{ url, dataUrl }` (same contract as current TS)
- [ ] Test with `curl localhost:9090/api/network/qrcode?url=http://...`

### Phase 2: Nginx Auto-Start Script (1 day)
- [ ] Create `scripts/dev-nginx.sh` — auto-start Nginx for dev env
- [ ] Update `web-2/package.json` — `npm run dev` calls `dev-nginx.sh` before Vite
- [ ] Test multi-device access (scan QR from phone, access `http://<local-ip>:80`)

### Phase 3: Remove TS Backend Logic (1 day)
- [ ] Delete `web-2/server.ts` lines 53-77 (QR code generation)
- [ ] Delete `web-2/server.ts` lines 73-95 (system telemetry)
- [ ] Update `web-2/src/lib/apiClient.ts` — QR calls Go `/api/network/qrcode`
- [ ] Verify `/api/system/metrics` already proxied to Go (no change needed)

---

## Current Status

**Temporarily ACCEPTED as-is** for Phase 1 standard docs. QR + telemetry logic in `server.ts` is **technical debt** (marked for next refactor phase), **NOT** a standard to preserve.

When writing `standard-docs/tech-stack/web-conventions.md`, document the **target state** (zero logic, Nginx serve), not the current workaround.

---

## Related

- ADR 0001: LLM SSOT is Rust (Go engine thins out, focuses on orchestration)
- ADR 0003: Tauri Metrics Policy (also moves from fake to real API read)
- `standard-docs/architecture/01-boundary-policy.md` (tier separation rules)
