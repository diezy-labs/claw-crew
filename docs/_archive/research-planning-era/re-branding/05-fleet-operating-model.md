# Fleet Operating Model

> Part of [Quartermaster — Fleet Command Architecture](00-overview.md)

---

## Fleet topology

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

## Ship isolation

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

## Voyage lifecycle

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

- [ ] Single-Ship.
- [ ] Cross-Ship with explicit dependency graph.
- [ ] Scheduled/read-only monitoring voyage.
- [ ] User-initiated ad hoc voyage.

## Fleet Order lifecycle

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

## Cross-Ship coordination rules

Cross-Ship work must use explicit contracts:

- [ ] Objective contract.
- [ ] Input artifact references.
- [ ] Data classification.
- [ ] Allowed recipient Ships.
- [ ] Output expectations.
- [ ] Budget allocation.
- [ ] Dependency order.
- [ ] Approval requirements.

A Ship must not directly inspect another Ship's raw workspace merely because both are in the same Fleet.
