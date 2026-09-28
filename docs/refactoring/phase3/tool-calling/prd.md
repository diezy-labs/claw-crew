# Claw-Crew Phase 3 — Tool Calling Platform: Product Requirements Document (PRD)

> **Status:** Approved Product Requirements  
> **Parent Directory:** [`docs/refactoring/phase3/tool-calling/`](./)  
> **Target Version:** Claw-Crew Phase 3 (Engine v0.9.0+)  

---

## 1. Problem Statement

### 1.1 The Need for Controlled Agency
Modern autonomous multi-agent systems must move beyond conversational outputs to perform substantive, verifiable engineering and research tasks in a real workspace. These tasks include:
- Inspecting source code, calculating AST differences, and identifying syntax/semantic errors.
- Running unit test suites, linters, and vulnerability scanners.
- Fetching web documentation, verifying API specifications, and citing primary sources.
- Drafting documentation, technical specifications, and pull request descriptions.
- Applying targeted code modifications with explicit user authorization.

### 1.2 The Risk of Unconstrained Execution
If tool calling is implemented naively (e.g., exposing an unconstrained bash runner or arbitrary file writer), systems suffer from:
1. **Host Compromise:** Untrusted or hijacked model prompts can execute destructive system commands (`rm -rf`, curl exfiltration).
2. **Path Traversal:** Accessing files outside the permitted workspace directory via `../` sequences or symlink spoofing.
3. **Prompt Injection Egress:** Hostile third-party web content instructing the LLM to read local secrets (`.env`, `id_rsa`) and transmit them to external servers.
4. **Non-Idempotent Duplicate Actions:** Unsafe automatic retries causing duplicated commits, multiple PRs, or multiple external webhook calls.
5. **Context Window Saturation:** Dumping multi-megabyte log files or build artifacts directly into the prompt context, incurring excessive token costs and degrading model reasoning.

---

## 2. Tool Calling vs. RAG (Retrieval-Augmented Generation)

Tool calling and RAG are complementary, distinct pillars in the Claw-Crew engine architecture:

| Dimension | Retrieval-Augmented Generation (RAG) | Tool Calling Platform |
|---|---|---|
| **Primary Question** | *"What knowledge or evidence is relevant to this task?"* | *"What real-world action or inspection must occur now?"* |
| **Typical Operation** | Semantic vector search, BM25 keyword matching, passage re-ranking. | Executing a linter, reading a specific file path, applying a patch, fetching a URL. |
| **Execution Safety** | Completely read-only and static; minimal system side effects. | Varies from safe read-only compute to mutating file writes and external API actions. |
| **Context Management** | Embeds chunked passages into system/user prompts. | Returns structured JSON data, summary excerpts, and persistent artifact pointers. |
| **Engine Integration** | Handled by `engine/src/memory/`. | Handled by `engine/src/tool/` and `engine/src/llm/`. |

---

## 3. Tool Calling Maturity Model

Claw-Crew categorizes agentic capabilities into 9 distinct maturity tiers:

| Tier | Maturity Name | Description | Claw-Crew Phase 3 Status |
|---|---|---|---|
| **L0** | Text-Only Responses | Model generates natural language without external awareness. | Deprecated / Insufficient. |
| **L1** | Structured Function Calling | Model emits validated JSON schema calls; engine executes safe compute. | **Baseline Core.** |
| **L2** | Policy-Aware Tool Invocation | Dynamic risk tiering, workspace scoping, and capability resolution. | **Mandatory Standard.** |
| **L3** | Human-in-the-Loop Approvals | Write and external operations pause for cryptographic hash-bound user consent. | **Mandatory Standard.** |
| **L4** | Model Context Protocol (MCP) | Seamless interoperability with standard external tool and resource servers. | **Core Phase 3 Deliverable.** |
| **L5** | Dynamic Tool Discovery | Lazy loading and semantic search over large catalogs to conserve prompt tokens. | **Phase 3 Extension.** |
| **L6** | Agent-as-a-Tool Delegation | Invoking specialized subagents through tool interfaces with inherited policy. | **Phase 3 Extension.** |
| **L7** | Programmatic Tool Execution | Generating and executing safe, disposable Python/Wasm scripts in isolated sandboxes. | Future Roadmap (Phase 4). |
| **L8** | Full OS / Browser Computer Use | Visual GUI automation and browser click-stream interactions. | Future Roadmap (Phase 4). |

---

## 4. Functional Requirements (FR)

