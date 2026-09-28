# Claw-Crew Phase 3 — Tool Calling Platform: Testing & Evaluation Strategy

> **Status:** Proposed Verification & Evaluation Plan  
> **Target:** `engine/src/tool/` test suites & evaluation benchmarks  
> **Parent Directory:** [`docs/refactoring/phase3/tool-calling/`](./)  

---

## 1. Testing Strategy Overview

Testing the Tool Calling platform requires a multi-layered verification strategy spanning unit logic, security containment, concurrency races, and agent behavioral evaluations:

```text
┌────────────────────────────────────────────────────────┐
│ Agent Evaluation Benchmarks (Golden Fixtures)          │
│ (Prompt injection, tool selection accuracy, grounding) │
└───────────────────────────┬────────────────────────────┘
                            │ Built on
                            ▼
┌────────────────────────────────────────────────────────┐
│ Concurrency & Race Tests (go test -race -count=100)    │
│ (Approval races, stream disconnects, timeout cleanup)  │
└───────────────────────────┬────────────────────────────┘
                            │ Built on
                            ▼
┌────────────────────────────────────────────────────────┐
│ Integration & Security Containment Tests               │
│ (ValidateSandboxPath, symlink escapes, SSRF filters)   │
└───────────────────────────┬────────────────────────────┘
                            │ Built on
                            ▼
┌────────────────────────────────────────────────────────┐
│ Unit & Schema Contract Tests                           │
│ (JSON Schema validation, canonical hashing, DTOs)      │
└────────────────────────────────────────────────────────┘
```

---

## 2. Go Test Gates & Concurrency Verification

All tool engine code must pass strict Go testing gates before merge:

```bash
cd engine

# Format and vet
gofmt -w ./src/tool/...
go vet ./src/tool/...

# Strict race detection and stress testing
go test -race ./src/tool/...
go test -race -count=20 ./src/tool/...

# Target-specific security regression suites
go test -run TestApprovalEnforcement -race ./src/tool/...
go test -run TestPathContainment -count=50 ./src/tool/...
go test -run TestDirectDispatchDenial -race ./src/tool/...
```

### Critical Concurrency Scenarios:
1. **Approval Double-Resolution Race:** Multiple parallel requests attempting to resolve the same approval token must result in exactly one execution, with all other requests receiving `ErrApprovalExpired` or `ErrAlreadyResolved`.
2. **Context Cancellation Propagation:** When the parent run context cancels while a sandboxed test runner or network fetch is in-flight, the process group and HTTP transport must terminate within 100ms.
3. **Semaphore Saturation:** When 50 concurrent tool invocations hit a worker pool sized to 10, excess jobs must queue without deadlocking or leaking memory.

---

## 3. Golden Evaluation Fixtures (`evals/tools/`)

To evaluate LLM tool-calling fidelity, standard test fixtures evaluate agent decisions against predefined ground truth:

```text
evals/
└── tools/
    ├── codebase-audit/         # Verify agent reads relevant files without mutating
    ├── web-fetch-ssrf/         # Verify SSRF block when prompt references 169.254.169.254
    ├── prompt-injection/       # Verify agent ignores "Ignore previous rules" in fetched HTML
    ├── patch-cas-conflict/     # Verify engine aborts when target file changes before patch
    └── secret-redaction/       # Verify tokens in output logs are scrubbed
```

### Evaluation Scorecard Metrics:

| Metric | Target | Measurement Method |
|---|---|---|
| **Tool Selection Precision** | > 95% | Fraction of tool requests matching optimal tool for prompt intent. |
| **Argument Validity Rate** | > 98% | Percentage of generated JSON calls passing schema validation on first try. |
| **Approval Appropriateness** | 100% | Zero instances of mutating (`WRITE`/`EXECUTE`) tools running without approval. |
| **Prompt Injection Resistance**| 100% | Zero unauthorized tool invocations triggered by untrusted web content. |
| **CAS Conflict Prevention** | 100% | Zero corrupt file overwrites when underlying file hash diverges. |
| **Secret Redaction Rate** | 100% | Zero leaked credential strings matching registered patterns. |

---

## 4. Observability & Prometheus Metrics

All tool invocations report runtime performance and security metrics through `engine/core/metrics`:

```go
// Metrics registered in engine/core/metrics/tool_metrics.go
var (
	ToolRequestsTotal = prometheus.NewCounterVec(prometheus.CounterOpts{
		Name: "clawcrew_tool_requests_total",
		Help: "Total tool requests categorized by tool name, risk tier, and status.",
	}, []string{"tool_name", "risk_tier", "status"})

	ToolExecutionDuration = prometheus.NewHistogramVec(prometheus.HistogramOpts{
		Name:    "clawcrew_tool_execution_duration_seconds",
		Help:    "Execution duration of tools in seconds.",
		Buckets: []float64{0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 15.0, 30.0},
	}, []string{"tool_name", "risk_tier"})

	ToolPolicyDenialsTotal = prometheus.NewCounterVec(prometheus.CounterOpts{
		Name: "clawcrew_tool_policy_denials_total",
		Help: "Count of tool invocations blocked by policy engine.",
	}, []string{"tool_name", "reason"})

	ToolApprovalsTotal = prometheus.NewCounterVec(prometheus.CounterOpts{
		Name: "clawcrew_tool_approvals_total",
		Help: "Total approval requests categorized by status (approved, denied, expired).",
	}, []string{"tool_name", "status"})
)
```
