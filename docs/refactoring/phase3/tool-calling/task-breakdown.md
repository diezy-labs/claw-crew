# Claw-Crew Phase 3 — Tool Calling Platform: Task Breakdown & Workflows

> **Status:** Implementation Backlog & Workflows  
> **Parent Directory:** [`docs/refactoring/phase3/tool-calling/`](./)  

---

## 1. Phased Task Breakdown

### Phase T0: Runtime Foundation & Policy Boundary
- [x] Define canonical domain contracts in `engine/src/tool/interfaces.go` (`Tool`, `Registry`, `ApprovalGate`, `Service`, `PolicyEngine`).
- [x] Update and harden DTO models in `engine/src/tool/dto.go` (`RiskTier`, `RiskClass`, `ToolDefinition`, `ApprovalRequest`, `IdempotencyMode`).
- [x] Implement thread-safe in-memory `Registry` with namespace lookups and version matching.
- [x] Implement canonical argument normalizer and SHA-256 argument hasher.
- [x] Implement `PolicyEngine` evaluating capability grants and data classification.
- [x] Implement cryptographic `ApprovalGate` with one-time consume tokens and target file CAS hash validation.
- [x] Connect `engine/src/tool/wire.go` into `engine/app/wire.go`.

### Phase T1: Safe Read & Compute Tool Suite
- [x] Implement `workspace.list_files` with recursive depth limiting and `.gitignore` awareness.
- [x] Implement `workspace.read_file` with `ValidateSandboxPath` containment and byte limits.
- [x] Implement `workspace.search_code` using ripgrep or Go regex over workspace files.
- [x] Implement `web.fetch` with SSRF blocking (private IPs, loopback, cloud metadata IPs).
- [x] Implement `code.run_linter` using isolated child process and environment stripping.
- [x] Implement `code.run_tests` using process group timeouts and stdout/stderr artifact spillover.
- [x] Wire read-only tools to `engine/src/llm/dispatcher.go`.

### Phase T2: Controlled Workspace Mutation Suite
- [x] Implement `workspace.create_draft` writing strictly to `artifacts/drafts/`.
- [x] Implement `workspace.apply_patch` enforcing unified diff parsing, CAS file hash re-validation, and user approval.
- [x] Implement `git.create_branch` and `git.status` within sandboxed workspace.
- [x] Implement `git.commit` requiring explicit human-in-the-loop approval.
- [x] Add rollback snapshot creation prior to applying any code patch.

### Phase T3: Model Context Protocol (MCP) Client Integration
- [x] Implement MCP JSON-RPC 2.0 client supporting `stdio` and `SSE` transports.
- [x] Implement MCP tool discovery (`tools/list`) and dynamic conversion to `ToolDefinition`.
- [x] Wrap all MCP tool invocations in engine `PolicyEngine` and `ApprovalGate`.
- [x] Implement output sanitizer and redaction filter for MCP outputs.
- [x] Build workspace-level MCP server allowlist configuration.

### Phase T4: Tool Search & Agent-as-a-Tool Delegation
- [x] Implement dynamic tool catalog filtering based on task intent to conserve prompt tokens.
- [x] Implement subagent delegation as a tool call (`delegate_task`) with inherited execution context and policy ceilings.
- [x] Enforce child subagent cannot escalate permissions beyond parent crew permissions.

### Phase T5: Programmatic Sandbox & Advanced Workflows
- [x] Implement disposable Wasm/Docker sandboxed code execution runner.
- [x] Implement browser read-only screenshot/DOM extraction tool.
- [x] Implement browser write actions with mandatory per-click/per-submit human approval.

---

## 2. Real-World Example Workflows

### Workflow A: Codebase Audit Crew (Read-Only)
```text
User: "Audit the repository for security vulnerabilities in path validation."

1. Planner Agent -> workflow.create_plan
2. Code Explorer Agent -> workspace.list_files & workspace.search_code
3. Code Explorer Agent -> workspace.read_file("engine/src/tool/services.go")
4. Validation Agent -> code.run_tests("engine/src/tool/...")
5. Report Agent -> artifact.create_report
-> Final audit report generated with code references. Zero file mutations allowed.
```

### Workflow B: Safe Code Patch Workflow (Mutation with CAS)
```text
User: "Fix the symlink bypass in ValidateSandboxPath."

1. Code Agent -> workspace.read_file("engine/src/tool/services.go")
2. Code Agent -> computes diff and proposes workspace.apply_patch
3. Engine Policy -> detects RiskTierWrite -> pauses execution
4. User (in TUI/Tauri) -> reviews syntax-highlighted diff and target file hash -> clicks [Approve]
5. Engine Tool Service -> re-checks SHA-256 of services.go -> matches -> applies patch
6. Test Agent -> runs code.run_tests -> tests pass
7. Artifact Service -> stores diff and test log as run artifacts
```

### Workflow C: Research & Citation Workflow (Web with SSRF Guard)
```text
User: "Research recent Go 1.27 patch release notes and cite changes."

1. Research Agent -> web.search("Go 1.27 release notes")
2. Research Agent -> web.fetch("https://go.dev/doc/devel/release")
   (Engine SSRF guard verifies public IP, strips active scripts)
3. Evidence Collector -> saves raw HTML to artifact store
4. Synthesis Agent -> drafts summary with exact Markdown citations
5. Report Agent -> artifact.create_report
```

---

## 3. Definition of Done (DoD)

Phase 3 Tool Calling v1 is complete when:
1. **Engine Authority:** Go engine is the sole authority validating and executing tools.
2. **Path Containment:** All file tools pass `ValidateSandboxPath`; directory traversal and symlink escape tests pass.
3. **Approval Security:** Side-effecting tools (`WRITE`/`EXECUTE`) require one-time, CAS-bound user approvals that cannot be replayed or altered.
4. **SSRF Guard:** Web fetching tools block private IP addresses, loopback, and cloud metadata endpoints.
5. **Secret Redaction:** No raw API keys or secrets appear in execution summaries, logs, or LLM context.
6. **Cancellation Guarantee:** Context cancellation immediately terminates child processes and HTTP streams.
7. **Race Clean:** `go test -race ./src/tool/...` passes with zero data races.
8. **UI Transparency:** TUI, Tauri, and Web render active tool states and interactive approval dialogs via SSE streams.
