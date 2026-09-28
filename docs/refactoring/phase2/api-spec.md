# API Specification — ClawCrew Engine REST, SSE & IPC (Phase 2)

> **Branch Context**: `feat/enhance-agent-phase2`  
> **Status**: Living API Specification  
> **Related Documents**: [README](./README.md) | [PRD](./prd.md) | [Technical Specification](./tech-spec.md) | [Task Breakdown](./task-breakdown.md)

---

## 1. API Conventions & Standards

### 1.1 Base Paths & Protocols
- **Base Path HTTP**: `/api/v1`
- **Port Default HTTP & Metrics**: `:9090` (dapat dikonfigurasi via CLI flag atau env)
- **Port Default gRPC IPC**: `:50051`
- **Format Payload**: `application/json` (menggunakan parser berkecepatan tinggi `encoding/json/v2`)
- **Event Streaming**: Server-Sent Events (`text/event-stream`)

### 1.2 Headers Standar
- `X-Request-ID`: String identifier unik untuk korelasi penelusuran log di setiap request (contoh: `req_01h...`).
- `Idempotency-Key`: Header opsional pada operasi mutasi (POST) untuk mencegah eksekusi berulang.
- `Last-Event-ID`: Digunakan oleh klien saat reconnecting ke stream SSE untuk memulihkan event yang terlewat.

### 1.3 Standard Error Envelope
Seluruh kegagalan request mengembalikan format JSON standar dengan status HTTP yang sesuai:

```json
{
  "error": {
    "code": "RUN_NOT_FOUND",
    "message": "The requested run does not exist or has expired.",
    "layer": "SERVICE",
    "request_id": "req_01h874xkm9",
    "details": {
      "run_id": "run_01h874xkm892"
    }
  }
}
```

Daftar error codes standar (`core/errors`):
- `INVALID_ARGUMENT` (HTTP 400)
- `NOT_FOUND` (HTTP 404)
- `ALREADY_EXISTS` (HTTP 409)
- `PERMISSION_DENIED` (HTTP 403)
- `FAILED_PRECONDITION` (HTTP 412)
- `INTERNAL` (HTTP 500)
- `UNAVAILABLE` (HTTP 503)

---

## 2. Crew Endpoints

### 2.1 List Available Crews
Mendapatkan daftar kru yang tersedia di engine.

```http
GET /api/v1/crews
```

#### Response (200 OK)
```json
{
  "items": [
    {
      "id": "crew_research_dev",
      "name": "Research & Engineering Crew",
      "description": "Multi-agent crew for code analysis, planning, implementation, and review.",
      "agent_count": 3,
      "agents": [
        {
          "id": "planner",
          "name": "Planner Agent",
          "role": "Decompose high-level tasks into DAG task graph.",
          "capabilities": ["planning", "task_breakdown"]
        },
        {
          "id": "coder",
          "name": "Code Specialist",
          "role": "Implements code, refactors, and edits files.",
          "capabilities": ["write_file", "edit_file", "read_file"]
        },
        {
          "id": "reviewer",
          "name": "Code Reviewer",
          "role": "Validates code changes, generates git diffs, runs test suites.",
          "capabilities": ["run_tests", "generate_diff"]
        }
      ]
    }
  ]
}
```

### 2.2 Get Crew Details
```http
GET /api/v1/crews/{crew_id}
```

### 2.3 Create or Update Crew
```http
PUT /api/v1/crews/{crew_id}
Content-Type: application/json
```

#### Request Body
```json
{
  "name": "Custom Audit Crew",
  "description": "Security and performance review crew.",
  "agents": [
    {
      "id": "auditor",
      "name": "Security Auditor",
      "role": "Analyze security vulnerabilities and safe tool execution.",
      "capabilities": ["static_analysis", "read_file"]
    }
  ]
}
```

---

## 3. Run Endpoints

### 3.1 Start a New Run
Memulai proses eksekusi tugas baru untuk suatu kru.

```http
POST /api/v1/runs
Idempotency-Key: 8a15b1c9-7623-4d7a-b5e2-2a74c2079018
Content-Type: application/json
```

