# Implementation Roadmap

> Part of [Quartermaster — Fleet Command Architecture](00-overview.md)

---

## Fleet Phase F0 — Fleet foundation

**Goal:** Represent Pirate King, Fleet, Quartermaster, and Ship boundaries without changing agent execution deeply.

### Deliverables

- [ ] Fleet, Pirate King, Quartermaster, Ship entities.
- [ ] Fleet/Ship status models.
- [ ] Command Deck read-only UI.
- [ ] Ship summary projection.
- [ ] Fleet Report generated from Ship summaries.
- [ ] Initial Developer/Marketing Ship templates.
- [ ] Basic audit events.

### Exit criteria

- [ ] One Pirate King can own multiple Ships.
- [ ] Quartermaster can produce read-only Fleet Report.
- [ ] No raw cross-Ship workspace access occurs.

## Fleet Phase F1 — Crew Members and Squad runtime configuration

**Goal:** Make Crew Members real runtime configurations rather than persona cards.

### Deliverables

- [ ] Squad entity.
- [ ] Crew Member entity.
- [ ] Role templates.
- [ ] Skill references.
- [ ] Tool/model/memory/evaluation policy references.
- [ ] Member status/activity UI.
- [ ] Developer Squad template.
- [ ] Marketing Squad template.

### Exit criteria

- [ ] Crew Member configuration resolves into effective policy.
- [ ] Member cannot exceed Ship/Fleet policy ceiling.
- [ ] UI shows role, skills, tools, model profile, and status.

## Fleet Phase F2 — Fleet Orders and Voyage coordination

**Goal:** Let Quartermaster route strategic objectives across Ships.

### Deliverables

- [ ] Fleet Order proposal and approval flow.
- [ ] Scoped Ship Orders.
- [ ] Cross-Ship dependency graph.
- [ ] Captain acceptance/planning workflow.
- [ ] Voyage linking to Fleet Order.
- [ ] Decision Brief UI.

### Exit criteria

- [ ] Pirate King can approve a multi-Ship plan.
- [ ] Each Ship receives only scoped objective/input/budget.
- [ ] Quartermaster cannot directly force unsafe execution.

## Fleet Phase F3 — Handoff, budgets, and governance

**Goal:** Add safe fleet-level resource coordination.

### Deliverables

- [ ] Fleet/Ship budget hierarchy.
- [ ] Artifact handoff proposal/approval/receipt.
- [ ] Cross-Ship classification policy.
- [ ] Fleet provider/model ceilings.
- [ ] Approval queue.
- [ ] Emergency freeze mechanism.
- [ ] Budget escalation.

### Exit criteria

- [ ] Cross-Ship artifact transfer is explicit/audited.
- [ ] Budget cannot silently exceed Fleet hard limit.
- [ ] Freeze action blocks affected route/voyage safely.

## Fleet Phase F4 — Fleet knowledge and evaluation

**Goal:** Make the fleet improve without contaminating project memory.

### Deliverables

- [ ] Fleet Knowledge store.
- [ ] Lesson promotion proposal workflow.
- [ ] Skill evaluation scorecards.
- [ ] Role performance scorecards.
- [ ] Recommended squad composition based on historical data.
- [ ] Canary skill/model profile rollout.

### Exit criteria

- [ ] Ship memory remains isolated by default.
- [ ] Fleet knowledge promotion is reviewed and provenance-backed.
- [ ] New skills/templates have measurable quality gates.
