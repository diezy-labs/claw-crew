# Quartermaster — Fleet Command Architecture: Overview

> **Status:** Product and technical design proposal
> **Product:** Galleon Fleet
> **Target architecture:** Go 1.27.1 as AI-native orchestrator and policy authority
> **Date:** 2026-09-28

---

## Document Index

This architecture was split into focused documents for easier navigation and review.

| # | Document | Scope |
|---|---|---|
| 00 | [Overview](00-overview.md) | Executive summary, background, and document map |
| 01 | [Product Vocabulary](01-product-vocabulary.md) | Canonical pirate-to-product term mapping |
| 02 | [Product Positioning](02-product-positioning.md) | Thesis, differentiators, why Fleet |
| 03 | [PRD](03-prd.md) | Problem, users, goals, user stories, success metrics |
| 04 | [User Roles and Authority](04-user-roles-authority.md) | Pirate King, Quartermaster, Captain, Squad, Crew Member |
| 05 | [Fleet Operating Model](05-fleet-operating-model.md) | Topology, isolation, Voyage lifecycle, cross-Ship rules |
| 06 | [Crew and Squad Model](06-crew-squad-model.md) | Ship templates, member config, skill packages |
| 07 | [Quartermaster Responsibilities](07-quartermaster-responsibilities.md) | Core duties, outputs, briefs, escalation, permissions |
| 08 | [System Design](08-system-design.md) | Design goals, components, summary projection, handoff |
| 09 | [C4 Architecture](09-c4-architecture.md) | C4 Levels 1-4 diagrams |
| 10 | [Technical Specification](10-technical-specification.md) | Go module layout, interfaces, concurrency, status, memory |
| 11 | [Data Model](11-data-model.md) | Entities, persistence tables, relationships |
| 12 | [Policy Security Governance](12-policy-security-governance.md) | Classification, budgets, freeze, audit |
| 13 | [API Contracts](13-api-contracts.md) | REST API endpoints and payloads |
| 14 | [Event Contracts](14-event-contracts.md) | Domain events and payloads |
| 15 | [UI UX Design](15-ui-ux-design.md) | Navigation, Command Deck, approval UX, visual style |
| 16 | [Operational Flows](16-operational-flows.md) | End-to-end operational scenarios |
| 17 | [Implementation Roadmap](17-implementation-roadmap.md) | Phases F0-F4 with deliverables and exit criteria |
| 18 | [Task Breakdown](18-task-breakdown.md) | All tasks with checkboxes |
| 19 | [Testing Strategy](19-testing-strategy.md) | Unit, integration, race, security, E2E tests |
| 20 | [Risks and Decisions](20-risks-decisions.md) | Risk register, critical decisions, defaults |
| 21 | [Definition of Done](21-definition-of-done.md) | Release completion criteria |
| 22 | [Appendices](22-appendices.md) | Example reports, templates, ADRs |

---

## Executive Summary

Galleon Fleet should evolve from a single-project agent workspace into a **Fleet Command system**. A human user, called the **Pirate King**, can own and govern multiple Ships. Each Ship is a bounded workspace or project with its own Captain, squads, crew members, memory boundary, budget, policies, artifacts, and active voyages.

The Pirate King is supported by one strategic AI assistant: the **Quartermaster**.

The Quartermaster is not a super-agent, unrestricted administrator, or autonomous executive. It is a fleet coordination layer that:

- Receives objectives and priorities from the Pirate King.
- Maintains an overview of all Ships.
- Routes strategic objectives to the appropriate Ship or Ships.
- Consolidates Ship Reports into concise Fleet Reports.
- Monitors budget, provider/model usage, policy health, workflow blockers, and approval queues.
- Coordinates approved cross-ship artifact handoff.
- Escalates only decisions, risks, and exceptions that require the Pirate King.
- Curates lessons as proposals; it never silently converts local Ship memory into fleet-wide policy.

This architecture preserves the most important safety rule:

> The Pirate King remains the final authority. The Quartermaster coordinates, summarizes, recommends, and escalates; it does not silently expand privileges, alter policy, publish externally, deploy, spend budget, or execute irreversible actions.