#### Request Body
```json
{
  "crew_id": "crew_research_dev",
  "workflow_id": "workflow_code_refactoring",
  "input": {
    "prompt": "Refactor engine/src/crew to support DAG task scheduling and event streams.",
    "target_files": ["engine/src/crew/services.go"]
  },
  "workspace": {
    "root_uri": "file:///c:/Users/Yoga%206/Documents/Github/diezy-labs/claw-crew"
  },
  "options": {
    "stream": true,
    "require_tool_approval": true
  }
}
```

#### Response (202 Accepted)
```json
{
  "id": "run_01h87b92mkq1",
  "crew_id": "crew_research_dev",
  "status": "queued",
  "events_url": "/api/v1/runs/run_01h87b92mkq1/events",
  "created_at": "2026-09-28T05:20:00Z"
}
```

### 3.2 Get Run Status & Details
```http
GET /api/v1/runs/{run_id}
```

#### Response (200 OK)
```json
{
  "id": "run_01h87b92mkq1",
  "crew_id": "crew_research_dev",
  "status": "running",
  "started_at": "2026-09-28T05:20:01Z",
  "tasks_summary": {
    "total": 4,
    "completed": 2,
    "running": 1,
    "pending": 1
  },
  "artifacts_count": 1
}
```

### 3.3 Cancel Run
Membatalkan seluruh proses eksekusi yang sedang berjalan secara kooperatif.

```http
POST /api/v1/runs/{run_id}/cancel
Content-Type: application/json
```

#### Request Body (Opsional)
```json
{
  "reason": "Cancelled by user via TUI shortcut (Ctrl+C)"
}
```

#### Response (200 OK)
```json
{
  "id": "run_01h87b92mkq1",
  "status": "cancelling",
  "message": "Cancellation signal sent to all active agent goroutines."
}
```

---

## 4. Server-Sent Events (SSE) Stream

### 4.1 Subscription Endpoint
Membuka koneksi HTTP streaming untuk menerima event progres real-time.

```http
GET /api/v1/runs/{run_id}/events
Accept: text/event-stream
Last-Event-ID: evt_01h87b93z9k0
```

### 4.2 Format Event Data
Setiap pesan mematuhi standar SSE dengan format:
- `id`: Unique event ID
- `event`: Event type
- `data`: JSON string

#### Contoh Aliran Event:

```text
id: evt_001
event: run.status_changed
data: {"event_id":"evt_001","sequence":1,"run_id":"run_01h87b92mkq1","status":"planning","timestamp":"2026-09-28T05:20:01Z"}

id: evt_002
event: task.created
data: {"event_id":"evt_002","sequence":2,"run_id":"run_01h87b92mkq1","task_id":"task_01","title":"Analyze codebase structure","assigned_agent_id":"planner","status":"running","timestamp":"2026-09-28T05:20:02Z"}

id: evt_003
event: agent.thought_summary
data: {"event_id":"evt_003","sequence":3,"run_id":"run_01h87b92mkq1","agent_id":"planner","summary":"Identified 3 source files requiring refactoring. Generating task DAG.","timestamp":"2026-09-28T05:20:04Z"}

id: evt_004
event: tool.approval_required
data: {"event_id":"evt_004","sequence":4,"run_id":"run_01h87b92mkq1","tool_execution_id":"tool_exec_01","tool_name":"write_file","risk_tier":"write","summary":"Modify engine/src/crew/services.go to inject TaskScheduler","expires_at":"2026-09-28T05:25:00Z","timestamp":"2026-09-28T05:20:05Z"}

id: evt_005
event: artifact.created
data: {"event_id":"evt_005","sequence":5,"run_id":"run_01h87b92mkq1","artifact_id":"art_01h87b","name":"services.go.patch","kind":"git_diff","summary":"Added TaskScheduler injection and event broadcaster integration","timestamp":"2026-09-28T05:20:10Z"}

id: evt_006
event: run.completed
data: {"event_id":"evt_006","sequence":6,"run_id":"run_01h87b92mkq1","status":"completed","duration_ms":9500,"timestamp":"2026-09-28T05:20:10Z"}
```

