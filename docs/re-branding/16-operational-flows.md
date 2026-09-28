# Operational Flows

> Part of [Quartermaster — Fleet Command Architecture](00-overview.md)

---

## Flow A — Pirate King creates a Ship

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

## Flow B — Strategic objective routing

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

## Flow C — Ship reporting

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

## Flow D — Cross-Ship handoff

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

## Flow E — Budget escalation

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

## Flow F — Emergency freeze

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

## Flow G — AI-assisted Squad creation

```text
Pirate King
  → opens Ship detail page
  → clicks [+ New Squad]
  → types natural language request: "Buatkan saya 1 squad tim marketing"
     (or selects a template)
  → clicks [Ask QM →]

Quartermaster
  → receives squad composition request
  → researches real-world industry standards for the requested domain
  → determines ideal team size (e.g., marketing: 5-8 members)
  → maps each position to:
       role, mission, skills, tool permissions, model profile, approval policy
  → considers Ship budget and Fleet policy ceilings
  → generates SquadCompositionProposal with rationale

Go Engine
  → creates SquadCompositionProposal entity (status: awaiting_review)
  → emits squad_proposal.created event

UI
  → displays proposal modal to Pirate King

Pirate King reviews proposal:
  → can [✎ Edit] any member: change name, mission, skills, tools, model profile
  → can [🗑 Remove] any member from proposal
  → can [+ Add Member] to the proposal
  → can reassign ★ Captain designation to a different member
  → can [Cancel] to discard the entire proposal

Pirate King decision:
  IF Cancel:
    → proposal marked as cancelled
    → no Squad created
    → emits squad_proposal.cancelled event

  IF [Submit & Create Squad]:
    → finalized proposal sent to Go Engine

Go Engine on submit:
  → validates all members against Ship policy and Fleet policy ceiling
  → creates Squad entity
  → creates each Crew Member entity
  → initializes per-member tool/model/memory policy references
  → allocates Squad budget from Ship budget
  → emits squad.created event
  → emits crew_member.created event per member
  → records proposal → approval audit trail
  → returns success confirmation

UI
  → displays success confirmation with [Open Squad] action
```
