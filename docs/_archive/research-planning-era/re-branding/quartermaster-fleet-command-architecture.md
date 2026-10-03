# Quartermaster — Fleet Command Architecture for Galleon Fleet

> **Status:** Product and technical design proposal  
> **Product:** Galleon Fleet  
> **Target architecture:** Go 1.27.1 as AI-native orchestrator and policy authority  
> **Scope:** Pirate King, Quartermaster, Fleet, Ships, Captains, Squads, Crew Members, Voyages, Job Orders, reporting, budgets, policy inheritance, and cross-ship coordination  
> **Date:** 2026-09-28

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Quartermaster Background](#2-quartermaster-background)
3. [Product Vocabulary](#3-product-vocabulary)
4. [Product Positioning](#4-product-positioning)
5. [Product Requirements Document](#5-product-requirements-document)
6. [User Roles and Authority](#6-user-roles-and-authority)
7. [Fleet Operating Model](#7-fleet-operating-model)
8. [Crew and Squad Model](#8-crew-and-squad-model)
9. [Quartermaster Responsibilities](#9-quartermaster-responsibilities)
10. [System Design](#10-system-design)
11. [C4 Architecture](#11-c4-architecture)
12. [Technical Specification](#12-technical-specification)
13. [Data Model](#13-data-model)
14. [Policy, Security, and Governance](#14-policy-security-and-governance)
15. [API Contracts](#15-api-contracts)
16. [Event Contracts](#16-event-contracts)
17. [UI and UX Design](#17-ui-and-ux-design)
18. [Operational Flows](#18-operational-flows)
19. [Implementation Roadmap](#19-implementation-roadmap)
20. [Task Breakdown](#20-task-breakdown)
21. [Testing and Quality Strategy](#21-testing-and-quality-strategy)
22. [Risks and Critical Decisions](#22-risks-and-critical-decisions)
23. [Definition of Done](#23-definition-of-done)
24. [Appendices](#24-appendices)

---

# 1. Executive Summary

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

The pirate metaphor is not decorative. It maps directly to product objects:

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

The target implementation is Go 1.27.1 as the control plane. Rust TUI, Tauri desktop, and web clients remain presentation surfaces. They render Fleet Command state and collect Pirate King approvals; they do not become independent authority or policy engines.

---

# 2. Quartermaster Background

## 2.1 Why the name Quartermaster

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

## 2.2 Why not First Mate

**First Mate** is a strong alternative for a general executive assistant. It emphasizes leadership and command succession. However, within Galleon Fleet, a Ship already has a Captain. If the global assistant is named First Mate, users may confuse whether it belongs to one Ship or the entire Fleet.

**Quartermaster** is more specific to fleet-level coordination and resource governance. It fits the intended role better:

```text
Captain: leads one Ship and its active voyage.
Quartermaster: coordinates fleet resources, reports, priorities, and readiness across Ships.
Pirate King: owns strategic direction and final authority.
```

## 2.3 Quartermaster identity

### Official role

```text
Quartermaster
Fleet Coordination and Governance Assistant
```

### Optional brand identity

```text
Codename: Quarterclaw
```

Use **Quartermaster** in all technical documents, policies, API names, and permission models. Use **Quarterclaw** as optional visual branding, mascot identity, or friendly UI copy.

### Core mission

> Help the Pirate King command many Ships without becoming an uncontrolled command authority.

### Short UI description

> Your fleet coordinator. Quartermaster organizes fleet priorities, summarizes Ship reports, tracks resources and risks, and escalates only the decisions that need your command.

---

# 3. Product Vocabulary

## 3.1 Canonical mapping

| Pirate term | Product/technical meaning | Example |
|---|---|---|
| Pirate King | Human account owner / executive authority | The user who owns the Fleet |
| Quartermaster | Fleet-level AI coordinator | Synthesizes reports and routes objectives |
| Fleet | Collection of related Ships under one owner | Personal Product Fleet |
| Ship | Isolated workspace/project/domain | Development Ship |
| Captain | Per-Ship orchestration agent | Engineering Captain |
| Squad | Functional group inside a Ship | Developer Squad |
| Squad Lead | Domain coordinator inside a Squad | Engineering Lead |
| Crew Member | Specialist agent | QA Engineer |
| Voyage | Workflow or run | Phase 3 Tool Runtime Review |
| Job Order | Task assignment | Run race tests and report failures |
| Ship Log | Immutable activity/audit feed | Tool calls, approvals, outputs |
| Fleet Report | Consolidated executive summary | Weekly Fleet Brief |
| Treasure | Generated artifact/value output | PRD, report, patch, article |
| Map | Task graph, plan, or workflow definition | Multi-Ship project plan |
| Port | External integration or provider endpoint | GitHub MCP, Ollama, OpenRouter |
| Cargo | Source data/documents/input artifacts | Repository, briefs, PDFs |
| Docked | Ship inactive but preserved | Archived marketing project |
| Distress Signal | Escalation/alert | Budget exceeded, policy violation |
| Rules of the Fleet | Fleet-level policy | Restricted data must remain local |

## 3.2 Naming guardrail

The pirate vocabulary should enrich product identity, but technical terms should remain visible in settings, APIs, logs, and documentation.

Example:

```text
Voyage (Workflow Run)
Job Order (Task)
Ship Log (Audit Timeline)
Treasure (Artifact)
```

This makes the interface approachable for users who enjoy the theme and understandable for professional/enterprise users.

---

# 4. Product Positioning

## 4.1 Product thesis

Galleon Fleet is a governed AI workforce platform organized as a fleet of specialized project workspaces.

It is not merely:

- A chatbot with named personas.
- A dashboard of random agents.
- A roleplay pirate interface.
- A generic autonomous-agent system.

It is:

```text
A fleet of constrained, specialized, observable AI teams
that work inside bounded Ships under human command.
```

## 4.2 Differentiator

The key product differentiation is the combination of:

```text
Fleet governance
+ Ship isolation
+ specialized crew roles
+ skills and tool boundaries
+ multi-agent coordination
+ approval-first execution
+ evidence/artifact-driven work
+ cost/provider governance
+ human executive control
```

## 4.3 Why Fleet matters

A single workspace model becomes difficult when a user manages multiple unrelated initiatives:

- Software product development.
- Marketing/SEO operations.
- Academic research.
- Content production.
- Server operations.
- Personal automation.

Without Fleet architecture, all memories, tools, budgets, reports, and agents can blend together. Fleet makes boundaries explicit while still enabling a human owner to receive an integrated overview.

---

# 5. Product Requirements Document

## 5.1 Problem statement

Users managing multiple projects need a way to coordinate AI agent work across separate workspaces without losing isolation, control, visibility, or decision authority.

A user should not need to open every project, read every raw log, manage every subtask, or inspect every agent action. At the same time, agents must not gain unrestricted cross-project access or autonomous authority over budget, policy, publication, deployment, or irreversible actions.

## 5.2 Target users

| User | Need |
|---|---|
| Solo builder / Pirate King | Oversees several technical, research, marketing, or operations projects |
| Project operator | Manages a Ship and needs focused agent teams |
| Technical lead | Needs delegated code/review/test workflows |
| Content/marketing lead | Needs research, SEO, drafting, and reporting workflows |
| Reviewer/approver | Needs concise decision briefs and exact approval payloads |
| Future team admin | Needs policy, budget, provider, and audit visibility |

## 5.3 Goals

1. Allow one Pirate King to own and manage multiple Ships.
2. Keep Ship data, policy, memory, provider, budget, and tool permissions isolated by default.
3. Add one Quartermaster agent that coordinates and summarizes at fleet scope.
4. Allow each Ship to have a Captain, Squads, and specialized Crew Members.
5. Allow objectives to be routed to one or more Ships through explicit Voyage proposals.
6. Provide concise Fleet Reports and escalation workflows.
7. Prevent privilege escalation from Quartermaster, Captain, Squad, Crew Member, or Skill.
8. Support cross-Ship artifact sharing only through explicit policy and provenance.
9. Make cost, risk, status, and approval queues visible at fleet level.
10. Preserve Go as the canonical orchestration and policy engine.

## 5.4 Non-goals

- Replace human strategy or executive judgment.
- Let Quartermaster autonomously alter global policy.
- Let Quartermaster publish, deploy, spend money, or access credentials directly.
- Share all memory automatically across Ships.
- Build a public marketplace before governance is mature.
- Make every Ship use the same tools/models/skills.
- Create unlimited agent hierarchies without budget/concurrency controls.

## 5.5 User stories

### Pirate King stories

- As a Pirate King, I want to see every Ship’s health, active voyages, budget, and blocked work in one dashboard.
- As a Pirate King, I want Quartermaster to summarize only decisions that require my attention.
- As a Pirate King, I want to create a Development Ship and a Marketing Ship with different permissions and models.
- As a Pirate King, I want to approve a cross-Ship transfer of an artifact before confidential content leaves a Ship.
- As a Pirate King, I want to pause a Ship or freeze its external actions during an incident.

### Quartermaster stories

- As Quartermaster, I want to receive status reports from Captains and produce a concise Fleet Report.
- As Quartermaster, I want to propose an objective route to relevant Ships without launching irreversible work.
- As Quartermaster, I want to flag budget, policy, provider, and dependency risks.
- As Quartermaster, I want to create decision briefs with options, trade-offs, and required approvals.

### Captain stories

- As a Captain, I want to receive a scoped Fleet Order with objective, constraints, inputs, and budget.
- As a Captain, I want to delegate Job Orders to my Ship’s Crew Members.
- As a Captain, I want to report completion, blockers, artifacts, and risk back to Quartermaster.

### Crew Member stories

- As a Crew Member, I want to receive only the skills, tools, memory scope, and budget needed for my Job Order.
- As a Crew Member, I want to create reviewable artifacts and request approval for consequential work.

## 5.6 Success metrics

| Metric | Target direction |
|---|---|
| Fleet report usefulness | Pirate King accepts/uses report with minimal correction |
| Escalation precision | Fewer non-actionable alerts; high coverage of real blockers |
| Ship isolation incidents | Zero unauthorized cross-Ship reads/writes |
| Approval clarity | High approval decision confidence; low reversal rate |
| Time to executive understanding | Lower than manual review of every Ship log |
| Voyage completion rate | Improve without increasing unsafe action rate |
| Budget variance | Actual spend stays within configured thresholds |
| Artifact handoff traceability | 100% of cross-Ship transfers have provenance/policy decision |
| Policy violation rate | Zero tolerated critical violations |

---

# 6. User Roles and Authority

## 6.1 Authority hierarchy

```text
Pirate King (Human final authority)
  ↓
Fleet Policy and Budget Boundaries
  ↓
Quartermaster (Fleet coordinator; bounded authority)
  ↓
Ship Captain (Project/workspace orchestrator)
  ↓
Squad Lead (Domain task coordinator)
  ↓
Crew Member (Specialized agent)
  ↓
Skill and Tool Runtime Constraints
```

## 6.2 Pirate King

### Responsibilities

- Defines strategic goals.
- Creates/archives Ships.
- Assigns fleet budget and top-level priorities.
- Approves global policy changes.
- Approves provider/model allowlists.
- Approves high-risk integrations, MCP servers, and credentials.
- Approves cross-Ship sharing for sensitive content.
- Approves irreversible actions: deploy, publish, send, purchase, migration, high-impact commit/push.
- Reviews Quartermaster decision briefs.

### Cannot delegate silently

The following actions must remain Pirate King/admin controlled unless an explicit future RBAC rule says otherwise:

```text
fleet policy update
fleet budget update
provider credential configuration
provider allowlist change
restricted data export
cross-Ship confidential artifact transfer
production deployment
external publishing
payment/purchase
fleet deletion
ship deletion with data purge
```

## 6.3 Quartermaster

### Role

Quartermaster is the fleet-level coordination assistant.

### Allowed actions by default

- Read permitted Ship summaries and report metadata.
- Read fleet budget summaries.
- Create Fleet Reports.
- Create decision briefs.
- Propose Fleet Orders and cross-Ship Voyage plans.
- Request status refresh from Captains.
- Create escalation events.
- Propose artifact transfer.
- Propose fleet lesson promotion.
- Suggest prioritization based on configured rules.

### Not allowed by default

- Modify Fleet Policy.
- Increase budget.
- Enable provider accounts.
- Read restricted Ship raw data.
- Access credentials.
- Apply code patches.
- Commit/push/deploy/publish/send.
- Approve its own requests.
- Override Captain/Ship policy.
- Auto-share artifacts outside permitted boundaries.

## 6.4 Captain

Captain is the per-Ship orchestrator. It receives a scoped objective and delegates to Squads/Crew Members. It owns the Ship-level plan but remains constrained by Ship/Fleet policies.

## 6.5 Squad Lead

Squad Lead coordinates work inside a functional domain. It cannot expand tool or model permissions beyond the Crew Member/Ship policy intersection.

## 6.6 Crew Member

Crew Members are specialist agents. They perform Job Orders with narrowly defined mission, skills, tool permissions, model route profile, memory policy, concurrency budget, and output contract.

---

# 7. Fleet Operating Model

## 7.1 Fleet topology

```text
Pirate King
  │
  └── Fleet
       │
       ├── Quartermaster
       │
       ├── Development Ship
       │    ├── Captain: Engineering Lead
       │    ├── Developer Squad
       │    └── QA / R&D Squads
       │
       ├── Marketing Ship
       │    ├── Captain: Marketing Lead
       │    ├── Research / SEO Squad
       │    └── Copywriting / Editorial Squad
       │
       ├── Research Ship
       │    ├── Captain: Research Lead
       │    └── Literature / Data / Review Squads
       │
       └── Operations Ship
            ├── Captain: Operations Lead
            └── Monitoring / Reliability / Automation Squads
```

## 7.2 Ship isolation

Each Ship is an isolation boundary for:

| Resource | Ship isolation rule |
|---|---|
| Workspace files | Read/write only within Ship scope |
| Memory | Local by default; explicit promotion/share required |
| Tools | Ship allowlist intersects Fleet policy |
| Provider/model profile | Ship-specific policy within Fleet ceiling |
| Budget | Ship allocation from Fleet budget |
| Artifacts | Ship-owned with access classification |
| Credentials | Referenced/scoped; no cross-Ship exposure |
| Audit logs | Ship-local; Fleet sees summary unless policy permits detail |
| MCP servers | Per-Ship enablement and capability allowlist |

## 7.3 Voyage lifecycle

```text
proposed
  → approved_for_planning
  → planned
  → running
  → waiting_for_input
  → waiting_for_approval
  → completed

planned | running | waiting_for_input | waiting_for_approval
  → failed
  → cancelled
  → interrupted

failed | interrupted
  → recovery_proposed
  → resumed | closed
```

A Voyage can be:

- Single-Ship.
- Cross-Ship with explicit dependency graph.
- Scheduled/read-only monitoring voyage.
- User-initiated ad hoc voyage.

## 7.4 Fleet Order lifecycle

```text
Pirate King objective
  ↓
Quartermaster intake
  ↓
Fleet Order proposal
  ↓
Pirate King approve / revise / reject
  ↓
Scoped Ship Order(s)
  ↓
Captain planning
  ↓
Voyage execution
  ↓
Ship Report(s)
  ↓
Quartermaster synthesis
  ↓
Fleet Report / Decision Brief
```

## 7.5 Cross-Ship coordination rules

Cross-Ship work must use explicit contracts:

- Objective contract.
- Input artifact references.
- Data classification.
- Allowed recipient Ships.
- Output expectations.
- Budget allocation.
- Dependency order.
- Approval requirements.

A Ship must not directly inspect another Ship’s raw workspace merely because both are in the same Fleet.

---

# 8. Crew and Squad Model

## 8.1 Developer Ship

```text
Development Ship
  ├── Engineering Lead / Captain
  ├── Product Owner
  ├── Backend Engineer
  ├── Frontend Engineer
  ├── QA Engineer
  └── R&D Engineer
```

| Member | Mission | Skills | Default tools | Default restrictions |
|---|---|---|---|---|
| Engineering Lead | Decompose work, coordinate, review | Architecture, delegation, integration review | Read repo, inspect artifacts, plan | No direct deploy/push by default |
| Product Owner | Define requirements and acceptance criteria | PRD, backlog, prioritization | Docs/read/report | No code mutation |
| Backend Engineer | APIs, domain, persistence, integration | Go, API, concurrency, schema | Read/search/test/draft patch | Apply patch requires approval |
| Frontend Engineer | UI, state, accessibility | TypeScript, React, UX state | Read/search/build/test/draft patch | Apply patch requires approval |
| QA Engineer | Test strategy and regression prevention | Test plan, contract test, failure triage | Read/search/lint/test/report | No write by default |
| R&D Engineer | Spikes, benchmarks, technology evaluation | Research, prototype, benchmark | Web/research/sandbox/report | Dependency adoption requires approval |

## 8.2 Marketing Ship

```text
Marketing Ship
  ├── Marketing Lead / Captain
  ├── Market Researcher
  ├── SEO Strategist
  ├── Copywriter
  ├── Content Editor
  ├── Growth Analyst
  └── Brand Strategist
```

| Member | Mission | Skills | Default tools | Default restrictions |
|---|---|---|---|---|
| Marketing Lead | Plan campaigns and prioritize work | Funnel, campaign, positioning | Research/report/plan | No external activation |
| Market Researcher | Find evidence about market/audience/competitors | Source evaluation, synthesis | Web search/fetch, RAG, report | Read-only |
| SEO Strategist | Find content opportunity and on-page issues | Keyword, intent, content gap | Crawl/read/analytics/report | CMS changes require approval |
| Copywriter | Draft content | Brand voice, article/draft workflow | Draft artifact writer | Publish denied by default |
| Content Editor | Check quality/evidence/style | Editorial review, claim validation | Read/review/report | No publish by default |
| Growth Analyst | Interpret performance data | Analytics, experiment design | Read analytics/report | No campaign change by default |
| Brand Strategist | Maintain positioning/messaging | Brand framework, creative brief | Research/draft/report | Public release needs approval |

## 8.3 Crew Member configuration

A Crew Member must be more than a prompt. It must resolve into:

```text
role
+ mission
+ skill references
+ tool policy reference
+ model route profile
+ memory policy reference
+ evaluation profile
+ budget ceiling
+ concurrency ceiling
+ approval policy
+ artifact output contract
```

## 8.4 Skill package format

```text
skills/
├── developer/
│   ├── go-concurrency-review/
│   ├── api-contract-review/
│   ├── test-strategy/
│   ├── regression-analysis/
│   └── frontend-accessibility/
├── marketing/
│   ├── competitor-research/
│   ├── keyword-clustering/
│   ├── search-intent-analysis/
│   ├── evidence-backed-copywriting/
│   ├── editorial-review/
│   └── website-growth-audit/
├── fleet/
│   ├── fleet-intake/
│   ├── executive-reporting/
│   ├── risk-escalation/
│   ├── budget-monitoring/
│   ├── artifact-handoff/
│   └── fleet-lesson-curation/
└── shared/
    ├── tool-safety/
    ├── citation-quality/
    ├── artifact-reporting/
    └── approval-aware-execution/
```

### Skill manifest example

```yaml
id: fleet-executive-reporting
version: 0.1.0
name: Fleet Executive Reporting
description: Consolidates approved Ship summaries into concise decision-oriented Fleet Reports.

mode: on_demand
eligible_roles:
  - quartermaster

required_capabilities:
  - fleet.read_summary
  - fleet.read_budget
  - artifact.read_summary
  - artifact.create_report

forbidden_tools:
  - workspace.apply_patch
  - provider.configure
  - policy.update
  - external.publish

inputs:
  - fleet_id
  - report_window
  - escalation_policy

outputs:
  - fleet_report
  - decision_brief
  - risk_register

evaluation:
  rubric: evals/fleet/executive-reporting.yaml
```

---

# 9. Quartermaster Responsibilities

## 9.1 Core responsibilities

| Responsibility | Description |
|---|---|
| Fleet intake | Interpret Pirate King objectives and identify affected Ships |
| Objective routing | Propose which Ship(s) should execute work |
| Portfolio visibility | Summarize Ship health, voyages, blockers, costs, risk, and approval state |
| Executive reporting | Turn Ship Reports into concise Fleet Reports |
| Escalation | Surface decisions, risks, policy issues, and budget exceptions |
| Cross-Ship coordination | Propose artifact handoff and dependency sequencing |
| Resource monitoring | Watch budget, concurrency, provider health, queue depth, and rate limits |
| Priority support | Recommend order based on impact, urgency, dependency, and configured priority rules |
| Lesson curation | Propose promotion of reusable lessons from Ship memory to Fleet Knowledge |
| Fleet hygiene | Identify stale voyages, abandoned approvals, inactive Ships, and degraded ports |

## 9.2 Quartermaster output types

```text
Fleet Report
Decision Brief
Risk Register
Fleet Order Proposal
Cross-Ship Voyage Proposal
Artifact Handoff Proposal
Budget Alert
Provider/Port Health Alert
Policy Escalation
Fleet Lesson Proposal
```

## 9.3 Decision brief format

```markdown
# Decision Brief

## Decision required
Approve a read-only GitHub MCP pilot for the Development Ship.

## Why now
The Developer Squad needs repository issue/PR context for codebase audit workflows.

## Options
1. Approve read-only GitHub MCP access for Development Ship only.
2. Keep current repository-local workflow without GitHub access.
3. Defer integration until Tool Runtime T3.

## Trade-offs
- Option 1: Better context, new external integration surface.
- Option 2: Lower risk, less complete repository intelligence.
- Option 3: Avoids immediate work but delays audit capability.

## Recommended option
Option 1 with read-only tool allowlist, no write operations, 30-day pilot, audit logging.

## Budget/risk
- Cost: low
- Security risk: medium, mitigated by read-only allowlist
- Approval required: Pirate King
```

## 9.4 Escalation policy

| Level | Meaning | Quartermaster action |
|---|---|---|
| `info` | Non-actionable status update | Include in digest only |
| `attention` | Needs monitoring/review | Flag in Fleet Report |
| `decision_required` | Strategic/approval decision needed | Create Decision Brief |
| `high_risk` | Security, budget, sensitive data, irreversible action | Immediate Pirate King alert; freeze action if policy says so |
| `critical` | Active violation/incident | Freeze affected route/voyage where authorized, alert Pirate King immediately |

## 9.5 Quartermaster permission profile

```yaml
role: quartermaster
mission: Coordinate fleet work, summarize status, surface decisions, and preserve governance.

allow:
  - fleet.read_summary
  - fleet.read_budget
  - fleet.read_policy_summary
  - ship.read_status
  - ship.request_status_report
  - voyage.create_proposal
  - artifact.read_summary
  - artifact.create_report
  - escalation.create
  - lesson.propose_promotion
  - artifact.transfer_proposal

deny:
  - fleet.policy.update
  - fleet.budget.update
  - provider.configure
  - credential.read
  - workspace.apply_patch
  - git.commit
  - git.push
  - deployment.deploy
  - cms.publish
  - email.send
  - approval.self_approve
```

---

# 10. System Design

## 10.1 Design goals

1. One Pirate King can govern many Ships.
2. Ships stay isolated by default.
3. Quartermaster sees summaries by default, not all raw data.
4. Cross-Ship transfer is explicit, auditable, and policy-filtered.
5. Fleet-level budget and policy constrain lower layers.
6. Every high-impact action remains human-approved.
7. Go owns state transitions, policy, event delivery, and audit logs.
8. Clients are projections of Go state, not independent authorities.

## 10.2 Logical components

```text
Fleet Command Domain
  ├── Fleet Service
  ├── Ship Service
  ├── Quartermaster Service
  ├── Voyage Service
  ├── Crew Directory
  ├── Skill Registry
  ├── Fleet Reporting Service
  ├── Escalation Service
  ├── Budget Allocation Service
  ├── Cross-Ship Handoff Service
  └── Fleet Knowledge Promotion Service

Existing/Core Domains
  ├── Crew Orchestrator
  ├── Tool Runtime
  ├── Model Gateway
  ├── Memory/RAG Service
  ├── Artifact Service
  ├── Approval Service
  ├── Event Store
  ├── Policy Service
  └── Audit Service
```

## 10.3 Core design rule: summary projection

Quartermaster operates primarily on **Ship Summary Projections**, not raw workspace data.

```text
Ship raw data
  → Ship policy filters/redacts
  → Ship Summary Projection
  → Quartermaster reads projection
  → Fleet Report
```

This minimizes accidental data leakage and keeps fleet reporting scalable.

## 10.4 Summary projection contents

```yaml
ship_summary:
  ship_id: ship_development
  status: active
  health: healthy
  active_voyages: 3
  blocked_voyages: 1
  approval_count: 2
  budget:
    allocated_usd: 20.00
    used_usd: 6.24
    estimated_remaining_usd: 13.76
  provider_health:
    healthy: 2
    degraded: 1
  highlights:
    - Phase 3 Tool Calling PRD complete
    - QA test matrix pending review
  blockers:
    - Decision required: approve MCP read-only pilot
  risks:
    - provider fallback rate above threshold
  artifacts:
    - artifact_id: artifact_abc
      title: Tool Calling Threat Model
      classification: internal
      shareable_to_fleet: true
  updated_at: timestamp
```

## 10.5 Cross-Ship artifact handoff

```text
Source Ship creates artifact
  ↓
Artifact classified and marked shareable/non-shareable
  ↓
Quartermaster or Captain creates handoff proposal
  ↓
Destination Ship and target use resolved
  ↓
Fleet/Ship policy evaluated
  ↓
Pirate King approval if required
  ↓
Redacted/copy/reference artifact transfer created
  ↓
Destination Captain receives scoped artifact reference
  ↓
Audit trail records source, destination, classification, policy, approval
```

The default action is **deny cross-Ship raw-data access**. Handoff can use:

- A reference if access policy permits.
- A redacted derivative artifact.
- A structured summary.
- A manual human handoff.

---

# 11. C4 Architecture

## 11.1 C4 Level 1 — System Context

```text
┌──────────────────────┐
│ Pirate King          │
│ Human Owner          │
└──────────┬───────────┘
           │ goals, approvals, policy decisions
           ▼
┌────────────────────────────────────────────────────────────┐
│ Galleon Fleet Fleet Command System                              │
│                                                            │
│ Coordinates Ships, Captains, Squads, Crew Members,         │
│ Voyages, reports, budget, policy, and approvals.           │
└───────┬──────────────────┬──────────────────┬──────────────┘
        │                  │                  │
        ▼                  ▼                  ▼
┌──────────────┐  ┌────────────────┐  ┌─────────────────────┐
│ AI Providers │  │ MCP / External │  │ Workspace / Data    │
│ Local/Cloud  │  │ Integrations   │  │ Repositories/docs   │
└──────────────┘  └────────────────┘  └─────────────────────┘
```

### External actors

| Actor/system | Relationship |
|---|---|
| Pirate King | Owns fleet, approves strategic actions, receives reports |
| TUI/Tauri/Web clients | Render command deck, collect approvals, configure fleet |
| AI providers | Provide model inference through Model Gateway |
| MCP servers | Provide approved tools/resources/prompts |
| Workspace/repositories | Ship-scoped data and code |
| External systems | CMS, GitHub, analytics, databases, only through governed tools |

## 11.2 C4 Level 2 — Containers

```text
┌───────────────────────────────────────────────────────────────────────────┐
│ Galleon Fleet System                                                          │
│                                                                           │
│  ┌────────────────────────────┐                                           │
│  │ Rust TUI / Tauri / Web UI  │                                           │
│  │ Command Deck + Ship Views  │                                           │
│  └──────────────┬─────────────┘                                           │
│                 │ REST/gRPC/SSE                                            │
│  ┌──────────────▼──────────────────────────────────────────────────────┐  │
│  │ Go Agent Engine                                                      │  │
│  │                                                                      │  │
│  │ Fleet Command  │ Crew Runtime │ Tool Runtime │ Model Gateway         │  │
│  │ Memory/RAG     │ Policy       │ Approval     │ Artifact/Audit         │  │
│  └───────┬───────────────────────────────────────────────┬─────────────┘  │
│          │                                               │                │
│  ┌───────▼────────────────┐                 ┌────────────▼────────────┐  │
│  │ Durable Data Plane      │                 │ Capability Plane         │  │
│  │ Postgres/Event Store    │                 │ Providers/MCP/Tools      │  │
│  │ Object/Artifact Store   │                 │ Sandboxed Executors      │  │
│  └────────────────────────┘                 └─────────────────────────┘  │
└───────────────────────────────────────────────────────────────────────────┘
```

## 11.3 C4 Level 3 — Go Engine components

```text
Go Agent Engine
│
├── Fleet Command Module
│   ├── Fleet Service
│   ├── Ship Service
│   ├── Quartermaster Service
│   ├── Fleet Report Service
│   ├── Fleet Order Service
│   ├── Cross-Ship Handoff Service
│   ├── Budget Allocation Service
│   └── Escalation Service
│
├── Crew Runtime Module
│   ├── Captain Runtime
│   ├── Squad Coordinator
│   ├── Crew Member Runtime
│   ├── Job Order Scheduler
│   └── Voyage State Machine
│
├── Skill Module
│   ├── Skill Registry
│   ├── Skill Resolver
│   ├── Skill Evaluation
│   └── Skill Versioning
│
├── Tool Runtime Module
│   ├── Tool Registry
│   ├── Policy Evaluator
│   ├── Approval Manager
│   ├── Sandbox Executor
│   ├── MCP Client
│   ├── Output Sanitizer
│   └── Audit Writer
│
├── Model Gateway Module
│   ├── Provider Adapters
│   ├── Route Policy
│   ├── Capability Filter
│   ├── Budget Guard
│   ├── Usage Ledger
│   └── Circuit Breaker
│
├── Knowledge Module
│   ├── Ship Memory
│   ├── Fleet Knowledge
│   ├── Artifact Index
│   ├── Retrieval Service
│   └── Lesson Promotion
│
└── Platform Core
    ├── Config
    ├── Secrets
    ├── Events
    ├── Metrics
    ├── Logging
    ├── Errors
    └── Auth/RBAC
```

## 11.4 C4 Level 4 — Quartermaster interaction sequence

```text
Pirate King          UI            Go Fleet API       Quartermaster       Ship Captain       Crew Runtime
     │               │                  │                   │                  │                  │
     │ Objective     │                  │                   │                  │                  │
     ├──────────────>│                  │                   │                  │                  │
     │               ├─────────────────>│                   │                  │                  │
     │               │                  ├──────────────────>│                  │                  │
     │               │                  │ Fleet intake      │                  │                  │
     │               │                  │                   ├─────────────────>│                  │
     │               │                  │                   │ request summary  │                  │
     │               │                  │                   │<─────────────────┤                  │
     │               │                  │                   │ create proposal  │                  │
     │               │<─────────────────┤                   │                  │                  │
     │ Approve plan  │                  │                   │                  │                  │
     ├──────────────>│                  │                   │                  │                  │
     │               ├─────────────────>│                   │                  │                  │
     │               │                  ├──────────────────>│                  │                  │
     │               │                  │                   ├─────────────────>│                  │
     │               │                  │                   │ scoped ship order│                  │
     │               │                  │                   │                  ├─────────────────>│
     │               │                  │                   │                  │ execute voyage  │
     │               │                  │                   │                  │<────────────────┤
     │               │                  │                   │                  │ Ship report     │
     │               │                  │                   │<─────────────────┤                  │
     │               │                  │<──────────────────┤ Fleet report     │                  │
     │<──────────────┤                  │                   │                  │                  │
```

---

# 12. Technical Specification

## 12.1 Go module layout

```text
engine/
├── app/
├── cmd/agent-engine/
├── core/
│   ├── config/
│   ├── errors/
│   ├── events/
│   ├── logger/
│   ├── metrics/
│   ├── policy/
│   ├── secrets/
│   ├── auth/
│   └── persistence/
├── internal/
│   ├── fleet/
│   │   ├── domain/
│   │   ├── application/
│   │   ├── infrastructure/
│   │   └── delivery/
│   ├── ships/
│   ├── crew/
│   ├── skills/
│   ├── voyages/
│   ├── reporting/
│   ├── escalation/
│   ├── handoff/
│   ├── budget/
│   ├── tools/
│   ├── modelgateway/
│   ├── memory/
│   └── artifacts/
├── proto/
│   ├── fleet.proto
│   ├── ship.proto
│   ├── crew.proto
│   ├── voyage.proto
│   ├── report.proto
│   └── handoff.proto
└── pkg/api/
```

## 12.2 Core interfaces

```go
package fleet

type FleetService interface {
    CreateFleet(ctx context.Context, cmd CreateFleetCommand) (Fleet, error)
    GetFleet(ctx context.Context, fleetID string) (Fleet, error)
    ListFleetSummaries(ctx context.Context, actor Actor) ([]FleetSummary, error)
    CreateFleetOrderProposal(ctx context.Context, cmd CreateFleetOrderProposalCommand) (FleetOrderProposal, error)
}

type QuartermasterService interface {
    IntakeObjective(ctx context.Context, req FleetObjective) (FleetOrderProposal, error)
    BuildFleetReport(ctx context.Context, fleetID string, window ReportWindow) (FleetReport, error)
    CreateDecisionBrief(ctx context.Context, req DecisionBriefRequest) (DecisionBrief, error)
    Escalate(ctx context.Context, req EscalationRequest) (Escalation, error)
}

type ShipService interface {
    CreateShip(ctx context.Context, cmd CreateShipCommand) (Ship, error)
    GetShipSummary(ctx context.Context, shipID string, actor Actor) (ShipSummary, error)
    SubmitShipReport(ctx context.Context, report ShipReport) error
    SetShipStatus(ctx context.Context, shipID string, status ShipStatus) error
}

type ArtifactHandoffService interface {
    Propose(ctx context.Context, cmd CreateHandoffProposalCommand) (HandoffProposal, error)
    Approve(ctx context.Context, approval ApprovalContext) (HandoffReceipt, error)
    Deny(ctx context.Context, approval ApprovalContext) error
}
```

## 12.3 Go concurrency rules

- Every Fleet Order, Voyage, Job Order, tool execution, and report aggregation inherits `context.Context` from its parent.
- Quartermaster report generation may run concurrently across Ships, but must use bounded worker pools.
- Fleet summary collection uses timeouts per Ship so one unhealthy Ship does not block the entire report.
- A Quartermaster must not hold a database/aggregate lock while calling a Ship, provider, MCP server, or tool.
- State transitions must be atomic and idempotent.
- Fleet Report creation must be reproducible from report inputs and persisted event references.

## 12.4 Status models

### Fleet status

```text
active
healthy
degraded
attention_required
paused
archived
```

### Ship status

```text
active
ready
working
waiting_for_approval
blocked
degraded
paused
docked
archived
```

### Crew member status

```text
idle
planning
working
waiting_for_input
waiting_for_approval
blocked
offline
paused
```

### Quartermaster status

```text
available
collecting_reports
building_brief
waiting_for_pirate_king
escalating
paused
```

## 12.5 Effective permission formula

```text
Effective Capability Set =
  Fleet Policy Ceiling
  ∩ Ship Policy
  ∩ Squad Policy
  ∩ Crew Member Policy
  ∩ Skill Policy
  ∩ Job Order Restriction
  ∩ Runtime Context Restriction
```

No lower layer can add a capability denied by a higher layer.

## 12.6 Memory boundaries

```text
Fleet Knowledge
  - Only curated, approved, shareable lessons/policies/templates

Ship Memory
  - Project-specific facts, artifacts, conventions, lessons

Voyage Memory
  - Active run/task context and checkpoints

Crew Member Working Memory
  - Short-lived context for current Job Order
```

Promotion flow:

```text
Crew/Ship lesson
  → Ship Lead/Captain review
  → Quartermaster fleet lesson proposal
  → Pirate King approval where policy requires
  → Fleet Knowledge
```

---

# 13. Data Model

## 13.1 Core entities

```go
type PirateKing struct {
    ID        string
    Name      string
    CreatedAt time.Time
    UpdatedAt time.Time
}

type Fleet struct {
    ID              string
    PirateKingID    string
    Name            string
    Description     string
    QuartermasterID string
    PolicyRef       string
    BudgetRef       string
    Status          FleetStatus
    CreatedAt       time.Time
    UpdatedAt       time.Time
}

type Quartermaster struct {
    ID                 string
    FleetID            string
    DisplayName        string
    Codename           string
    Mission            string
    SkillRefs          []SkillRef
    ToolPolicyRef      string
    ModelProfileRef    string
    MemoryPolicyRef    string
    EscalationPolicyRef string
    MaxConcurrency     int
    Enabled            bool
    Version            int64
}

type Ship struct {
    ID                string
    FleetID           string
    Name              string
    Domain            ShipDomain
    Description       string
    CaptainMemberID   string
    WorkspaceRef      string
    PolicyRef         string
    BudgetRef         string
    MemoryScopeRef    string
    Status            ShipStatus
    Version           int64
    CreatedAt         time.Time
    UpdatedAt         time.Time
}

type Squad struct {
    ID                    string
    ShipID                string
    Name                  string
    LeadMemberID          string
    DefaultToolPolicyRef  string
    DefaultModelProfileRef string
    Status                SquadStatus
}

type CrewMember struct {
    ID                 string
    ShipID             string
    SquadID            string
    RoleID             string
    DisplayName        string
    Mission            string
    Status             CrewMemberStatus
    SkillRefs          []SkillRef
    ToolPolicyRef      string
    ModelProfileRef    string
    MemoryPolicyRef    string
    EvaluationProfile  string
    BudgetCeiling      BudgetLimit
    MaxConcurrency     int
    Enabled            bool
    Version            int64
}
```

## 13.2 Work entities

```go
type FleetOrderProposal struct {
    ID               string
    FleetID          string
    RequestedBy      string
    Objective        string
    ProposedShipIDs  []string
    Dependencies     []ShipDependency
    BudgetEstimate   BudgetEstimate
    RiskSummary      RiskSummary
    Status           ProposalStatus
    CreatedAt        time.Time
}

type Voyage struct {
    ID               string
    FleetID          string
    ShipID           string
    ParentVoyageID   *string
    FleetOrderID     *string
    Name             string
    Objective        string
    Status           VoyageStatus
    Budget           BudgetLimit
    PolicySnapshotID string
    StartedAt        *time.Time
    CompletedAt      *time.Time
}

type JobOrder struct {
    ID                 string
    VoyageID           string
    ParentJobOrderID   *string
    AssignedMemberID   string
    Objective          string
    InputArtifacts     []ArtifactRef
    RequiredSkills     []string
    RequiredTools      []string
    Status             JobOrderStatus
    Budget             BudgetLimit
    ApprovalPolicyRef  string
    OutputContractRef  string
}
```

## 13.3 Reporting entities

```go
type ShipReport struct {
    ID              string
    ShipID          string
    VoyageID        *string
    Status          ReportStatus
    Summary         string
    Highlights      []string
    Blockers        []Blocker
    Risks           []RiskItem
    BudgetSnapshot  BudgetSnapshot
    ArtifactRefs    []ArtifactRef
    EscalationRefs  []string
    CreatedAt       time.Time
}

type FleetReport struct {
    ID               string
    FleetID          string
    WindowStart      time.Time
    WindowEnd        time.Time
    OverallStatus    FleetStatus
    ExecutiveSummary string
    ShipSummaries    []ShipSummary
    DecisionsNeeded  []DecisionItem
    TopRisks         []RiskItem
    BudgetSnapshot   BudgetSnapshot
    ArtifactRefs     []ArtifactRef
    CreatedAt        time.Time
}

type Escalation struct {
    ID              string
    FleetID         string
    ShipID          *string
    VoyageID        *string
    Severity        EscalationSeverity
    Category        EscalationCategory
    Summary         string
    RecommendedAction string
    Status          EscalationStatus
    CreatedAt       time.Time
}
```

## 13.4 Persistence tables

```text
pirate_kings
fleets
quartermasters
fleet_policies
fleet_budget_limits
ships
ship_policies
ship_budget_limits
squads
crew_members
role_templates
skills
skill_versions
crew_member_skills
fleet_orders
fleet_order_ship_links
voyages
job_orders
ship_reports
fleet_reports
escalations
artifact_handoff_proposals
artifact_handoff_receipts
fleet_knowledge_entries
lesson_promotion_proposals
audit_events
approval_requests
```

## 13.5 Key relationships

```text
PirateKing 1 ─── * Fleet
Fleet      1 ─── 1 Quartermaster
Fleet      1 ─── * Ship
Ship       1 ─── * Squad
Squad      1 ─── * CrewMember
Ship       1 ─── * Voyage
Voyage     1 ─── * JobOrder
Ship       * ─── * Artifact via ownership/access policy
Fleet      1 ─── * FleetReport
Ship       1 ─── * ShipReport
```

---

# 14. Policy, Security, and Governance

## 14.1 Fleet policy categories

```text
Data classification policy
Tool capability ceiling
Provider/model allowlist
Budget policy
Approval policy
Cross-Ship sharing policy
Artifact retention policy
MCP integration policy
Audit retention policy
Emergency freeze policy
```

## 14.2 Data classification

```text
public
internal
confidential
restricted
```

### Cross-Ship default policy

| Classification | Default handoff behavior |
|---|---|
| Public | Allow if destination Ship allows it |
| Internal | Require source/destination policy match |
| Confidential | Require explicit handoff proposal and approval |
| Restricted | Deny by default; only dedicated approved route |

## 14.3 Budget hierarchy

```text
Fleet total budget
  ↓
Ship allocation
  ↓
Squad allocation
  ↓
Voyage budget
  ↓
Job Order budget
  ↓
Tool/model request limits
```

Budget can only narrow downward. A Captain cannot increase Ship allocation. A Quartermaster can recommend reallocation but needs Pirate King approval for increase/rebalance beyond configured limits.

## 14.4 Emergency freeze

Pirate King can initiate:

```text
Freeze Fleet
Freeze Ship
Freeze tool risk class
Freeze provider route
Freeze MCP server
Freeze external actions
```

Quartermaster can propose or, if explicitly authorized by emergency policy, temporarily freeze a narrowly scoped dangerous route such as a degraded provider or policy-violating MCP server. It cannot unfreeze that route without policy/user authority.

## 14.5 Audit rules

Every cross-Ship action requires audit fields:

```text
source_fleet_id
source_ship_id
destination_ship_id
artifact_id or summary_id
classification
policy_version
handoff_mode
approval_id if required
actor_id
quartermaster involvement flag
created_at
```

---

# 15. API Contracts

## 15.1 Fleet APIs

```http
GET    /api/v1/fleets
POST   /api/v1/fleets
GET    /api/v1/fleets/{fleet_id}
PATCH  /api/v1/fleets/{fleet_id}
POST   /api/v1/fleets/{fleet_id}/pause
POST   /api/v1/fleets/{fleet_id}/resume
GET    /api/v1/fleets/{fleet_id}/command-deck
```

### Create fleet

```http
POST /api/v1/fleets
Content-Type: application/json
```

```json
{
  "name": "Personal Product Fleet",
  "description": "Development, marketing, research, and operations ships.",
  "quartermaster": {
    "display_name": "Quartermaster",
    "codename": "Quarterclaw",
    "model_profile": "fleet-balanced"
  }
}
```

## 15.2 Ship APIs

```http
GET    /api/v1/fleets/{fleet_id}/ships
POST   /api/v1/fleets/{fleet_id}/ships
GET    /api/v1/ships/{ship_id}
PATCH  /api/v1/ships/{ship_id}
POST   /api/v1/ships/{ship_id}/dock
POST   /api/v1/ships/{ship_id}/activate
POST   /api/v1/ships/{ship_id}/pause
GET    /api/v1/ships/{ship_id}/summary
```

### Create ship

```json
{
  "name": "Development Ship",
  "domain": "development",
  "workspace_ref": "workspace://galleon-fleet",
  "captain_template": "engineering-lead",
  "budget": {
    "currency": "USD",
    "soft_limit": 10.0,
    "hard_limit": 20.0
  },
  "policy_profile": "development-standard"
}
```

## 15.3 Quartermaster APIs

```http
GET  /api/v1/fleets/{fleet_id}/quartermaster
POST /api/v1/fleets/{fleet_id}/quartermaster/intake
POST /api/v1/fleets/{fleet_id}/quartermaster/reports
POST /api/v1/fleets/{fleet_id}/quartermaster/decision-briefs
GET  /api/v1/fleets/{fleet_id}/quartermaster/queue
```

### Submit strategic objective

```http
POST /api/v1/fleets/{fleet_id}/quartermaster/intake
```

```json
{
  "objective": "Prepare Phase 3 release plan covering tool calling, provider routing, and Crew Members.",
  "priority": "high",
  "constraints": {
    "max_budget_usd": 15.0,
    "deadline": "2026-10-10T00:00:00Z",
    "data_classification": "internal"
  }
}
```

Response:

```json
{
  "proposal_id": "fleetproposal_01J...",
  "status": "awaiting_pirate_king_approval",
  "proposed_ships": [
    "ship_development",
    "ship_research"
  ],
  "summary": "Development Ship will design implementation; Research Ship will validate patterns and risks.",
  "budget_estimate_usd": 8.5
}
```

## 15.4 Fleet order APIs

```http
GET  /api/v1/fleet-orders/{fleet_order_id}
POST /api/v1/fleet-orders/{fleet_order_id}/approve
POST /api/v1/fleet-orders/{fleet_order_id}/reject
POST /api/v1/fleet-orders/{fleet_order_id}/revise
```

## 15.5 Crew Member APIs

```http
GET    /api/v1/ships/{ship_id}/crew-members
POST   /api/v1/ships/{ship_id}/crew-members
GET    /api/v1/crew-members/{member_id}
PATCH  /api/v1/crew-members/{member_id}
POST   /api/v1/crew-members/{member_id}/pause
POST   /api/v1/crew-members/{member_id}/resume
GET    /api/v1/crew-members/{member_id}/activity
GET    /api/v1/crew-members/{member_id}/performance
```

## 15.6 Reports and escalation APIs

```http
GET  /api/v1/fleets/{fleet_id}/reports
POST /api/v1/fleets/{fleet_id}/reports/generate
GET  /api/v1/fleets/{fleet_id}/escalations
POST /api/v1/escalations/{escalation_id}/acknowledge
POST /api/v1/escalations/{escalation_id}/resolve
```

## 15.7 Cross-Ship handoff APIs

```http
POST /api/v1/artifact-handoffs
GET  /api/v1/artifact-handoffs/{handoff_id}
POST /api/v1/artifact-handoffs/{handoff_id}/approve
POST /api/v1/artifact-handoffs/{handoff_id}/deny
```

### Handoff proposal

```json
{
  "source_ship_id": "ship_research",
  "destination_ship_id": "ship_marketing",
  "artifact_id": "artifact_market_research_01J",
  "handoff_mode": "redacted_summary",
  "purpose": "Use approved competitor research to inform content brief.",
  "classification": "internal"
}
```

## 15.8 Error envelope

```json
{
  "error": {
    "code": "CROSS_SHIP_POLICY_DENIED",
    "message": "The artifact classification does not permit transfer to the requested Ship.",
    "request_id": "req_01J...",
    "retryable": false,
    "details": {
      "source_ship_id": "ship_research",
      "destination_ship_id": "ship_marketing"
    }
  }
}
```

---

# 16. Event Contracts

## 16.1 Fleet events

```text
fleet.created
fleet.updated
fleet.paused
fleet.resumed
fleet.policy_changed
fleet.budget_threshold_reached
fleet.report_created
fleet.decision_required
fleet.escalation_created
```

## 16.2 Ship events

```text
ship.created
ship.activated
ship.docked
ship.paused
ship.status_changed
ship.report_submitted
ship.budget_threshold_reached
ship.provider_degraded
ship.blocked
```

## 16.3 Quartermaster events

```text
quartermaster.intake_received
quartermaster.fleet_order_proposed
quartermaster.report_collection_started
quartermaster.report_collection_completed
quartermaster.fleet_report_created
quartermaster.decision_brief_created
quartermaster.escalation_created
quartermaster.handoff_proposed
quartermaster.lesson_promotion_proposed
```

## 16.4 Crew events

```text
crew_member.created
crew_member.status_changed
crew_member.job_assigned
crew_member.job_completed
crew_member.blocked
crew_member.paused
```

## 16.5 Event payload example

```json
{
  "event_id": "evt_01J...",
  "sequence": 84,
  "fleet_id": "fleet_01J...",
  "ship_id": "ship_development",
  "voyage_id": "voyage_01J...",
  "type": "ship.report_submitted",
  "occurred_at": "2026-09-28T11:28:00Z",
  "correlation_id": "corr_01J...",
  "data": {
    "report_id": "shipreport_01J...",
    "status": "attention_required",
    "summary": "QA matrix complete; MCP integration decision is blocking implementation."
  }
}
```

---

# 17. UI and UX Design

## 17.1 Navigation structure

```text
Fleet Command
├── Command Deck
├── Ships
├── Quartermaster
├── Voyages
├── Crew Members
├── Skills
├── Approvals
├── Fleet Budget
├── Fleet Knowledge
├── Ports & Providers
├── Policies
└── Ship Logs
```

## 17.2 Command Deck

```text
Pirate King Command Deck

Fleet Health: Stable
Ships: 4 active / 1 docked
Active Voyages: 6
Crew Working: 13
Awaiting Approvals: 2
Budget: 38% daily / 24% monthly
Provider Health: 3 healthy / 1 degraded

Quartermaster Brief
- Development Ship needs a decision on an MCP pilot.
- Marketing Ship has two drafts ready for review.
- Research Ship completed provider-routing evidence collection.
- One model route crossed a soft budget threshold.

[View Decision Briefs] [Open Approval Queue] [View All Ships]
```

## 17.3 Quartermaster page

Sections:

1. Current mission and status.
2. Fleet priority queue.
3. Report collection status.
4. Decision briefs.
5. Escalations.
6. Cross-Ship handoff proposals.
7. Budget/watch alerts.
8. Fleet lesson proposals.
9. Recent actions/audit summary.

## 17.4 Ship card

```text
Development Ship
Status: Working
Captain: Engineering Lead
Crew: 6 members
Active Voyages: 2
Blocked: 1
Budget: $6.24 / $20.00
Ports: GitHub (pending), Local Ollama (healthy), OpenRouter (healthy)

Latest report:
Tool Calling threat-model complete. MCP pilot decision required.

[Open Ship] [View Report] [Pause Ship]
```

## 17.5 Crew Members submenu

```text
Crew Members
├── Overview
├── Squads
├── Member Directory
├── Role Templates
├── Skills
├── Tool Permissions
├── Model Profiles
├── Performance
└── Activity
```

## 17.6 Approval UX

Fleet-level approvals must be clearly distinct from Ship-level approvals:

```text
Fleet Approval Required

Action: Transfer a confidential research artifact from Research Ship to Marketing Ship
Requested by: Quartermaster
Purpose: Create evidence-backed content brief
Transfer mode: Redacted summary only
Policy: fleet-data-sharing-v1

[View Source Summary] [Approve Transfer] [Deny]
```

## 17.7 Visual style guidance

- Use clean information hierarchy first.
- Use pirate language in labels/empty states/illustration, not in every technical field.
- Show clear status color semantics.
- Avoid decorative overload such as excessive skulls, maps, or cartoon elements in operational screens.
- Use subtle nautical visual cues: ship icons, compass indicators, port markers, logbook cards.
- Make professional mode possible through terminology settings if needed.

---

# 18. Operational Flows

## 18.1 Flow A — Pirate King creates a Ship

```text
Pirate King
  → creates Fleet or opens existing Fleet
  → selects Create Ship
  → chooses Ship template (Development/Marketing/Research/Operations)
  → assigns workspace reference
  → chooses Captain role template
  → sets budget and policy profile
  → reviews Crew Member template composition
  → activates Ship

Go Engine
  → validates Fleet policy ceiling
  → creates Ship, Captain, Squads, Crew Members
  → initializes isolated memory scope
  → initializes budget ledger and Ship Log
  → emits ship.created
```

## 18.2 Flow B — Strategic objective routing

```text
Pirate King objective:
"Prepare a launch plan for a new website feature."

Quartermaster
  → classifies objective
  → gathers permitted Ship summaries
  → proposes Development Ship + Marketing Ship participation
  → defines dependency: Development evidence first, Marketing messaging second
  → estimates budget/risk
  → creates Fleet Order proposal

Pirate King
  → approves/revises/rejects

Quartermaster
  → sends scoped Ship Orders to Captains

Captains
  → plan Voyages and delegate Job Orders
```

## 18.3 Flow C — Ship reporting

```text
Captain
  → collects voyage/task/artifact statuses
  → produces Ship Report
  → submits report

Ship policy
  → filters/redacts report for Fleet scope

Quartermaster
  → collects Ship Summary Projections
  → identifies blockers and budget issues
  → generates Fleet Report
  → creates Decision Brief if needed

Pirate King
  → reviews concise report and resolves required decisions
```

## 18.4 Flow D — Cross-Ship handoff

```text
Research Ship produces competitor research artifact
  ↓
Marketing Captain needs result for content brief
  ↓
Quartermaster creates Handoff Proposal
  ↓
Policy checks classification, sharing rule, destination scope
  ↓
Approval required if classification is confidential/restricted
  ↓
Pirate King approves exact transfer mode
  ↓
Go engine creates redacted summary/reference receipt
  ↓
Marketing Ship receives allowed artifact reference
  ↓
Audit event recorded
```

## 18.5 Flow E — Budget escalation

```text
Voyage approaches Ship soft limit
  ↓
Budget service emits threshold event
  ↓
Captain receives warning and can reduce scope/request adjustment
  ↓
Quartermaster includes issue in Fleet Report
  ↓
If hard limit reached: engine pauses further paid provider/tool calls
  ↓
Pirate King can approve additional allocation or revise scope
```

## 18.6 Flow F — Emergency freeze

```text
MCP server produces suspicious output or policy violation
  ↓
Tool Runtime emits high-risk event
  ↓
Ship is marked degraded
  ↓
Quartermaster creates critical escalation
  ↓
Configured emergency policy may disable that MCP server route
  ↓
Pirate King receives immediate alert
  ↓
Investigation / approval required to re-enable
```

---

# 19. Implementation Roadmap

## Fleet Phase F0 — Fleet foundation

**Goal:** Represent Pirate King, Fleet, Quartermaster, and Ship boundaries without changing agent execution deeply.

### Deliverables

- Fleet, Pirate King, Quartermaster, Ship entities.
- Fleet/Ship status models.
- Command Deck read-only UI.
- Ship summary projection.
- Fleet Report generated from Ship summaries.
- Initial Developer/Marketing Ship templates.
- Basic audit events.

### Exit criteria

- One Pirate King can own multiple Ships.
- Quartermaster can produce read-only Fleet Report.
- No raw cross-Ship workspace access occurs.

## Fleet Phase F1 — Crew Members and Squad runtime configuration

**Goal:** Make Crew Members real runtime configurations rather than persona cards.

### Deliverables

- Squad entity.
- Crew Member entity.
- Role templates.
- Skill references.
- Tool/model/memory/evaluation policy references.
- Member status/activity UI.
- Developer Squad template.
- Marketing Squad template.

### Exit criteria

- Crew Member configuration resolves into effective policy.
- Member cannot exceed Ship/Fleet policy ceiling.
- UI shows role, skills, tools, model profile, and status.

## Fleet Phase F2 — Fleet Orders and Voyage coordination

**Goal:** Let Quartermaster route strategic objectives across Ships.

### Deliverables

- Fleet Order proposal and approval flow.
- Scoped Ship Orders.
- Cross-Ship dependency graph.
- Captain acceptance/planning workflow.
- Voyage linking to Fleet Order.
- Decision Brief UI.

### Exit criteria

- Pirate King can approve a multi-Ship plan.
- Each Ship receives only scoped objective/input/budget.
- Quartermaster cannot directly force unsafe execution.

## Fleet Phase F3 — Handoff, budgets, and governance

**Goal:** Add safe fleet-level resource coordination.

### Deliverables

- Fleet/Ship budget hierarchy.
- Artifact handoff proposal/approval/receipt.
- Cross-Ship classification policy.
- Fleet provider/model ceilings.
- Approval queue.
- Emergency freeze mechanism.
- Budget escalation.

### Exit criteria

- Cross-Ship artifact transfer is explicit/audited.
- Budget cannot silently exceed Fleet hard limit.
- Freeze action blocks affected route/voyage safely.

## Fleet Phase F4 — Fleet knowledge and evaluation

**Goal:** Make the fleet improve without contaminating project memory.

### Deliverables

- Fleet Knowledge store.
- Lesson promotion proposal workflow.
- Skill evaluation scorecards.
- Role performance scorecards.
- Recommended squad composition based on historical data.
- Canary skill/model profile rollout.

### Exit criteria

- Ship memory remains isolated by default.
- Fleet knowledge promotion is reviewed and provenance-backed.
- New skills/templates have measurable quality gates.

---

# 20. Task Breakdown

## F0.1 — Domain and persistence

- [ ] Define `PirateKing` entity.
- [ ] Define `Fleet` entity.
- [ ] Define `Quartermaster` entity.
- [ ] Define `Ship` entity.
- [ ] Create persistence schema/migrations.
- [ ] Add Fleet/Ship status enums.
- [ ] Add Fleet/Ship repository interfaces.
- [ ] Add event types.
- [ ] Add read-only Fleet summary projection.

## F0.2 — Fleet services

- [ ] Create Fleet service.
- [ ] Create Ship service.
- [ ] Create Quartermaster report service.
- [ ] Implement Ship Report submission.
- [ ] Implement Fleet Report aggregation.
- [ ] Add report window filtering.
- [ ] Add summary redaction filter.
- [ ] Add Command Deck API.

## F0.3 — UI foundation

- [ ] Add Fleet Command navigation.
- [ ] Add Command Deck page.
- [ ] Add Ships list page.
- [ ] Add Ship detail read-only page.
- [ ] Add Quartermaster page.
- [ ] Add Fleet report card and timeline.
- [ ] Add state handling for active/degraded/paused/docked.

## F1.1 — Crew organization

- [ ] Define Squad entity.
- [ ] Define Crew Member entity.
- [ ] Define Role Template entity.
- [ ] Define Crew Member status lifecycle.
- [ ] Add Developer Squad template.
- [ ] Add Marketing Squad template.
- [ ] Add member activity view.
- [ ] Add member pause/resume.

## F1.2 — Skills/policy linkage

- [ ] Create Skill reference model.
- [ ] Add Crew Member → Skill association.
- [ ] Add Crew Member → Tool policy association.
- [ ] Add Crew Member → Model profile association.
- [ ] Add Crew Member → Memory policy association.
- [ ] Add effective-policy resolver using intersection semantics.
- [ ] Add UI tool/skill permission preview.

## F2.1 — Fleet orders

- [ ] Define Fleet Order proposal entity.
- [ ] Define Ship Order entity.
- [ ] Implement Quartermaster intake service.
- [ ] Implement proposed Ship routing.
- [ ] Implement budget/risk estimate placeholder.
- [ ] Implement Pirate King approve/reject/revise actions.
- [ ] Implement Captain order acceptance.
- [ ] Link Ship Voyage to Fleet Order.

## F2.2 — Decision briefs and escalation

- [ ] Define Decision Brief entity.
- [ ] Define Escalation entity.
- [ ] Implement severity classification.
- [ ] Implement Quartermaster decision brief generation.
- [ ] Implement notification/event stream integration.
- [ ] Implement acknowledge/resolve flow.
- [ ] Add fleet approval queue UI.

## F3.1 — Budget hierarchy

- [ ] Define Fleet budget limit.
- [ ] Define Ship budget allocation.
- [ ] Define Voyage/Job budget limit.
- [ ] Implement soft/hard threshold events.
- [ ] Implement budget pause behavior.
- [ ] Implement Quartermaster budget reporting.
- [ ] Implement Pirate King allocation approval.

## F3.2 — Artifact handoff

- [ ] Define artifact shareability/classification fields.
- [ ] Define Handoff Proposal/Receipt entities.
- [ ] Implement source/destination policy checks.
- [ ] Implement reference/redacted-summary/copy modes.
- [ ] Implement approval binding.
- [ ] Add handoff audit events.
- [ ] Add Handoff proposal UI.

## F3.3 — Fleet governance controls

- [ ] Fleet-level provider/model allowlist.
- [ ] Fleet-level MCP allowlist.
- [ ] Fleet-level tool risk ceiling.
- [ ] Ship policy inheritance enforcement.
- [ ] Emergency freeze route.
- [ ] Freeze/unfreeze approval policy.
- [ ] Incident/escalation integration.

## F4.1 — Knowledge promotion

- [ ] Define Fleet Knowledge entity.
- [ ] Define Lesson Promotion Proposal.
- [ ] Add Ship lesson source provenance.
- [ ] Add Pirate King approval flow.
- [ ] Add fleet memory retrieval policy.
- [ ] Add restricted-data protection.

## F4.2 — Quality and performance

- [ ] Define role evaluation profile.
- [ ] Collect performance metrics per role/skill.
- [ ] Add human acceptance/rejection signal.
- [ ] Add skill version regression suite.
- [ ] Add suggested squad composition.
- [ ] Add canary rollout for skill/model profile.

---

# 21. Testing and Quality Strategy

## 21.1 Unit tests

- [ ] Fleet/Ship status state-machine tests.
- [ ] Effective permission intersection tests.
- [ ] Fleet budget allocation tests.
- [ ] Cross-Ship classification policy tests.
- [ ] Summary redaction tests.
- [ ] Handoff approval binding tests.
- [ ] Decision Brief formatting/required-field tests.
- [ ] Escalation severity routing tests.
- [ ] Fleet Report aggregation ordering tests.

## 21.2 Integration tests

- [ ] Pirate King creates Fleet and multiple Ships.
- [ ] Quartermaster receives Ship summaries but not restricted raw data.
- [ ] Fleet Order routes to correct Ship.
- [ ] Captain receives scoped order only.
- [ ] Cross-Ship artifact handoff denied without policy/approval.
- [ ] Approved redacted handoff succeeds and records receipt.
- [ ] Budget hard limit pauses new paid action.
- [ ] Emergency freeze disables affected route.
- [ ] Fleet report handles one unavailable Ship without failing entire report.

## 21.3 Race and concurrency tests

```bash
cd engine
go test -race ./...
go test -race -count=20 ./internal/fleet/...
go test -race -count=20 ./internal/handoff/...
go test -count=100 ./internal/reporting/...
```

Scenarios:

- [ ] Two Quartermaster report generation jobs overlap.
- [ ] Ship submits report while Fleet Report is aggregating.
- [ ] Two approvals race for same handoff request.
- [ ] Budget threshold races with job execution start.
- [ ] Freeze races with provider/tool call initiation.
- [ ] Ship status update races with dock/pause action.

## 21.4 Security tests

- [ ] Quartermaster cannot access Ship raw workspace path.
- [ ] Quartermaster cannot read credential material.
- [ ] Crew Member cannot access another Ship’s memory.
- [ ] Child job cannot elevate parent capability.
- [ ] Artifact transfer cannot bypass classification policy.
- [ ] Approval replay is rejected.
- [ ] Modified handoff target after approval is rejected.
- [ ] Fleet report redacts sensitive fields.
- [ ] Emergency freeze cannot be bypassed by alternate delivery route.

## 21.5 End-to-end acceptance scenarios

### Scenario 1 — Development and Marketing collaboration

```text
Pirate King asks for feature launch preparation.
Quartermaster proposes Development + Marketing Ship plan.
Pirate King approves.
Development Ship produces feature capability artifact.
Quartermaster proposes redacted handoff to Marketing Ship.
Pirate King approves.
Marketing Ship creates content brief and draft.
Quartermaster reports launch readiness and outstanding approvals.
```

### Scenario 2 — Fleet budget escalation

```text
Two concurrent voyages use more token budget than forecast.
Ship soft budget alert occurs.
Quartermaster includes it in decision brief.
Hard limit stops new paid calls.
Pirate King approves additional allocation or reduces scope.
```

### Scenario 3 — Security incident

```text
An MCP server output is flagged as malicious.
Tool Runtime disables server route per emergency policy.
Ship becomes degraded.
Quartermaster creates critical escalation.
Pirate King sees affected Ships and remediation options.
```

---

# 22. Risks and Critical Decisions

## 22.1 Risks

| Risk | Consequence | Mitigation |
|---|---|---|
| Quartermaster becomes super-agent | Privilege escalation and opaque actions | Strict read/coordination default, no direct high-risk tools |
| Fleet scope leaks Ship data | Privacy/security incident | Summary projections, classification, explicit handoffs |
| Pirate terminology harms professional adoption | UX confusion | Dual labels and clean/professional mode |
| Too many roles/agents | Cost, redundancy, coordination overhead | Start with small templates and measurable role contracts |
| Reports become noise | Pirate King ignores alerts | Escalation thresholds, decision-focused briefs |
| Cross-Ship task loops | Runaway orchestration | Depth/concurrency/budget limits and cycle detection |
| Budget estimates inaccurate | Overspend or false confidence | Estimated vs reported cost confidence labels |
| Memory contamination | Wrong project context used | Ship isolation and reviewed fleet knowledge promotion |
| Quartermaster model hallucination | Incorrect executive brief | Evidence-linked reports, source references, validation agent/skill |
| Complex UI too early | Delays core runtime | Start read-only Command Deck and ship summaries |

## 22.2 Critical decisions required

1. Is one Pirate King limited to one Fleet initially, or can they have several Fleets?
2. What level of Ship summary data may Quartermaster read by default?
3. Which classification requires Pirate King approval for cross-Ship handoff?
4. Can Quartermaster temporarily freeze a route under emergency policy, or only propose a freeze?
5. Should Fleet Knowledge be opt-in only at first?
6. Which Ships should ship in the first template set: Development and Marketing only, or also Research/Operations?
7. Should `Quartermaster` be strictly one agent per Fleet, or configurable in future?
8. What budget controls are available in v1: token count, cost estimate, or both?
9. Which high-risk action categories always require Pirate King approval?
10. Should professional terminology mode be available from v1?

## 22.3 Recommended defaults

```text
- One Quartermaster per Fleet.
- One Pirate King can own multiple Fleets, but start UI with one active Fleet.
- Quartermaster sees Ship Summary Projections only by default.
- Confidential/restricted cross-Ship handoffs require Pirate King approval.
- Quartermaster may auto-freeze only already-authorized emergency routes; cannot unfreeze.
- Fleet Knowledge promotion is opt-in and approval-gated.
- First templates: Development Ship and Marketing Ship.
- Budget v1: soft/hard estimated cost plus token telemetry where available.
- Professional terminology toggle should be available, but pirate theme remains default brand.
```

---

# 23. Definition of Done

The Quartermaster/Fleet Command feature is complete for its first production-ready release when:

1. One Pirate King can create and manage multiple Ships within a Fleet.
2. Every Ship has isolated policy, budget, workspace, memory, artifact, and audit boundaries.
3. A Quartermaster exists as a bounded Fleet coordinator with no implicit high-risk authority.
4. Quartermaster consumes Ship Summary Projections rather than unrestricted raw Ship data.
5. Quartermaster can create Fleet Reports, Decision Briefs, and Escalations.
6. Pirate King can approve/reject Fleet Order proposals and high-risk handoffs.
7. Ships can have Captains, Squads, and Crew Members configured with real skill/tool/model/memory policies.
8. Effective permissions are computed as an intersection of Fleet-to-task policy layers.
9. Cross-Ship artifact transfer is policy-checked, approval-bound where required, and audited.
10. Fleet and Ship budget thresholds are visible and enforced.
11. Emergency freeze can safely halt configured routes without deleting audit history.
12. TUI, Tauri, and web clients render the same canonical Go-engine state/events.
13. Fleet reports include traceable source Ship report/artifact references.
14. No credential, restricted raw data, or unauthorized workspace content leaks across Ship boundaries.
15. Unit, integration, security, and race tests pass consistently.

---

# 24. Appendices

## Appendix A — Example Fleet Report

```markdown
# Fleet Report — 28 September 2026

## Overall Status
Attention Required

- Ships active: 4
- Voyages running: 6
- Voyages blocked: 2
- Approval requests: 2
- Fleet daily budget: 38% used
- Provider health: 3 healthy, 1 degraded

## Development Ship
- Phase 3 Tool Calling specification completed.
- QA threat-model test matrix is ready.
- Blocker: decision required for read-only GitHub MCP pilot.

## Marketing Ship
- Website content audit completed.
- Six content opportunities identified.
- Two article drafts are ready for editorial review.

## Research Ship
- Provider-routing comparison and evidence set complete.
- One source requires freshness check before reuse.

## Decisions Required from Pirate King
1. Approve GitHub MCP read-only pilot for Development Ship.
2. Approve internal research artifact handoff to Marketing Ship as a redacted summary.
3. Decide whether to allocate additional budget for a premium research model route.

## Top Risks
- Development Ship has no durable approval-store integration yet.
- One cloud provider route exceeded the soft latency threshold.
- Marketing draft requires editorial confirmation before publishing.
```

## Appendix B — Example Developer Ship Template

```yaml
ship_template:
  id: development-ship-v1
  name: Development Ship
  domain: development
  captain:
    role: engineering_lead
  squads:
    - developer_squad
    - quality_squad
    - research_and_development_squad
  members:
    - engineering_lead
    - product_owner
    - backend_engineer
    - frontend_engineer
    - qa_engineer
    - rnd_engineer
  default_model_profile: code-balanced
  default_tool_policy: development-standard
  default_memory_policy: ship-isolated
  budget_profile: engineering-standard
```

## Appendix C — Example Marketing Ship Template

```yaml
ship_template:
  id: marketing-ship-v1
  name: Marketing Ship
  domain: marketing
  captain:
    role: marketing_lead
  squads:
    - market_research_squad
    - seo_squad
    - content_squad
    - growth_squad
  members:
    - marketing_lead
    - market_researcher
    - seo_strategist
    - copywriter
    - content_editor
    - growth_analyst
    - brand_strategist
  default_model_profile: research-balanced
  default_tool_policy: marketing-read-draft
  default_memory_policy: ship-isolated
  budget_profile: marketing-standard
```

## Appendix D — Recommended ADRs

1. ADR-FLEET-001: Pirate King is the final human authority for Fleet governance.
2. ADR-FLEET-002: Quartermaster is a bounded coordinator, not a superuser.
3. ADR-FLEET-003: Ship is the default isolation boundary for workspace, memory, tools, budget, and artifacts.
4. ADR-FLEET-004: Quartermaster consumes summary projections by default.
5. ADR-FLEET-005: Cross-Ship artifact transfer requires explicit policy and auditable provenance.
6. ADR-FLEET-006: Effective permissions are the intersection of Fleet, Ship, Squad, Member, Skill, and Task restrictions.
7. ADR-FLEET-007: Fleet Knowledge is curated and approval-gated; Ship Memory is isolated by default.
8. ADR-FLEET-008: Fleet-level budget/policy ceilings cannot be increased by lower agents.
9. ADR-FLEET-009: Emergency freeze may only restrict, never expand, capability.
10. ADR-FLEET-010: Pirate-themed labels must retain clear professional technical equivalents.
