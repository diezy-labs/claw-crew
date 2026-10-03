# HTTP Engine API (Go :9090)

**Status:** Living spec (updated with each API change)  
**Last updated:** 2026-10-03  
**Base URL:** `http://localhost:9090`  
**Protocol:** HTTP/1.1 REST + JSON

---

## Overview

Go engine exposes HTTP REST API on port :9090 for frontend-facing operations:
- Fleet management (crews, policies, roster)
- Crew operations (turn execution, history, artifacts)
- System telemetry (metrics, health, logs)

**Clients:**
- web-2 (React SPA, Vite dev server proxies `/api/*` to :9090)
- Tauri desktop app (IPC commands forward to :9090)

**NOT exposed:** gRPC :50051 (internal Go ↔ Rust only)

---

## Authentication

**Current:** None (local-only, trusted client)  
**Future:** JWT token in `Authorization: Bearer <token>` header

---

## Common Headers

### Request
```http
Content-Type: application/json
Accept: application/json
```

### Response
```http
Content-Type: application/json
Access-Control-Allow-Origin: http://localhost:5173
```

---

## Error Format

```json
{
  "error": "Crew not found",
  "code": "crew_not_found",
  "details": "Crew ID 'invalid-crew' does not exist in fleet registry"
}
```

**HTTP Status Codes:**
- `200 OK` — Success
- `201 Created` — Resource created
- `400 Bad Request` — Invalid input
- `404 Not Found` — Resource not found
- `500 Internal Server Error` — Server error

---

## Endpoints

### Fleet Management

#### `GET /api/fleet/crews`
List all crews in fleet.

**Response:** `200 OK`
```json
[
  {
    "id": "crew-001",
    "name": "Backend Squad",
    "risk_tier": "medium",
    "status": "active",
    "active_turns": 2
  }
]
```

---

#### `GET /api/fleet/crews/:id`
Get crew details by ID.

**Response:** `200 OK`
```json
{
  "id": "crew-001",
  "name": "Backend Squad",
  "risk_tier": "medium",
  "status": "active",
  "members": ["agent-01", "agent-02"],
  "policies": {
    "auto_approve": false,
    "max_concurrent_turns": 3
  }
}
```

**Errors:**
- `404` — Crew not found

---

#### `POST /api/fleet/crews`
Create new crew.

**Request:**
```json
{
  "name": "Frontend Squad",
  "risk_tier": "low",
  "members": ["agent-03"]
}
```

**Response:** `201 Created`
```json
{
  "id": "crew-002",
  "name": "Frontend Squad",
  "risk_tier": "low"
}
```

**Errors:**
- `400` — Invalid request (missing name, invalid risk_tier)

---

#### `PUT /api/fleet/crews/:id`
Update crew config.

**Request:**
```json
{
  "risk_tier": "high",
  "policies": {
    "auto_approve": true
  }
}
```

**Response:** `200 OK`
```json
{
  "id": "crew-001",
  "name": "Backend Squad",
  "risk_tier": "high"
}
```

---

#### `DELETE /api/fleet/crews/:id`
Delete crew (stops all active turns).

**Response:** `200 OK`
```json
{
  "deleted": true,
  "stopped_turns": 2
}
```

---

### Crew Operations

#### `POST /api/fleet/crews/:id/run`
Start crew turn execution.

**Request:**
```json
{
  "task": "Fix authentication bug in /login endpoint",
  "context": "User reported 500 error on login",
  "tools": ["filesystem", "shell", "http"]
}
```

**Response:** `200 OK`
```json
{
  "turn_id": "turn-12345",
  "status": "running",
  "started_at": "2026-10-03T23:30:00Z"
}
```

**Errors:**
- `404` — Crew not found
- `400` — Invalid task (empty, too long)
- `500` — Engine error (LLM API failed, tool dispatch failed)

---

#### `GET /api/fleet/crews/:id/history`
Get crew turn history.

**Query params:**
- `limit` (int, default 50) — Max turns to return
- `offset` (int, default 0) — Pagination offset

