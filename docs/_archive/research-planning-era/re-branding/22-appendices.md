# Appendices

> Part of [Quartermaster — Fleet Command Architecture](00-overview.md)

---

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

- [ ] ADR-FLEET-001: Pirate King is the final human authority for Fleet governance.
- [ ] ADR-FLEET-002: Quartermaster is a bounded coordinator, not a superuser.
- [ ] ADR-FLEET-003: Ship is the default isolation boundary for workspace, memory, tools, budget, and artifacts.
- [ ] ADR-FLEET-004: Quartermaster consumes summary projections by default.
- [ ] ADR-FLEET-005: Cross-Ship artifact transfer requires explicit policy and auditable provenance.
- [ ] ADR-FLEET-006: Effective permissions are the intersection of Fleet, Ship, Squad, Member, Skill, and Task restrictions.
- [ ] ADR-FLEET-007: Fleet Knowledge is curated and approval-gated; Ship Memory is isolated by default.
- [ ] ADR-FLEET-008: Fleet-level budget/policy ceilings cannot be increased by lower agents.
- [ ] ADR-FLEET-009: Emergency freeze may only restrict, never expand, capability.
- [ ] ADR-FLEET-010: Pirate-themed labels must retain clear professional technical equivalents.
