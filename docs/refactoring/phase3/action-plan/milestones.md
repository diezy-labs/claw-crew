# Claw-Crew Phase 3 — Action Plan: Milestone Deliverables

> **Status:** Detailed Milestone Specifications  
> **Parent Directory:** [`docs/refactoring/phase3/action-plan/`](./)  

---

## Milestone M0 — Foundation Contracts & ADRs

### Goal
Establish canonical provider-routing data models, security boundaries, and error contracts in `engine/src/llm/` prior to writing provider-specific network code.

### Deliverables
- [ ] **ADR-001:** Go engine is the sole owner of provider route selection.
- [ ] **ADR-002:** Only official, documented, and terms-compliant provider integrations are permitted.
- [ ] **ADR-003:** Upstream credentials are referenced via secret URIs (`secret://`), never logged or returned.
- [ ] **ADR-004:** Data classification (`public`, `internal`, `confidential`, `restricted`) gates all routing decisions.
- [ ] **ADR-005:** Route decisions, attempts, and fallbacks are auditable events persisted per run.
- [ ] Define canonical DTO schemas in `engine/src/llm/dto.go`: `ProviderAccount`, `ModelDescriptor`, `CapabilitySet`, `RoutePolicy`, `RouteDecision`, `TokenUsage`.
- [ ] Standardize provider error codes in `engine/core/errors/errors.go`:
  ```text
  PROVIDER_AUTH_FAILED
  PROVIDER_RATE_LIMITED
  PROVIDER_TIMEOUT
  PROVIDER_UNAVAILABLE
  PROVIDER_OVERLOADED
  PROVIDER_CAPABILITY_UNSUPPORTED
  MODEL_NOT_FOUND
  MODEL_DISABLED
  MODEL_QUARANTINED
  ROUTE_POLICY_DENIED
  ROUTE_BUDGET_EXCEEDED
  ROUTE_PRIVACY_DENIED
  ROUTE_FALLBACK_EXHAUSTED
  ```

### Exit Criteria
- Contract tests verify serialization/deserialization across all new DTOs.
- Zero raw secrets appear in log outputs, API responses, or error envelopes.
- Contract tests pass in CI.

---

## Milestone M1 — Provider Adapter Foundation

### Goal
Implement a unified `ProviderAdapter` abstraction supporting local inference, cloud OpenAI-compatible endpoints, and OpenRouter through a single Go interface.

### Adapter Deliverables
1. **Local / Ollama Adapter (`engine/src/llm/adapter_ollama.go`):**
   - Health check ping (`GET /api/tags`).
   - Non-streaming and streaming chat completion (`POST /api/chat`).
   - Normalization of local model tool calling and token usage.
2. **OpenAI-Compatible Adapter (`engine/src/llm/adapter_openai.go`):**
   - Configurable custom base URL (supports private gateways, vLLM, Azure).
   - Secret reference resolution for Bearer authentication.
   - Structured JSON output support (`response_format: { type: "json_object" }`).
   - Streaming SSE chunk parsing into `ChatChunk`.
3. **OpenRouter Adapter (`engine/src/llm/adapter_openrouter.go`):**
   - Official API integration with model catalog sync.
   - Normalization of upstream provider-reported usage and cache hits.

### Common Interface Skeleton
```go
type ProviderAdapter interface {
	ProviderType() ProviderType
	CheckHealth(ctx context.Context, account *ProviderAccount) (HealthStatus, error)
	DiscoverModels(ctx context.Context, account *ProviderAccount) ([]ModelDescriptor, error)
	StreamChat(ctx context.Context, account *ProviderAccount, modelID string, req *ChatRequest, chunkCh chan<- *ChatChunk) error
}
```

### Exit Criteria
- A single test crew task executes successfully against either local Ollama or cloud OpenAI without modifying orchestrator code.
- Context cancellation terminates upstream HTTP connections within 100ms.

---

## Milestone M2 — Route Profiles & Policy Selection

### Goal
Enable crew workflows to select routes based on intent (`local-only`, `economy`, `balanced`, `premium`, `code`, `research`) and explicit model pinning.

