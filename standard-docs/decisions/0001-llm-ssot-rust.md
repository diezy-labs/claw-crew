# ADR 0001: LLM Provider Calling SSOT is Rust

**Status:** Accepted  
**Date:** 2026-10-03  
**Deciders:** Achmad (Product Owner), squad-lead (Tech Lead)

---

## Context

Galleon-fleet codebase has **two independent LLM provider implementations**:

1. **Go `engine/src/llm/`** — OpenAI streaming client with retry+fallback (`multi_provider.go`), used by `crew.StartTurn` (Go orchestrator)
2. **Rust `clawcrew-providers`** — Multi-vendor native (OpenAI, Gemini, Bedrock, Anthropic) with `Router`+`Dispatch` resilience, used by Rust `runtime/agent/turn.rs`

Both are **production-active** (Go: 15 matches in 7 files, Rust: 895 matches in 76 files). This violates Single Source of Truth (SSOT) principle and creates:

- **Duplicate resilience logic** (two fallback chains for the same domain)
- **Vendor coverage mismatch** (Rust supports 4+ vendors, Go only OpenAI)
- **Architecture boundary confusion** (Go should orchestrate, not execute wire-calling)

---

## Decision

**Rust `clawcrew-providers` is the SSOT for all LLM vendor API calling.**

Go `engine/src/llm/` will be **removed**. Go `crew.StartTurn` will **delegate to Rust** via gRPC `SystemGateway.ExecuteTurn` for all provider execution.

---

## Rationale

### 1. Product Positioning Alignment

Galleon-fleet was built to avoid **single-point-of-failure** when one vendor's credit limit blocks all work (KiroCrew limitation). Rust providers already implement **native multi-vendor fallback** (OpenAI → Gemini → Bedrock chain), which directly serves this product goal. Go implementation is OpenAI-only hardcode, not aligned.

### 2. Architecture Tier Separation

Per `.gemini/architecture.md` (corrected):

- **Go engine = AI Orchestrator** (fleet logic, crew coordination, memory governance)
- **Rust core = System Core + Security Microkernel** (native tools, sandboxing, provider execution)

LLM wire-calling is **security-sensitive execution** (API key handling, rate-limit enforcement, credential scrubbing) — belongs in Rust System Core, not Go Orchestrator.

### 3. Code Maturity & Vendor Support

| Metric | Go `llm/` | Rust `providers` |
|--------|-----------|------------------|
| LOC | ~500 | 81,000 |
| Vendors | 1 (OpenAI only) | 4+ (OpenAI, Gemini, Bedrock, Anthropic) |
| Resilience | `MultiProvider` retry | `Router`+`Dispatch` + circuit-breaker |
| Test coverage | Basic | Comprehensive (dispatch_integration, hailo_ollama) |

Rust implementation is **production-mature** and **multi-vendor native** as claimed by product goals.

### 4. Existing gRPC Boundary

`clawcrew-gateway` already exposes `SystemGatewayService` gRPC at `:50052`. Go engine calls Rust for tool execution (`ToolDispatcher` already delegates correctly). Extending this to LLM execution is **architectural consistency**, not a new pattern.

---

## Consequences

### Positive

- **Single SSOT** for all vendor API calling (no duplicate resilience logic)
- **Consistent vendor fallback** across all execution paths (crew, workflow, standalone agent)
- **Tighter security boundary** (API keys + credential scrubbing isolated in Rust microkernel)
- **Go engine becomes thinner** (orchestration-only, as intended)

### Negative

- **gRPC overhead** (~1-2ms per turn) — acceptable trade-off for security isolation + SSOT consistency
- **Go `crew/services.go` refactor required** — remove `llmProvider` field, delegate to Rust gRPC

### Neutral

- **Frontend routing unchanged** — web-2 + Tauri already call Go HTTP :9090, Go `crew.StartTurn` internally forwards to Rust (invisible to frontend)

---

## Implementation Plan

### Phase 1: Rust gRPC Extension (2-3 days)
- [ ] Add `ExecuteTurn` RPC to `SystemGatewayService` proto
- [ ] Wire Rust `runtime/agent/turn.rs` `execute_turn` as gRPC handler
- [ ] Test gRPC call from Go client with OpenAI provider

### Phase 2: Go Delegation (1 day)
- [ ] Refactor Go `crew/services.go`: remove `llmProvider llm.Provider` field
- [ ] Add `systemGateway client.SystemGatewayClient` field
- [ ] `StartTurn` calls `systemGateway.ExecuteTurn()` instead of `llmProvider.StreamChat()`
- [ ] Test crew turn end-to-end (Go orchestration → Rust execution)

### Phase 3: Cleanup (1 day)
- [ ] Delete Go `engine/src/llm/provider.go`, `multi_provider.go`, `mock_provider.go`
- [ ] Delete Go `llm/interfaces.go`, `llm/wire.go` (no longer needed)
- [ ] Update Go tests to use Rust gRPC mock instead of Go mock provider
- [ ] Verify all Go tests + integration tests pass

---

## Related

- ADR 0002: web-2 Backend Logic Policy (QR code + telemetry move to Go/Nginx)
- ADR 0003: Tauri Metrics Policy (fake data → real API read)
- `docs/refactoring-rust/living-strategy.md` Section 7 (Rust<->Go hierarchy audit)