---

## 5. Task Endpoints

### 5.1 List Tasks for Run
```http
GET /api/v1/runs/{run_id}/tasks
```

#### Response (200 OK)
```json
{
  "tasks": [
    {
      "id": "task_01",
      "run_id": "run_01h87b92mkq1",
      "assigned_agent_id": "planner",
      "title": "Analyze architecture",
      "status": "completed",
      "dependencies": []
    },
    {
      "id": "task_02",
      "run_id": "run_01h87b92mkq1",
      "assigned_agent_id": "coder",
      "title": "Implement task scheduler",
      "status": "running",
      "dependencies": ["task_01"]
    }
  ]
}
```

### 5.2 Retry Failed Task
```http
POST /api/v1/runs/{run_id}/tasks/{task_id}/retry
```

---

## 6. Tool Execution & Approval Endpoints

### 6.1 List Tool Executions
```http
GET /api/v1/runs/{run_id}/tool-executions
```

### 6.2 Approve Tool Execution
Memberikan izin pada tool berisiko untuk segera dijalankan.

```http
POST /api/v1/runs/{run_id}/tool-executions/{tool_execution_id}/approve
Content-Type: application/json
```

#### Request Body (Opsional)
```json
{
  "approved_by": "user_local_terminal"
}
```

#### Response (200 OK)
```json
{
  "tool_execution_id": "tool_exec_01",
  "status": "executing",
  "message": "Tool execution approved and dispatched to OS gateway."
}
```

### 6.3 Deny Tool Execution
Menolak izin eksekusi tool.

```http
POST /api/v1/runs/{run_id}/tool-executions/{tool_execution_id}/deny
Content-Type: application/json
```

#### Request Body
```json
{
  "reason": "Forbidden file path modification."
}
```

#### Response (200 OK)
```json
{
  "tool_execution_id": "tool_exec_01",
  "status": "denied",
  "message": "Tool execution was denied by user."
}
```

---

## 7. Artifact Endpoints

### 7.1 List Artifacts for Run
```http
GET /api/v1/runs/{run_id}/artifacts
```

#### Response (200 OK)
```json
{
  "artifacts": [
    {
      "id": "art_01h87b",
      "run_id": "run_01h87b92mkq1",
      "kind": "git_diff",
      "name": "services.go.patch",
      "media_type": "text/x-diff",
      "byte_size": 2415,
      "summary": "Added TaskScheduler injection and event broadcaster integration",
      "content_url": "/api/v1/artifacts/art_01h87b/content"
    }
  ]
}
```

### 7.2 Get Artifact Content
```http
GET /api/v1/artifacts/{artifact_id}/content
```

#### Response (200 OK)
Mengembalikan data biner atau teks mentah (misal: raw patch/diff).

---

## 8. Workflow & SOP Endpoints

### 8.1 List Workflows / SOPs
```http
GET /api/v1/workflows
```

### 8.2 Instantiate Workflow into a Run
```http
POST /api/v1/workflows/{workflow_id}/instantiate
Content-Type: application/json
```

#### Request Body
```json
{
  "parameters": {
    "target_directory": "engine/src/crew",
    "mode": "exhaustive_audit"
  }
}
```

---

## 9. Health, Readiness & Observability

### 9.1 Liveness Probe
```http
GET /api/v1/health
```
#### Response (200 OK)
```json
{ "status": "UP", "version": "v0.2.0-phase2" }
```

### 9.2 Readiness Probe
```http
GET /api/v1/ready
```
#### Response (200 OK)
```json
{
  "status": "READY",
  "dependencies": {
    "llm_provider": "OK",
    "rust_gateway_ipc": "OK",
    "vector_store": "OK"
  }
}
```

### 9.3 Diagnostics Endpoint
```http
GET /api/v1/diagnostics
```

### 9.4 Prometheus Metrics
```http
GET /metrics
```
Mengembalikan counter dan histogram Prometheus standar (`agent_active_goroutines`, `llm_token_usage_total`, `agent_turn_duration_seconds`, dll).