**Response:** `200 OK`
```json
{
  "turns": [
    {
      "turn_id": "turn-12345",
      "task": "Fix authentication bug",
      "status": "completed",
      "started_at": "2026-10-03T23:30:00Z",
      "completed_at": "2026-10-03T23:35:42Z",
      "tokens_used": 1842,
      "result": "Fixed bug in auth middleware, deployed to staging"
    }
  ],
  "total": 127,
  "limit": 50,
  "offset": 0
}
```

---

#### `GET /api/fleet/crews/:id/artifacts`
List artifacts produced by crew.

**Response:** `200 OK`
```json
[
  {
    "artifact_id": "art-001",
    "name": "API Design Doc",
    "type": "markdown",
    "created_at": "2026-10-03T22:15:00Z",
    "url": "/api/artifacts/art-001"
  }
]
```

---

### System Telemetry

#### `GET /api/system/metrics`
Get system metrics (latency, memory, active crews).

**Response:** `200 OK`
```json
{
  "gateway_latency_ms": 12,
  "memory_db_mb": 187.3,
  "active_crews": 4,
  "active_turns": 7,
  "uptime_seconds": 345612
}
```

---

#### `GET /api/system/health`
Health check endpoint (used by monitoring, load balancers).

**Response:** `200 OK`
```json
{
  "status": "healthy",
  "version": "0.3.0",
  "rust_gateway": "connected",
  "database": "ok",
  "vector_store": "ok"
}
```

**Errors:**
- `503 Service Unavailable` — Unhealthy (DB down, Rust gateway unreachable)

---

#### `GET /api/system/logs`
Stream system logs (SSE).

**Query params:**
- `level` (string, default "info") — Log level filter (debug, info, warn, error)
- `since` (string, ISO8601) — Start time

**Response:** `200 OK` (text/event-stream)
```
data: {"timestamp":"2026-10-03T23:30:15Z","level":"info","message":"Turn started","crew_id":"crew-001"}

data: {"timestamp":"2026-10-03T23:30:18Z","level":"debug","message":"Tool executed","tool":"filesystem"}
```

---

### [TEMP] Network Utils (ADR 0002 Violations)

#### `GET /api/network/qrcode`
Generate QR code (SHOULD BE in Go, currently in `web-2/server.ts`).

**Query params:**
- `data` (string) — Data to encode

**Response:** `200 OK`
```json
{
  "qrcode": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUg..."
}
```

**Status:** ADR 0002 — Move to Go endpoint + Nginx auto-start (next phase).

---

## Rate Limiting

**Current:** None (local-only)  
**Future:** 100 req/min per client IP (429 Too Many Requests on exceed)

---

## Pagination

List endpoints return:
```json
{
  "data": [...],
  "total": 127,
  "limit": 50,
  "offset": 0
}
```

**Query params:**
- `limit` (int, max 100)
- `offset` (int)

---

## WebSocket (Real-Time Streaming)

**NOT HTTP :9090** — WebSocket handled by Rust `clawcrew-gateway/ws.rs` on :50052.

Frontend connects:
```typescript
const ws = new WebSocket('ws://localhost:50052/ws/chat');
ws.onmessage = (event) => {
  const token = JSON.parse(event.data);
  console.log(token); // LLM token stream
};
```

**Why separate?** Rust handles long-lived connections (Go HTTP = short-lived request/response).

---

## Versioning

**Current:** No versioning (v0.x, breaking changes allowed)  
**Future (v1.0+):** `/api/v1/...` prefix, semantic versioning

---

## Related Documents

- [Architecture Overview](../architecture/00-overview.md) — 4-tier system design
- [Boundary Policy](../architecture/01-boundary-policy.md) — What HTTP API owns vs delegates
- [Go Conventions](../tech-stack/go-conventions.md) — HTTP handler patterns
- [gRPC System Gateway](./grpc-system-gateway.md) — Rust :50052 internal spec
