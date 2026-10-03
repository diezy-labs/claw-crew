# Principle Review Focus — Phase 3 (Track A + B)

**Last Updated:** 2026-10-03  
**Phase 3 Scope:** Workspace Routing (Track A) + 5 API Handlers (Track B)  
**Lead Briefing Reference:** Phase 3 Backend execution completed 2026-10-02 (awaiting Achmad merge decision).

---

## Phase 3 Technical Summary

### Track A: Workspace Routing (Rust Backend)
- **Proto Changes:** `ToolCallRequest` + workspace_path (field 3) + timeout_seconds (field 4); `ToolCallResponse` + exit_code (field 4)
- **Rust Implementation:** `grpc_system_gateway.rs` now validates workspace per-request via `validate_path_with_workspace()`, enforces timeout (1-300s, default 60s), applies timeout to ALL tool execution paths (bash/git/read_file/write_file)
- **Security:** Path canonicalization prevents traversal; timeout prevents hangs; per-request workspace allows caller to override default root

### Track B: 5 API Handlers (TypeScript Frontend)
1. **GET `/api/health`** — System health probe (status, uptime, mode)
2. **GET `/api/system/metrics`** — Host CPU/RAM/processes telemetry
3. **GET `/api/engine/processes`** — Local process monitor stub (4 processes: agent-engine, mesh-discovery-mdns, tauri-core-guard, sqlite-fts5-worker)
4. **POST `/api/engine/execute`** — Shell execution fallback (GATED: workspace validation, 30s timeout, array args, error shape safe)
5. **GET `/api/providers/ollama/*`** — Ollama bridge (status + generate routes; probes host daemon, not Go engine)

---

## Security Gates to Review

When reviewing Phase 3 PRs, use this checklist in priority order:

### 1. **Path Canonicalization + Containment (Rust)**
- [ ] **Check:** `validate_path_with_workspace()` is used for ALL file-touching operations (read_file_with_workspace, write_file_with_workspace)
- [ ] **Check:** Both `workspace_root` parameter AND `path` are canonicalized before containment check (prevents symlink escapes)
- [ ] **Check:** Non-existent paths are handled safely (walk up to existing ancestor, canonicalize, re-attach suffix)
- [ ] **Check:** Path traversal attempt (`../../../etc/passwd`) is rejected
- **File:** `crates/clawcrew-gateway/src/grpc_system_gateway.rs` (lines 40–65, test at line 370)

### 2. **Per-Request Workspace Validation (Rust)**
- [ ] **Check:** If `req.workspace_path` is set, it is validated via `validate_path_with_workspace()` against the default workspace_root FIRST (prevents escape from allowed roots)
- [ ] **Check:** On validation failure, error is returned with clear message (not silent fallback)
- [ ] **Check:** The validated workspace becomes the execution context (passed to all tool functions)
- **File:** `crates/clawcrew-gateway/src/grpc_system_gateway.rs` (lines 203–216)

### 3. **Timeout Enforcement (Rust)**
- [ ] **Check:** All tool execution paths (bash, git, read_file, write_file) are wrapped in `tokio::time::timeout(timeout_duration, ...)`
- [ ] **Check:** timeout_seconds request field is clamped: `max(1).min(300)` (1–300s, no 0 or unbounded)
- [ ] **Check:** Default is 60s (not user-controllable to 0 or infinity)
- [ ] **Check:** Timeout error case returns non-null error string (e.g., "execution timed out after 60 seconds")
- **File:** `crates/clawcrew-gateway/src/grpc_system_gateway.rs` (lines 223–226)

### 4. **TypeScript Handler Safety (Node.js)**
- [ ] **Check:** `spawn()` uses array args `[shell, [arg1, arg2, ...]]` NOT string concatenation (`shell + args` is injection vector)
- [ ] **Check:** `cwd` is set to `workspace_path` if provided, else `process.cwd()` (isolates execution context)
- [ ] **Check:** `timeout` option is set on `spawn()` (30s for this handler, matches proto intent)
- [ ] **Check:** workspace_path is validated before use: rejects `..` and `/` (absolute paths disallowed)
- [ ] **Check:** Error responses include `workspace` field in JSON (for audit/debugging, NOT a security gate)
- **File:** `web-2/server.ts` (lines 180–226)

### 5. **Proto Field Alignment**
- [ ] **Check:** `ToolCallRequest.workspace_path` is field 3 (not 10/11/12 — collision check passed)
- [ ] **Check:** `ToolCallRequest.timeout_seconds` is field 4 (int32, optional)
- [ ] **Check:** `ToolCallResponse.exit_code` is field 4 (int32, optional)
- [ ] **Check:** All three fields are populated correctly in response messages (not left as null when they have values)
- **File:** `proto/agent_service.proto` (lines 89–101)

### 6. **No Secrets in Code/Logs**
- [ ] **Check:** No hardcoded API keys, tokens, or passwords in workspace_path handling
- [ ] **Check:** Error messages do NOT echo full paths, command args, or sensitive details (sanitize before log)
- [ ] **Check:** workspace_path parameter is NOT logged verbatim; if logged at all, it is canonical path only