### Deliverables
- [ ] Implement `RouteSelector` in `engine/src/llm/route_service.go`.
- [ ] Implement hard capability filtering (disqualifies models lacking tool calling or structured output).
- [ ] Implement data classification filtering (restricts confidential data to local or approved private endpoints).
- [ ] Implement profile resolution hierarchy:
  $$\text{Run Pinning} \succ \text{Task Override} \succ \text{Agent Default} \succ \text{Crew Default} \succ \text{Workspace Default}$$
- [ ] Implement simulation endpoint `POST /api/v1/route-policies/simulate`.
- [ ] Persist `RouteDecision` to disk/database and emit SSE event `route.decided`.

### Exit Criteria
- Tasks requiring tool calling are rejected before execution if routed to models without tool support.
- Confidential workspace tasks never invoke public cloud endpoints.
- User-pinned models are honored without silent fallback.

---

## Milestone M3 — Model Catalog, Health & Circuit Breakers

### Goal
Provide observable, automated health tracking and lifecycle management for all configured models and accounts.

### Deliverables
- [ ] Persist model catalog and provider account health in `engine/src/llm/catalog_service.go`.
- [ ] Implement background health checker with configurable probe interval (default 60s).
- [ ] Implement circuit breaker state transitions (`CLOSED`, `OPEN`, `HALF-OPEN`) based on rolling error rates.
- [ ] Implement model lifecycle states:
  $$\text{Discovered} \to \text{Unverified} \to \text{Sandbox} \to \text{Canary} \to \text{Enabled} \to \text{Quarantined}$$
- [ ] Build desktop and web UI views for Provider Settings, Model Catalog, and Health status.

### Exit Criteria
- Unhealthy endpoints are automatically removed from candidate selection.
- Newly discovered models remain in `unverified` state until explicitly approved or validated.

---

## Milestone M4 — Token, Cost & Fallback Controls

### Goal
Eliminate token waste and runaway API costs through context budgeting and safe fallback boundaries.

### Deliverables
- [ ] **Token Estimator:** Pre-flight token counting to detect oversized prompts before making upstream calls.
- [ ] **Context Budgeter:** Prioritized partition allocation protecting user goal, agent safety policy, and tool schemas while compressing retrieval evidence.
- [ ] **Budget Guardrails:** Enforce per-run hard spending limits and soft warning alerts.
- [ ] **Usage Ledger:** Track prompt, completion, and cache tokens with explicit pricing confidence labels (`provider_reported`, `catalog_estimate`, `unknown`).
- [ ] **Safe Fallback Controller:** Enforce that fallback triggers only on transient errors prior to partial tool execution.

### Exit Criteria
- Runs cannot exceed hard budget limits without explicit user approval.
- Retries and fallbacks never cause duplicate side-effecting operations.

---

## Milestone M5 — RAG-Aware Model Routing

### Goal
Integrate retrieval volume and citation requirements into model selection.

### Deliverables
- [ ] Separate embedding provider routing in `engine/src/memory/`.
- [ ] Inject retrieval volume metadata into `ModelRequest`.
- [ ] Route long-context research tasks to high-capacity models (e.g., Gemini 1.5 Pro / 2.0 Flash) when evidence volume exceeds 32k tokens.
- [ ] Add citation verification validator route.

### Exit Criteria
- Research tasks automatically select long-context models when retrieval exceeds standard window limits.
- RAG evidence remains strictly bounded within workspace privacy boundaries.

---

## Milestone M6 — Evals Suite, Canary & Governance

### Goal
Prevent model routing changes from silently degrading task success rates, output quality, or system safety.

### Deliverables
- [ ] Implement golden evaluation fixtures in `evals/provider-routing/` covering codebase audits, deep research, and tool use.
- [ ] Implement benchmark runner comparing cost, latency, and quality across route profiles.
- [ ] Implement canary deployment pipeline for newly added models.
- [ ] Add audit export for organization-level compliance and usage analysis.

### Exit Criteria
- Automated evals verify zero quality regression prior to promoting a model to default status.
- Versioned routing policies support instant rollback.
