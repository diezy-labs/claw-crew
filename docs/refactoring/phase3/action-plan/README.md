# Claw-Crew Phase 3 — Action Plan: Execution Strategy & Milestones

> **Status:** Implementation-Oriented Execution Plan  
> **Target Branch:** `feat/enhance-agent-phase2`  
> **Primary Runtime:** Go `1.27.1` (`engine/src/`)  
> **Parent Directory:** [`docs/refactoring/`](../../)  
> **Date:** 2026-09-28  

---

## 1. Objective & Strategic Vision

The objective of this action plan is to execute **Claw Model Gateway v1** and the **Tool Calling Platform** inside the Go orchestrator (`engine/`). Every crew, agent, and task will dynamically route to an approved model and provider according to:
- Required capabilities (tool calling, structured output, vision, context size).
- Data privacy classification (`public`, `internal`, `confidential`, `restricted`).
- User or workspace profile preferences (`local-only`, `economy`, `balanced`, `premium`).
- Cost budget ceilings and token efficiency constraints.
- Latency and reliability targets.
- Live provider health and bounded fallback policies.

The resulting system is **strictly explainable**: developers and users can inspect which route was chosen, why alternatives were rejected, estimated vs. actual costs, and whether any fallback occurred.

---

## 2. Scope Boundaries

```text
┌────────────────────────────────────────────────────────┐
│ IN SCOPE (Phase 3 Core Deliverables)                   │
├────────────────────────────────────────────────────────┤
│ ✓ Official/authorized API, SDK, and MCP integrations    │
│ ✓ Local model endpoint support (Ollama)                │
│ ✓ OpenAI-compatible cloud and gateway endpoints        │
│ ✓ OpenRouter supported cloud adapter                   │
│ ✓ Zero-plaintext secret reference management           │
│ ✓ Normalized model capability catalog & health monitor │
│ ✓ Route profiles, deterministic ranking & model pinning│
│ ✓ Bounded, side-effect-safe fallback controller        │
│ ✓ Token estimation & context budgeting                 │
│ ✓ Usage/cost ledger with confidence indicators         │
│ ✓ Full route decision auditability in UI & SSE streams │
│ ✓ Golden evals benchmarks & canary rollout pipeline    │
└────────────────────────────────────────────────────────┘

┌────────────────────────────────────────────────────────┐
│ OUT OF SCOPE (Deferred Beyond Phase 3)                 │
├────────────────────────────────────────────────────────┤
│ ✗ Universal reverse proxy for arbitrary third-party IDEs│
│ ✗ Account rotation / quota evasion techniques          │
│ ✗ Browser session / cookie harvesting hacks            │
│ ✗ Reverse-engineering private client endpoints         │
│ ✗ Multi-tenant enterprise credit billing system        │
│ ✗ Public community marketplace                         │
└────────────────────────────────────────────────────────┘
```

---

## 3. Milestones Roadmap (M0 to M6)

```mermaid
gantt
    title Claw-Crew Phase 3 Implementation Timeline
    dateFormat  YYYY-MM-DD
    section Foundation
    M0 : Foundation Contracts & ADRs       :m0, 2026-10-01, 7d
    section Core Routing & Adapters
    M1 : Provider Adapter Foundation       :m1, after m0, 10d
    M2 : Route Profiles & Selection        :m2, after m1, 10d
    section Observability & Governance
    M3 : Model Catalog & Health Monitor    :m3, after m2, 8d
    M4 : Token Budgeter & Cost Guardrails  :m4, after m3, 12d
    section Advanced Capabilities
    M5 : RAG-Aware Model Routing           :m5, after m4, 10d
    M6 : Evals Suite, Canary & Governance  :m6, after m5, 10d
```

| Milestone | Milestone Name | Main Outcome | Complexity | Prerequisite |
|---|---|---|---|---|
| **M0** | Foundation Contracts | Stable domain models, ADRs, secret interfaces, error taxonomy. | Medium | Phase 2 baseline |
| **M1** | Adapter Foundation | Local Ollama + OpenAI-compatible + OpenRouter adapters. | Medium | M0 |
| **M2** | Route Profiles | Capability-aware policy ranking and user model pinning. | Medium | M1 |
| **M3** | Catalog & Health | Model catalog storage, health checks, circuit breakers. | Medium | M2 |
| **M4** | Token & Cost Control | Context budgeter, token estimation, fallback guardrails. | Hard | M2, M3 |
| **M5** | RAG-Aware Routing | Embedding provider classes, evidence-aware context allocation. | Hard | Memory / Storage |
| **M6** | Evals & Rollout | Golden eval benchmarks, canary rollout, governance audit. | Hard | M4, M5 |

---

## 4. Document Index

This action plan is organized into the following specialized documents:

| Document | Focus & Key Deliverables |
|---|---|
| [**`milestones.md`**](./milestones.md) | **Detailed Milestone Deliverables**: Detailed objectives, tasks, required tests, and exit criteria for Milestones M0 through M6. |
| [**`task-checklist.md`**](./task-checklist.md) | **Granular Task Breakdown**: Comprehensive checklist (`[ ]`) across Backend APIs, Tauri IPC commands, Web UI, and Core Engine internals. |
| [**`data-migration.md`**](./data-migration.md) | **Database Entities & Persistence**: Schema definitions, indexes, and storage strategies for provider accounts, catalogs, decisions, and usage. |
| [**`test-and-rollout.md`**](./test-and-rollout.md) | **Testing & Rollout Plan**: Unit, contract, integration, and race testing (`go test -race`), plus 4-stage canary deployment plan. |
| [**`governance-decisions.md`**](./governance-decisions.md) | **Architecture Decisions & DoD**: 10 explicit policy questions, recommended defaults, final build priority, and Definition of Done. |
