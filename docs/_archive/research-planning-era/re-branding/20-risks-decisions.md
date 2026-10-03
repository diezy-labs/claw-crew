# Risks and Critical Decisions

> Part of [Quartermaster — Fleet Command Architecture](00-overview.md)

---

## Risks

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

## Critical decisions required

- [ ] Is one Pirate King limited to one Fleet initially, or can they have several Fleets?
- [ ] What level of Ship summary data may Quartermaster read by default?
- [ ] Which classification requires Pirate King approval for cross-Ship handoff?
- [ ] Can Quartermaster temporarily freeze a route under emergency policy, or only propose a freeze?
- [ ] Should Fleet Knowledge be opt-in only at first?
- [ ] Which Ships should ship in the first template set: Development and Marketing only, or also Research/Operations?
- [ ] Should `Quartermaster` be strictly one agent per Fleet, or configurable in future?
- [ ] What budget controls are available in v1: token count, cost estimate, or both?
- [ ] Which high-risk action categories always require Pirate King approval?
- [ ] Should professional terminology mode be available from v1?

## Recommended defaults

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