### FR-01: Centralized Tool Registry
- The engine must provide a thread-safe registry (`Registry`) registering both native Go tools and dynamic MCP tools.
- Each tool definition must declare:
  - Unique namespaced `ID` (e.g. `workspace.read_file`, `web.fetch`).
  - Semantic `Version` (e.g. `1.0.0`).
  - Human-readable `DisplayName` and detailed `Description`.
  - JSON Schema-compliant `InputSchema` and `OutputSchema`.
  - `RiskTier` (`READ`, `WRITE`, `EXECUTE`).
  - Required `Capabilities` (e.g. `workspace.read`, `network.egress`).
  - Default `Timeout` and `MaxOutputBytes`.
  - Declared `IdempotencyMode` (`safe`, `idempotent_key_required`, `compare_and_swap`, `at_most_once`).

### FR-02: Strict Schema Validation & Canonical Argument Hashing
- Tool invocations must be rejected if arguments do not strictly validate against the registered JSON Schema.
- Arguments must be normalized into a canonical JSON representation (sorted keys, consistent whitespace) and hashed via SHA-256 to ensure cryptographic binding for approvals.

### FR-03: Server-Side Policy Evaluation
- Every tool request must be evaluated by the Go engine policy engine before execution.
- Evaluates against: Workspace permissions, Agent capability grants, Data classification level, and Tool risk tier.
- Outcomes must strictly resolve to:
  - `ALLOW`: Immediately scheduled for execution.
  - `REQUIRE_APPROVAL`: Pauses execution, persists approval request, and notifies client shells.
  - `DENY`: Rejects request with typed domain error (`appErrors.CodePermissionDenied`).

### FR-04: Cryptographic Approval Binding
- User approvals must bind directly to:
  - `ToolID` and `Version`.
  - Canonical `ArgumentsHash`.
  - Resolved physical target(s) and their current hash (e.g. SHA-256 of target file before patching).
  - Scope identity (`WorkspaceID`, `RunID`, `TaskID`).
  - Absolute expiration timestamp.
- Any modification of arguments or target file state between approval and execution must invalidate the approval token and abort execution.

### FR-05: Comprehensive Execution Scoping
- Every invocation must pass an immutable `ExecutionContext` including:
  `ActorID`, `WorkspaceID`, `CrewID`, `AgentID`, `RunID`, `TaskID`, `RequestID`, `AllowedRoots`, and `DataClassification`.

### FR-06: Artifact Management & Token Protection
- Tool outputs exceeding `MaxOutputBytes` (default 50 KB) must be automatically offloaded to persistent disk storage in `engine/src/artifact/`.
- The LLM receives a sanitized, structured summary and an `ArtifactID` pointer, preventing prompt context bloat.

### FR-07: Context Propagation & Structured Cancellation
- Tool execution must strictly inherit the parent `context.Context` of the task/run.
- When a user cancels a run or a deadline expires, all underlying child processes, HTTP connections, and MCP sessions must terminate immediately.

### FR-08: Complete Auditability & Event Streaming
- Every state transition (`requested`, `validated`, `policy_decided`, `approval_required`, `approved`, `started`, `completed`, `failed`) must emit a typed SSE event and be recorded in persistent audit logs.

### FR-09: Model Context Protocol (MCP) Client
- The engine must act as an MCP host, connecting to local (stdio) and remote (SSE/HTTP) MCP servers.
- Discovered MCP tools must be wrapped in native `ToolDefinition` adapters and subjected to the identical policy and approval gates as native tools.

### FR-10: Transparent Human-in-the-Loop UI/UX
- Client interfaces (TUI, Tauri, Web) must provide interactive approval dialogs presenting exact diffs, command parameters, target resources, and risk rationales.

---

## 5. Non-Functional Requirements (NFR)

| Category | Requirement Specification |
|---|---|
| **Security** | Default-deny posture for mutating/executing tools; zero plain-text secrets in prompts, logs, or UI; strict SSRF guards blocking private IP ranges (`127.0.0.1`, `169.254.169.254`). |
| **Reliability** | Safe retry classification; compare-and-swap (CAS) mechanics on file writes; zero duplicate side effects on network drops. |
| **Performance** | In-process tool validation overhead < 5ms; process sandboxing initialization < 50ms; bounded memory allocation for tool stdout/stderr streams. |
| **Observability** | Correlated OpenTelemetry spans; Prometheus counters/histograms (`tool_requests_total`, `tool_execution_duration_seconds`); structured JSON logging via `log/slog`. |
| **Portability** | Go 1.27.1 cross-compilation across Linux, macOS, and Windows without external CGO dependencies for the core engine. |
| **Compatibility** | Universal mapping across LLM tool-calling dialects (OpenAI function calling, Anthropic tools, Gemini function declarations). |
