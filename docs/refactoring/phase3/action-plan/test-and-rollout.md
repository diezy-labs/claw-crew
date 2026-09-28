# Claw-Crew Phase 3 — Action Plan: Testing & Rollout Strategy

> **Status:** Proposed Testing & Rollout Plan  
> **Parent Directory:** [`docs/refactoring/phase3/action-plan/`](./)  

---

## 1. Testing Strategy

```text
┌────────────────────────────────────────────────────────┐
│ Concurrency & Race Verification (go test -race)        │
│ (Concurrent route selection, circuit breaker updates)  │
└───────────────────────────┬────────────────────────────┘
                            │ Built on
                            ▼
┌────────────────────────────────────────────────────────┐
│ End-to-End Integration & Fallback Scenarios            │
│ (Transient 429 recovery, confidential data blocks)     │
└───────────────────────────┬────────────────────────────┘
                            │ Built on
                            ▼
┌────────────────────────────────────────────────────────┐
│ Shared Provider Adapter Contract Test Suite            │
│ (Streaming chunks, context cancel, error normalization)│
└───────────────────────────┬────────────────────────────┘
                            │ Built on
                            ▼
┌────────────────────────────────────────────────────────┐
│ Unit Tests: Filtering, Scoring & Hashing               │
│ (Capability filters, weight calculations, budget check)│
└────────────────────────────────────────────────────────┘
```

---

## 2. Shared Adapter Contract Test Suite (`adapter_contract_test.go`)

Every upstream provider adapter (Ollama, OpenAI, OpenRouter) must pass the exact same suite of parameterized contract tests:

- [ ] **Health Probe:** Returns `HealthStatusHealthy` on reachable endpoint; `HealthStatusDown` on network failure.
- [ ] **Stream Generation:** Emits valid `ChatChunk` instances in correct lexical order until `IsDone=true`.
- [ ] **Immediate Cancellation:** Cancelling the `context.Context` aborts streaming within 100ms and cleans up TCP resources.
- [ ] **Tool Call Formatting:** Correctly maps model tool invocations to structured `ToolCall` objects.
- [ ] **Usage Extraction:** Normalizes prompt and completion tokens into `TokenUsage`.
- [ ] **Credential Masking:** Never reveals raw Bearer tokens or API keys in error strings.

---

## 3. Concurrency, Race & Load Testing

To ensure stability under multi-agent execution loads:

```bash
cd engine

# Strict data race check across all packages
go test -race ./...

# High-frequency stress test on route selection and circuit breakers
go test -race -count=50 ./src/llm/...
go test -run TestConcurrentRouteSelection -count=100 ./src/llm/...
go test -run TestCircuitBreakerUnderContention -race ./src/llm/...
```

### Stress Scenarios:
1. **Parallel Route Resolution:** 100 parallel agent tasks requesting route decisions simultaneously without mutex deadlocks.
2. **Dynamic Circuit Breaker Tripping:** Upstream 500 errors injected concurrently while other tasks are routing.
3. **Budget Contention:** Multiple parallel tasks updating the shared run spending ledger atomically without lost updates.

---

## 4. Four-Stage Rollout Plan

### Stage 1: Internal Developer Preview
- **Target:** Core engine developers.
- **Configuration:** Local Ollama + one OpenAI-compatible cloud endpoint enabled. Static model catalog.
- **Goal:** Verify that basic routing, streaming, and tool dispatch work smoothly in CLI and TUI.

### Stage 2: Controlled Workspace Testing
- **Target:** Selected dogfooding workspaces.
- **Configuration:** OpenRouter adapter enabled with verified route profiles (`balanced`, `code`, `local-only`).
- **Goal:** Validate Route Simulator UI, Run Inspector explainability, and token usage accounting.

### Stage 3: Canary Model Evaluation
- **Target:** 10% of background research tasks.
- **Configuration:** Newly added models deployed in `canary` lifecycle state.
- **Goal:** Compare cost, latency, and task completion metrics against established baselines using golden eval suites.

### Stage 4: General Availability (Stable Default)
- **Target:** All Claw-Crew users.
- **Configuration:** Full dynamic routing active. Default profile set to `balanced` for standard workspaces, `local-only` for high-privacy mode.
- **Goal:** Continuous observability, automated circuit breaking, and stable multi-provider operations.