### 7. **RCE Vector Closure** (Critical)
- [ ] **Check:** `/api/engine/execute` is NOT spawning shell with raw request body (old vulnerability)
- [ ] **Check:** Command IS validated (non-empty) before spawn
- [ ] **Check:** `spawn()` receives args as array, not string (defeats shell injection)
- [ ] **Note:** OLD `server.ts:165` RCE vector (raw powershell -Command) is FIXED in this commit

---

## Code Review Focus (File Order)

### Phase 3 PR Review Sequence

1. **Read Proto First** → Verify field numbers & types
   - File: `proto/agent_service.proto`
   - Time: 5 min

2. **Review Rust Handler** → Verify workspace validation & timeout
   - File: `crates/clawcrew-gateway/src/grpc_system_gateway.rs`
   - Sections: `validate_path_with_workspace()` (40–65), tool dispatch (200–260), tests (370+)
   - Time: 15 min

3. **Review TypeScript Handler** → Verify spawn array args & workspace gating
   - File: `web-2/server.ts`
   - Sections: POST `/api/engine/execute` (180–226)
   - Time: 10 min

4. **Verify CI Pipeline** → Check build + security lint pass
   - File: `.github/workflows/*.yml` (check if Phase 3 build passes)
   - Time: 5 min

### Total Review Time: ~35 min

---

## Common Issues to Flag

### APPROVE if ALL gates pass:
- Workspace validation on every path-touching operation ✓
- Timeout applied to all async operations ✓
- spawn() uses array args, not string ✓
- Proto field numbers don't collide ✓
- No secrets in code/logs ✓
- CI green (build + lint + tests) ✓

### CHANGES-REQUESTED if ANY gates fail:
- Path traversal not blocked (e.g., `../../../etc/passwd` escapes root)
- Workspace override not validated before use
- Timeout not clamped or not applied to a tool path
- spawn() uses string args (shell injection risk)
- Proto field collision with existing fields
- Hardcoded secrets or raw command echo in logs

### FLAKY CI (Mark Explicitly):
- If a CI step fails intermittently (network, timeouts, lock contention), note it as flaky
- Do NOT merge on a flaky red; request re-run or code fix depending on root cause

---

## Checklist Template (Copy for Each PR)

```
## Verdict: [ ] APPROVE | [ ] CHANGES-REQUESTED | [ ] NEEDS-CLARIFICATION

## Proto Validation
- [ ] workspace_path=3 on ToolCallRequest
- [ ] timeout_seconds=4 on ToolCallRequest
- [ ] exit_code=4 on ToolCallResponse
- [ ] Field numbers do not collide

## Rust Workspace Validation
- [ ] validate_path_with_workspace() called for read_file, write_file, bash, git
- [ ] Path canonicalization present before containment check
- [ ] Per-request workspace validated before use
- [ ] Non-existent path handling safe (ancestor walk + re-attach)

## Rust Timeout
- [ ] tokio::time::timeout() wraps all tool paths
- [ ] timeout_seconds clamped to 1-300s
- [ ] Default is 60s
- [ ] Timeout error returns clear message

## TypeScript spawn() Safety
- [ ] spawn(shell, args_ARRAY, options) — NOT string concatenation
- [ ] workspace_path validated (reject .. and /)
- [ ] cwd set to workspace_path or process.cwd()
- [ ] timeout: 30000 set on spawn options

## Security
- [ ] No hardcoded secrets
- [ ] Error messages sanitized (no raw paths/args)
- [ ] Old RCE vector closed (/api/engine/execute array args)

## CI Pipeline
- [ ] Build green
- [ ] Lint/fmt clean
- [ ] Tests pass (or mark flaky)

## Decision
- Merge to: [ ] testing | [ ] prerelease | [ ] main (if Lead-approved)
- Next: [ ] Await Lead final approval | [ ] Route to QA testing
```

---

## Lead Handoff Notes

- **Phase 3 Backend Implementation:** Complete as of 2026-10-02 (commit 125bc5b5)
- **Status:** Awaiting Achmad merge decision (code in feat/ai-studio-sync, not yet on main)
- **Principle Review Ready:** Yes — all gates documented above
- **QA Next:** After merge, route to QA for integration testing (workspace override behavior, timeout edge cases)
- **One Known Deferred Item:** Ships collection allowlist integration deferred (ponytail comment in server.ts:189)

---

## References

- **Learned Lesson:** galleon-fleet web-2/server.ts old RCE at 165–195 (raw powershell -Command) was mistakenly assumed 'gated' but was wide-open. Phase 3 fixes it via array args + workspace validation. This MUST be confirmed in code review (not silent merge).
- **Ponytail Principle:** Minimal workspace check in TS (basic .. + / rejection); full Ships collection allowlist integration deferred by design (not a blocker for Phase 3).

