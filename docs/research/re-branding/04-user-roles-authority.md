# User Roles and Authority

> Part of [Quartermaster — Fleet Command Architecture](00-overview.md)

---

## Authority hierarchy

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

## Pirate King

### Responsibilities

- [ ] Defines strategic goals.
- [ ] Creates/archives Ships.
- [ ] Assigns fleet budget and top-level priorities.
- [ ] Approves global policy changes.
- [ ] Approves provider/model allowlists.
- [ ] Approves high-risk integrations, MCP servers, and credentials.
- [ ] Approves cross-Ship sharing for sensitive content.
- [ ] Approves irreversible actions: deploy, publish, send, purchase, migration, high-impact commit/push.
- [ ] Reviews Quartermaster decision briefs.

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

## Quartermaster

### Role

Quartermaster is the fleet-level coordination assistant.

### Allowed actions by default

- [ ] Read permitted Ship summaries and report metadata.
- [ ] Read fleet budget summaries.
- [ ] Create Fleet Reports.
- [ ] Create decision briefs.
- [ ] Propose Fleet Orders and cross-Ship Voyage plans.
- [ ] Request status refresh from Captains.
- [ ] Create escalation events.
- [ ] Propose artifact transfer.
- [ ] Propose fleet lesson promotion.
- [ ] Suggest prioritization based on configured rules.

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

## Captain

Captain is the per-Ship orchestrator. It receives a scoped objective and delegates to Squads/Crew Members. It owns the Ship-level plan but remains constrained by Ship/Fleet policies.

## Squad Lead

Squad Lead coordinates work inside a functional domain. It cannot expand tool or model permissions beyond the Crew Member/Ship policy intersection.

## Crew Member

Crew Members are specialist agents. They perform Job Orders with narrowly defined mission, skills, tool permissions, model route profile, memory policy, concurrency budget, and output contract.
