# System Design

> Part of [Quartermaster — Fleet Command Architecture](00-overview.md)

---

## Design goals

- [ ] One Pirate King can govern many Ships.
- [ ] Ships stay isolated by default.
- [ ] Quartermaster sees summaries by default, not all raw data.
- [ ] Cross-Ship transfer is explicit, auditable, and policy-filtered.
- [ ] Fleet-level budget and policy constrain lower layers.
- [ ] Every high-impact action remains human-approved.
- [ ] Go owns state transitions, policy, event delivery, and audit logs.
- [ ] Clients are projections of Go state, not independent authorities.

## Logical components

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

## Core design rule: summary projection

Quartermaster operates primarily on **Ship Summary Projections**, not raw workspace data.

```text
Ship raw data
  → Ship policy filters/redacts
  → Ship Summary Projection
  → Quartermaster reads projection
  → Fleet Report
```

This minimizes accidental data leakage and keeps fleet reporting scalable.

## Summary projection contents

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

## Cross-Ship artifact handoff

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
