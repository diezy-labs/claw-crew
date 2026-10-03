# Product Requirements Document

> Part of [Quartermaster — Fleet Command Architecture](00-overview.md)

---

## Problem statement

Users managing multiple projects need a way to coordinate AI agent work across separate workspaces without losing isolation, control, visibility, or decision authority.

A user should not need to open every project, read every raw log, manage every subtask, or inspect every agent action. At the same time, agents must not gain unrestricted cross-project access or autonomous authority over budget, policy, publication, deployment, or irreversible actions.

## Target users

| User | Need |
|---|---|
| Solo builder / Pirate King | Oversees several technical, research, marketing, or operations projects |
| Project operator | Manages a Ship and needs focused agent teams |
| Technical lead | Needs delegated code/review/test workflows |
| Content/marketing lead | Needs research, SEO, drafting, and reporting workflows |
| Reviewer/approver | Needs concise decision briefs and exact approval payloads |
| Future team admin | Needs policy, budget, provider, and audit visibility |

## Goals

- [ ] Allow one Pirate King to own and manage multiple Ships.
- [ ] Keep Ship data, policy, memory, provider, budget, and tool permissions isolated by default.
- [ ] Add one Quartermaster agent that coordinates and summarizes at fleet scope.
- [ ] Allow each Ship to have a Captain, Squads, and specialized Crew Members.
- [ ] Allow objectives to be routed to one or more Ships through explicit Voyage proposals.
- [ ] Provide concise Fleet Reports and escalation workflows.
- [ ] Prevent privilege escalation from Quartermaster, Captain, Squad, Crew Member, or Skill.
- [ ] Support cross-Ship artifact sharing only through explicit policy and provenance.
- [ ] Make cost, risk, status, and approval queues visible at fleet level.
- [ ] Preserve Go as the canonical orchestration and policy engine.

## Non-goals

- Replace human strategy or executive judgment.
- Let Quartermaster autonomously alter global policy.
- Let Quartermaster publish, deploy, spend money, or access credentials directly.
- Share all memory automatically across Ships.
- Build a public marketplace before governance is mature.
- Make every Ship use the same tools/models/skills.
- Create unlimited agent hierarchies without budget/concurrency controls.

## User stories

### Pirate King stories

- [ ] As a Pirate King, I want to see every Ship's health, active voyages, budget, and blocked work in one dashboard.
- [ ] As a Pirate King, I want Quartermaster to summarize only decisions that require my attention.
- [ ] As a Pirate King, I want to create a Development Ship and a Marketing Ship with different permissions and models.
- [ ] As a Pirate King, I want to approve a cross-Ship transfer of an artifact before confidential content leaves a Ship.
- [ ] As a Pirate King, I want to pause a Ship or freeze its external actions during an incident.

### Quartermaster stories

- [ ] As Quartermaster, I want to receive status reports from Captains and produce a concise Fleet Report.
- [ ] As Quartermaster, I want to propose an objective route to relevant Ships without launching irreversible work.
- [ ] As Quartermaster, I want to flag budget, policy, provider, and dependency risks.
- [ ] As Quartermaster, I want to create decision briefs with options, trade-offs, and required approvals.

### Captain stories

- [ ] As a Captain, I want to receive a scoped Fleet Order with objective, constraints, inputs, and budget.
- [ ] As a Captain, I want to delegate Job Orders to my Ship's Crew Members.
- [ ] As a Captain, I want to report completion, blockers, artifacts, and risk back to Quartermaster.

### Crew Member stories

- [ ] As a Crew Member, I want to receive only the skills, tools, memory scope, and budget needed for my Job Order.
- [ ] As a Crew Member, I want to create reviewable artifacts and request approval for consequential work.

## Success metrics

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
