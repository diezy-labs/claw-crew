# Testing and Quality Strategy

> Part of [Quartermaster — Fleet Command Architecture](00-overview.md)

---

## Unit tests

- [ ] Fleet/Ship status state-machine tests.
- [ ] Effective permission intersection tests.
- [ ] Fleet budget allocation tests.
- [ ] Cross-Ship classification policy tests.
- [ ] Summary redaction tests.
- [ ] Handoff approval binding tests.
- [ ] Decision Brief formatting/required-field tests.
- [ ] Escalation severity routing tests.
- [ ] Fleet Report aggregation ordering tests.

## Integration tests

- [ ] Pirate King creates Fleet and multiple Ships.
- [ ] Quartermaster receives Ship summaries but not restricted raw data.
- [ ] Fleet Order routes to correct Ship.
- [ ] Captain receives scoped order only.
- [ ] Cross-Ship artifact handoff denied without policy/approval.
- [ ] Approved redacted handoff succeeds and records receipt.
- [ ] Budget hard limit pauses new paid action.
- [ ] Emergency freeze disables affected route.
- [ ] Fleet report handles one unavailable Ship without failing entire report.

## Race and concurrency tests

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

## Security tests

- [ ] Quartermaster cannot access Ship raw workspace path.
- [ ] Quartermaster cannot read credential material.
- [ ] Crew Member cannot access another Ship's memory.
- [ ] Child job cannot elevate parent capability.
- [ ] Artifact transfer cannot bypass classification policy.
- [ ] Approval replay is rejected.
- [ ] Modified handoff target after approval is rejected.
- [ ] Fleet report redacts sensitive fields.
- [ ] Emergency freeze cannot be bypassed by alternate delivery route.

## End-to-end acceptance scenarios

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
