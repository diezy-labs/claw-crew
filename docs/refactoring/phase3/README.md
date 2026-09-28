# Claw-Crew Phase 3: Architectural Documentation & Execution Roadmap

> **Status:** Living Architectural Documentation & Roadmap  
> **Target Branch Context:** `feat/enhance-agent-phase2`  
> **Target Runtime:** Go `1.27.1` (`engine/src/`)  
> **Parent Directory:** [`docs/refactoring/`](../)  
> **Date:** 2026-09-28  

---

## 1. Phase 3 Overview & Subsystem Pillars

Claw-Crew Phase 3 builds upon the multi-agent and streaming foundation of Phase 2 to deliver a governed, policy-first execution engine. Phase 3 documentation is partitioned into three core directories:

```text
docs/refactoring/phase3/
├── tool-calling/        # Governed Tool Runtime, sandboxing, MCP client, approvals
├── provider-routing/    # Claw Model Gateway (CMG), model governance, token efficiency
└── action-plan/         # Milestones M0-M6, database migrations, rollout & DoD
```

---

## 2. Directory Navigation & Summary

### 2.1 Tool Calling Platform ([`tool-calling/`](./tool-calling/))
Focuses on creating a **governed Tool Runtime** in `engine/src/tool/`.
- [**`tool-calling/README.md`**](./tool-calling/README.md): High-level topology, core principles, and executive summary.
- [**`tool-calling/prd.md`**](./tool-calling/prd.md): Problem statement, Tool Calling vs. RAG, maturity levels (L0–L8), and requirements (FR-01 to FR-10).
- [**`tool-calling/tech-spec.md`**](./tool-calling/tech-spec.md): Go 1.27.1 Clean Architecture (`interfaces.go`, `dto.go`, `services.go`, `wire.go`), `ValidateSandboxPath`, and MCP integration.
- [**`tool-calling/api-spec.md`**](./tool-calling/api-spec.md): REST endpoints (`/api/v1/tools`, `/api/v1/approvals`), SSE streams, and UI/UX flows.
- [**`tool-calling/security-governance.md`**](./tool-calling/security-governance.md): Capability hierarchy, one-time CAS-bound approvals, SSRF guard, and threat model.
- [**`tool-calling/testing-evals.md`**](./tool-calling/testing-evals.md): Concurrency/race tests (`go test -race`), golden eval fixtures, and Prometheus metrics.
- [**`tool-calling/task-breakdown.md`**](./tool-calling/task-breakdown.md): Phased task checklist (T0–T5), real-world workflows, and Definition of Done.

### 2.2 Provider Routing & Model Governance ([`provider-routing/`](./provider-routing/))
Focuses on the **Claw Model Gateway (CMG)** in `engine/src/llm/`.
- [**`provider-routing/README.md`**](./provider-routing/README.md): Executive decision, 9router comparator analysis (adopt vs reject), and high-level topology.
- [**`provider-routing/prd.md`**](./provider-routing/prd.md): CMG product goals, non-goals, capability matrix, route profiles, and UI/UX plan.
- [**`provider-routing/tech-spec.md`**](./provider-routing/tech-spec.md): Go 1.27.1 Clean Architecture in `engine/src/llm/`, domain abstractions, Wire DI, and context budgeter.
- [**`provider-routing/api-spec.md`**](./provider-routing/api-spec.md): REST contracts (`/api/v1/provider-accounts`, `/api/v1/models`, `/api/v1/route-policies`) and Tauri IPC commands.
- [**`provider-routing/routing-algorithm.md`**](./provider-routing/routing-algorithm.md): 15-stage deterministic selection pipeline, multi-attribute scoring formula, and safe fallback rules.
- [**`provider-routing/security-governance.md`**](./provider-routing/security-governance.md): Secret references (`secret://`), data classification matrix, and compliance rules.
- [**`provider-routing/evaluation-scorecard.md`**](./provider-routing/evaluation-scorecard.md): Golden eval fixtures, quality scorecards, model lifecycle, and canary gates.

### 2.3 Provider Flexibility Action Plan ([`action-plan/`](./action-plan/))
Focuses on the **concrete execution roadmap and operational delivery**.
- [**`action-plan/README.md`**](./action-plan/README.md): Execution strategy, scope boundaries, and milestone roadmap (M0–M6).
- [**`action-plan/milestones.md`**](./action-plan/milestones.md): In-depth deliverable definitions, required tests, and exit criteria for Milestones M0 through M6.
- [**`action-plan/task-checklist.md`**](./action-plan/task-checklist.md): Granular task checklist across backend APIs, Tauri IPC, Web UI, and engine wiring.
- [**`action-plan/data-migration.md`**](./action-plan/data-migration.md): Relational schemas, database entities, indexes, and persistence strategy.
- [**`action-plan/test-and-rollout.md`**](./action-plan/test-and-rollout.md): Shared adapter contract tests, race/load tests, and 4-stage rollout plan.
- [**`action-plan/governance-decisions.md`**](./action-plan/governance-decisions.md): 10 explicit policy decisions, recommended defaults, prioritization, and Definition of Done.
