# Task Breakdown

> Part of [Quartermaster — Fleet Command Architecture](00-overview.md)

---

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
