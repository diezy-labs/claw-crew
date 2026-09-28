# Claw-Crew Phase 3 — Tool Calling Platform: API & Interaction Specification

> **Status:** Proposed API Specification  
> **Target Endpoints:** `/api/v1/tools`, `/api/v1/approvals`, `/api/v1/runs/{id}/tool-requests`  
> **Parent Directory:** [`docs/refactoring/phase3/tool-calling/`](./)  

---

## 1. REST API Specification

### 1.1 List Available Tools
Retrieves all registered native and MCP tools filtered by the caller's workspace permissions and assigned agent capabilities.

- **Method:** `GET`
- **Path:** `/api/v1/tools`
- **Query Parameters:**
  - `workspace_id` (string, required): Active workspace context.
  - `agent_id` (string, optional): Specific agent filter.
  - `risk_tier` (string, optional): Filter by `READ`, `WRITE`, `EXECUTE`.

#### Response `200 OK`
```json
{
  "tools": [
    {
      "id": "workspace.read_file",
      "version": "1.0.0",
      "display_name": "Read Workspace File",
      "description": "Reads contents of a file inside the authorized workspace boundary.",
      "risk_tier": "READ",
      "risk_class": "read",
      "requires_approval": false,
      "capabilities": ["workspace.read"],
      "input_schema": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "path": {
            "type": "string",
            "description": "Relative path within the workspace root."
          },
          "max_bytes": {
            "type": "integer",
            "default": 50000,
            "maximum": 200000
          }
        },
        "required": ["path"]
      }
    },
    {
      "id": "workspace.apply_patch",
      "version": "1.0.0",
      "display_name": "Apply Workspace Patch",
      "description": "Applies a unified diff patch to a target file with CAS hash check.",
      "risk_tier": "WRITE",
      "risk_class": "write_workspace",
      "requires_approval": true,
      "capabilities": ["workspace.write"],
      "input_schema": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "path": { "type": "string" },
          "patch": { "type": "string" },
          "expected_file_hash": { "type": "string" },
          "reason": { "type": "string" }
        },
        "required": ["path", "patch", "expected_file_hash", "reason"]
      }
    }
  ]
}
```

---

### 1.2 Invoke Tool Request
Explicitly requests invocation of a tool within a run and task context.

- **Method:** `POST`
- **Path:** `/api/v1/runs/{run_id}/tool-requests`
- **Headers:**
  - `Idempotency-Key` (string, required): Unique client key to prevent duplicate execution.

#### Request Body
```json
{
  "task_id": "task_01JM59P8QR2N",
  "tool_name": "workspace.apply_patch",
  "arguments": "{\"path\":\"engine/src/tool/services.go\",\"patch\":\"@@ -50,6 +50,12 @@...\",\"expected_file_hash\":\"sha256:4a8b...\",\"reason\":\"Add CAS validation\"}"
}
```

#### Response `202 Accepted` (Requires Approval)
```json
{
  "execution_id": "exec_01JM59X2KV7T",
  "status": "pending",
  "requires_approval": true,
  "approval_request": {
    "approval_id": "appr_01JM59Y9NZ4D",
    "tool_name": "workspace.apply_patch",
    "risk_tier": "WRITE",
    "summary": "Apply unified diff patch to engine/src/tool/services.go",
    "preview_diff": "--- engine/src/tool/services.go\n+++ engine/src/tool/services.go\n@@ ...",
    "target_resource": {
      "type": "file",
      "target": "engine/src/tool/services.go",
      "expected_hash": "sha256:4a8b..."
    },
    "expires_at": "2026-09-28T10:15:00Z"
  }
}
```

#### Response `200 OK` (Auto-Approved Read Tool)
```json
{
  "execution_id": "exec_01JM59R4WX1A",
  "status": "approved",
  "output": "package tool\n\nimport (\n...",
  "artifact_ids": []
}
```

---

### 1.3 Resolve Approval Gate (Approve / Deny)
Allows human-in-the-loop operators in TUI, Tauri, or Web to grant or deny execution.

- **Method:** `POST`
- **Path:** `/api/v1/approvals/{approval_id}/resolve`

#### Request Body
```json
{
  "approved": true,
  "reason": "Verified diff addresses BUG-005 without regressions"
}
```

#### Response `200 OK`
```json
{
  "approval_id": "appr_01JM59Y9NZ4D",
  "status": "approved",
  "resolved_at": "2026-09-28T10:05:22Z",
  "execution_id": "exec_01JM59X2KV7T"
}
```

---

### 1.4 Error Envelopes (`engine/core/errors`)
All failures adhere strictly to the standardized Clean Architecture error envelope:

```json
{
  "error": {
    "code": "PERMISSION_DENIED",
    "message": "path traversal blocked: ../../secrets.env is outside workspace root",
    "layer": "SERVICE",
    "request_id": "req_01JM5A2K9Z8F",
    "retryable": false
  }
}
```

---

## 2. Server-Sent Events (SSE) Stream

Clients maintain a single event stream connection to receive live run progress and tool execution lifecycles:

- **Path:** `GET /api/v1/runs/{run_id}/events`
- **Header:** `Accept: text/event-stream`

### Event Sequence: Tool Calling Lifecycle

```text
event: tool.requested
data: {"execution_id":"exec_01","tool_name":"code.run_tests","risk_tier":"EXECUTE","created_at":"2026-09-28T10:00:00Z"}

event: tool.approval_required
data: {"execution_id":"exec_01","approval_id":"appr_01","summary":"Run unit test suite in sandboxed process","expires_at":"2026-09-28T10:10:00Z"}

event: tool.approved
data: {"execution_id":"exec_01","approval_id":"appr_01","resolved_by":"user_desktop","resolved_at":"2026-09-28T10:01:15Z"}

event: tool.started
data: {"execution_id":"exec_01","tool_name":"code.run_tests","started_at":"2026-09-28T10:01:16Z"}

event: tool.completed
data: {"execution_id":"exec_01","tool_name":"code.run_tests","summary":"38 passed, 0 failed","artifact_ids":["art_test_log_01"],"duration_ms":3420}
```

---

## 3. Client UI/UX Interaction Standards

### 3.1 Activity Timeline
Clients display tool calls in an interactive timeline reflecting state changes in real time:
- **Pending Approval:** Visual badge (yellow pulse), action summary, and quick-action buttons `[Approve (Enter)]` / `[Deny (Esc)]`.
- **Executing:** Subtle spinner displaying running duration.
- **Completed:** Green badge showing execution duration, structured output summary, and clickable links to output artifacts.
- **Denied / Failed:** Red alert box showing the typed error code and reason.

### 3.2 Human-in-the-Loop Approval Modal
The approval dialog must never ask vague questions like *"Do you want to proceed?"*. It must present:
1. **Target Resource:** Explicit file path or remote URI.
2. **Action Summary:** One-sentence explanation generated by the agent.
3. **Diff View:** Syntax-highlighted unified diff for code modifications.
4. **Target Hash Status:** Indicator that the current target matches `expected_file_hash`.
5. **Countdown Timer:** Time remaining before automatic expiration.
