# Phase 3 Correction: Provider Routing Architecture Decision
**Date:** 2026-10-02 23:42 UTC+7  
**Status:** LOCKED (Achmad decision)  
**Related:** `phase3-scope-tech-spec.md` TASK C

---

## Executive Summary

The original `README.md` (2026-09-28) proposed building a **Go-side provider gateway** (`engine/src/llm/`).

**Correction (2026-10-02):** A mature, battle-tested provider abstraction **already exists in Rust** (`crates/clawcrew-providers/`). Building a parallel Go system would:
- Duplicate 20+ provider implementations
- Require new gRPC RPC (proto doesn't have model-call RPC today)
- Add architectural complexity with no concrete use case justifying it

**Decision:** Extend Rust provider system for Phase 3. Defer Go `engine/src/llm/` until Go engine has a **concrete need** to call LLM directly (currently: zero such need).

---

## Architecture Ground Truth

### What Already Exists (Rust)

**`crates/clawcrew-providers/`** (live, mature):
- `FamilyProviderFactory` trait + dispatch macro
- 20+ provider families: OpenRouter, Anthropic, Azure, Bedrock, Ollama, Gemini CLI, Kiro CLI, etc.
- `router.rs`: hint-based model routing (model name → provider)
- `catalog.rs`: model metadata + capability discovery
- `reliable.rs`: retry + fallback wrapper
- `auth.rs`: credential management

**All LLM calls today:** Happen inside Rust process (zerocode app, orchestrator agents). Go engine does NOT call providers.

### What Was Proposed (Go, Not Yet Built)

`docs/refactoring/phase3/provider-routing/` (dated 2026-09-28):
- Proposed `engine/src/llm/` Go-side gateway
- Would replicate provider families from Rust
- Would require new gRPC RPC for model calls
- No use case exists yet (Go engine = orchestrator only)

### What Proto Says

`proto/agent_service.proto`:
- `StartTurn`, `QuickQuery`, `HealthCheck` only
- **Zero model-call RPC** between Go and Rust

This means: All model calls are still Rust-internal. Go never invokes them.

---

## Phase 3 Decision: Extend Rust, Not Build Go

### What Phase 3 Will Do (Rust Extension)

**Add `RoutingStrategy` enum** to `router.rs`:
```rust
pub enum RoutingStrategy {
    ManualTable,      // Current: static hint → provider mapping
    OpenRouterNative, // Use OpenRouter's built-in routing
    NineBotRouting,   // Use 9Router's scoring + fallback
    // Extensible: add more strategies
}
```

**Verify OAuth providers** in `auth.rs` (Antigravity, Google already supported? if not, add).

**Effort:** ~3.5h (extend Rust router, verify auth)

### What Phase 3 Will NOT Do

- ❌ Do NOT scaffold `engine/src/llm/` Go system
- ❌ Do NOT duplicate 20+ providers from Rust to Go
- ❌ Do NOT create new model-call RPC (not needed; Rust already handles this)

### When Go `engine/src/llm/` Becomes Relevant

**Concrete use cases that would justify Go provider system:**
1. Go orchestrator needs to **call LLM directly** (today: it doesn't)
2. Go engine needs **independent provider health checking** (today: not needed)
3. Go requires **policy-driven model routing** separate from Rust execution (today: routing happens in Rust, decisions made by orchestrator in Go)

**Until one of these materializes, building Go system is YAGNI violation.**

---

## Principle

**Rust = Core System (Providers, Tool Execution, Sandbox)**
- `clawcrew-providers/` is the canonical provider abstraction
- Tool execution (ExecuteNativeTool) is Rust responsibility
- LLM calls happen in Rust

**Go = Orchestrator Only**
- Manage runs, turns, agents, tasks
- Coordinate with Rust via gRPC (StartTurn, QuickQuery, HealthCheck)
- Make policy decisions (which agent, which workspace, which tool)
- Does NOT call providers directly

---

## Impact on `provider-routing/` Docs

**This correction does NOT invalidate** the original `README.md`'s thinking about:
- ✅ Provider normalization (adopt in Rust)
- ✅ Model catalog synchronization (adopt in Rust)
- ✅ Provider accounts / credentials (adopt in Rust)
- ✅ Pricing & usage tracking (adopt in Rust)
- ✅ Route profiles / combos (adopt in Rust router strategies)

**But it changes WHERE these are built:** All in Rust `clawcrew-providers/`, not in Go `engine/src/llm/`.

**To update `provider-routing/README.md`:**
1. Add this correction section to top
2. Reframe §2 "9router Pattern Adoption" as "How Rust `clawcrew-providers` Already Implements These Patterns"
3. Add "Future: Go integration" section explaining when/why `engine/src/llm/` might become relevant
4. Link to `phase3-scope-tech-spec.md` TASK C for Phase 3 scope

---

## References

- **Phase 3 Tech Spec:** `/artifacts/phase3-scope-tech-spec` (TASK C section)
- **Existing Rust Providers:** `crates/clawcrew-providers/src/`
- **Architecture Decision:** Achmad (@github user) — 2026-10-02, phase3 architecture review
- **Deferred Proposal:** Original `docs/refactoring/phase3/provider-routing/` (still valid thinking, just implementation location changed)