The pirate metaphor maps directly to product objects:

```text
Pirate King    = human owner and final authority
Quartermaster  = fleet coordinator and executive reporting assistant
Fleet          = portfolio of Ships
Ship           = project/workspace isolation boundary
Captain        = orchestrator of one Ship
Squad          = functional team within a Ship
Crew Member    = specialist AI agent with bounded role/skills/tools
Voyage         = run or workflow
Job Order      = task assignment
Ship Log       = event/audit timeline
Treasure       = artifact/output
Map            = plan/task graph
Port           = external integration, provider, tool, or MCP server
Cargo          = source documents/data inputs
```

The target implementation is Go 1.27.1 as the control plane. Rust TUI, Tauri desktop, and web clients remain presentation surfaces.

---

## Quartermaster Background

### Why the name Quartermaster

In maritime and naval traditions, a quartermaster historically had operational responsibilities that varied by era: navigation support, watch operations, crew coordination, supply/logistics, stores, and distribution of operational resources. In pirate narratives, the quartermaster is often portrayed as an important officer who represents crew interests, helps coordinate operations, and manages practical ship-level concerns.

For Galleon Fleet, the Quartermaster name is useful because it implies more than a secretary and less than an unchecked commander. It communicates:

- Operational coordination.
- Resource and budget awareness.
- Readiness monitoring.
- Information flow.
- Task routing.
- Report consolidation.
- Fleet discipline and governance support.

It is particularly appropriate because Galleon Fleet will eventually need to manage finite resources:

- Token budgets.
- Provider/model allocation.
- Tool permissions.
- Concurrent voyages.
- Artifact storage.
- Crew capacity.
- Approval queues.
- Cross-Ship dependencies.

### Why not First Mate

**First Mate** is a strong alternative for a general executive assistant. It emphasizes leadership and command succession. However, within Galleon Fleet, a Ship already has a Captain. If the global assistant is named First Mate, users may confuse whether it belongs to one Ship or the entire Fleet.

**Quartermaster** is more specific to fleet-level coordination and resource governance. It fits the intended role better:

```text
Captain: leads one Ship and its active voyage.
Quartermaster: coordinates fleet resources, reports, priorities, and readiness across Ships.
Pirate King: owns strategic direction and final authority.
```

### Quartermaster identity

#### Official role

```text
Quartermaster
Fleet Coordination and Governance Assistant
```

#### Optional brand identity

```text
Codename: Quarterclaw
```

Use **Quartermaster** in all technical documents, policies, API names, and permission models. Use **Quarterclaw** as optional visual branding, mascot identity, or friendly UI copy.

#### Core mission

> Help the Pirate King command many Ships without becoming an uncontrolled command authority.

#### Short UI description

> Your fleet coordinator. Quartermaster organizes fleet priorities, summarizes Ship reports, tracks resources and risks, and escalates only the decisions that need your command.

---

## Existing Engine Foundation

The current `engine/` codebase already implements core concepts that the Fleet Command architecture renames and extends:

| Existing | Fleet Equivalent | Status |
|---|---|---|
| `src/run/` — Run lifecycle, state machine, SSE events | Voyage | ✅ Rename + extend state machine |
| `src/task/` — DAG task scheduler with dependencies | Job Order | ✅ Rename |
| `src/artifact/` — Artifact with hash, type, content | Treasure | ✅ Rename + add classification |
| `src/crew/` — AgentDefinition, CrewDefinition, Orchestrator | CrewMember, Squad, Captain | ✅ Rename + extend |
| `src/workflow/` — WorkflowTemplate, StepTemplate | Map | ✅ Rename |
| `src/tool/` — PolicyEngine, ApprovalGate, RiskTier, ExecutionContext | Tool Runtime (Fleet policy) | ✅ Already aligned |
| `src/llm/` — MultiProvider, TokenUsage | Model Gateway | ✅ Extend with budget/routing |
| `src/memory/` — VectorStore with scope isolation | Ship Memory | ✅ Extend scope boundaries |

> **Genuinely new modules:** Fleet, Ship, Pirate King, Quartermaster, Reporting, Escalation, Budget, Handoff, Fleet Knowledge.
