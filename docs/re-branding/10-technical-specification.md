# Technical Specification

> Part of [Quartermaster — Fleet Command Architecture](00-overview.md)
>
> Go conventions follow the existing `engine/` layout: `core/` for platform,
> `src/` for domain modules. Each module uses `interfaces.go`, `dto.go`,
> `services.go`, `delivery.go`, `wire.go`.

---

## Go module layout

```text
engine/
├── app/
├── cmd/agent-engine/
├── core/
│   ├── config/
│   ├── errors/
│   ├── events/
│   ├── logger/
│   ├── metrics/
│   ├── policy/
│   ├── secrets/
│   ├── auth/
│   └── persistence/
├── src/
│   ├── fleet/
│   │   ├── interfaces.go
│   │   ├── dto.go
│   │   ├── services.go
│   │   ├── delivery.go
│   │   └── wire.go
│   ├── ships/
│   ├── crew/
│   ├── skills/
│   ├── voyages/
│   ├── reporting/
│   ├── escalation/
│   ├── handoff/
│   ├── budget/
│   ├── tools/
│   ├── modelgateway/
│   ├── memory/
│   └── artifacts/
├── proto/
│   ├── fleet.proto
│   ├── ship.proto
│   ├── crew.proto
│   ├── voyage.proto
│   ├── report.proto
│   └── handoff.proto
└── pkg/api/
```

## Core interfaces

```go
package fleet

import "context"

// FleetService defines the contract for fleet lifecycle operations
type FleetService interface {
	CreateFleet(ctx context.Context, cmd CreateFleetCommand) (Fleet, error)
	GetFleet(ctx context.Context, fleetID string) (Fleet, error)
	ListFleetSummaries(ctx context.Context, actor Actor) ([]FleetSummary, error)
	CreateFleetOrderProposal(ctx context.Context, cmd CreateFleetOrderProposalCommand) (FleetOrderProposal, error)
}

// QuartermasterService defines the contract for fleet coordination operations
type QuartermasterService interface {
	IntakeObjective(ctx context.Context, req FleetObjective) (FleetOrderProposal, error)
	BuildFleetReport(ctx context.Context, fleetID string, window ReportWindow) (FleetReport, error)
	CreateDecisionBrief(ctx context.Context, req DecisionBriefRequest) (DecisionBrief, error)
	Escalate(ctx context.Context, req EscalationRequest) (Escalation, error)
}

// ShipService defines the contract for ship lifecycle and reporting
type ShipService interface {
	CreateShip(ctx context.Context, cmd CreateShipCommand) (Ship, error)
	GetShipSummary(ctx context.Context, shipID string, actor Actor) (ShipSummary, error)
	SubmitShipReport(ctx context.Context, report ShipReport) error
	SetShipStatus(ctx context.Context, shipID string, status ShipStatus) error
}

// ArtifactHandoffService defines the contract for cross-ship artifact transfer
type ArtifactHandoffService interface {
	Propose(ctx context.Context, cmd CreateHandoffProposalCommand) (HandoffProposal, error)
	Approve(ctx context.Context, approval ApprovalContext) (HandoffReceipt, error)
	Deny(ctx context.Context, approval ApprovalContext) error
}
```

## Go concurrency rules

- [ ] Every Fleet Order, Voyage, Job Order, tool execution, and report aggregation inherits `context.Context` from its parent.
- [ ] Quartermaster report generation may run concurrently across Ships, but must use bounded worker pools.
- [ ] Fleet summary collection uses timeouts per Ship so one unhealthy Ship does not block the entire report.
- [ ] A Quartermaster must not hold a database/aggregate lock while calling a Ship, provider, MCP server, or tool.
- [ ] State transitions must be atomic and idempotent.
- [ ] Fleet Report creation must be reproducible from report inputs and persisted event references.

## Status models

### Fleet status

```go
// FleetStatus represents the state of a fleet
type FleetStatus string

const (
	FleetStatusActive            FleetStatus = "active"
	FleetStatusHealthy           FleetStatus = "healthy"
	FleetStatusDegraded          FleetStatus = "degraded"
	FleetStatusAttentionRequired FleetStatus = "attention_required"
	FleetStatusPaused            FleetStatus = "paused"
	FleetStatusArchived          FleetStatus = "archived"
)
```

### Ship status

```go
// ShipStatus represents the state of a ship
type ShipStatus string

const (
	ShipStatusActive             ShipStatus = "active"
	ShipStatusReady              ShipStatus = "ready"
	ShipStatusWorking            ShipStatus = "working"
	ShipStatusWaitingForApproval ShipStatus = "waiting_for_approval"
	ShipStatusBlocked            ShipStatus = "blocked"
	ShipStatusDegraded           ShipStatus = "degraded"
	ShipStatusPaused             ShipStatus = "paused"
	ShipStatusDocked             ShipStatus = "docked"
	ShipStatusArchived           ShipStatus = "archived"
)
```

### Crew member status

```go
// CrewMemberStatus represents the state of a crew member
type CrewMemberStatus string

const (
	CrewMemberStatusIdle               CrewMemberStatus = "idle"
	CrewMemberStatusPlanning           CrewMemberStatus = "planning"
	CrewMemberStatusWorking            CrewMemberStatus = "working"
	CrewMemberStatusWaitingForInput    CrewMemberStatus = "waiting_for_input"
	CrewMemberStatusWaitingForApproval CrewMemberStatus = "waiting_for_approval"
	CrewMemberStatusBlocked            CrewMemberStatus = "blocked"
	CrewMemberStatusOffline            CrewMemberStatus = "offline"
	CrewMemberStatusPaused             CrewMemberStatus = "paused"
)
```

### Quartermaster status

```go
// QuartermasterStatus represents the state of the quartermaster
type QuartermasterStatus string

const (
	QuartermasterStatusAvailable          QuartermasterStatus = "available"
	QuartermasterStatusCollectingReports  QuartermasterStatus = "collecting_reports"
	QuartermasterStatusBuildingBrief      QuartermasterStatus = "building_brief"
	QuartermasterStatusWaitingForPirateKing QuartermasterStatus = "waiting_for_pirate_king"
	QuartermasterStatusEscalating         QuartermasterStatus = "escalating"
	QuartermasterStatusPaused             QuartermasterStatus = "paused"
)
```

## Effective permission formula

```text
Effective Capability Set =
  Fleet Policy Ceiling
  ∩ Ship Policy
  ∩ Squad Policy
  ∩ Crew Member Policy
  ∩ Skill Policy
  ∩ Job Order Restriction
  ∩ Runtime Context Restriction
```

No lower layer can add a capability denied by a higher layer.

## Memory boundaries

```text
Fleet Knowledge
  - Only curated, approved, shareable lessons/policies/templates

Ship Memory
  - Project-specific facts, artifacts, conventions, lessons

Voyage Memory
  - Active run/task context and checkpoints

Crew Member Working Memory
  - Short-lived context for current Job Order
```

Promotion flow:

```text
Crew/Ship lesson
  → Ship Lead/Captain review
  → Quartermaster fleet lesson proposal
  → Pirate King approval where policy requires
  → Fleet Knowledge
```

---

## Existing Engine Module Mapping

> The following shows how current `engine/src/` modules map to Fleet Command.
> Modules marked ✅ already exist and need renaming or extension.

| Fleet Module (proposed) | Existing Module | Key Interfaces | Change Required |
|---|---|---|---|
| `src/fleet/` | — | `FleetService` | 🆕 New module |
| `src/ships/` | — | `ShipService` | 🆕 New module |
| `src/voyages/` | `src/run/` | `run.Service`, `run.Store`, `run.EventHub` | ✅ Rename `Run` → `Voyage`, add `ShipID`, `FleetOrderID`, `ParentVoyageID` fields, extend state machine |
| `src/crew/` | `src/crew/` | `crew.Orchestrator`, `crew.Registry` | ✅ Extend `AgentDefinition` → `CrewMember` with SkillRefs, ToolPolicyRef, ModelProfileRef, MemoryPolicyRef, BudgetCeiling. `CrewDefinition` → `Squad`. |
| `src/skills/` | — | Skill Registry/Resolver | 🆕 New module (skill references exist as `Capabilities []string` in `AgentDefinition`) |
| `src/reporting/` | — | Fleet/Ship Report services | 🆕 New module |
| `src/escalation/` | — | Escalation service | 🆕 New module |
| `src/handoff/` | — | Artifact handoff service | 🆕 New module |
| `src/budget/` | — (partial: `llm.TokenUsage.EstimatedCostUSD`) | Budget allocation | 🆕 New module (extends existing token tracking) |
| `src/tools/` | `src/tool/` | `tool.Service`, `tool.Registry`, `tool.PolicyEngine`, `tool.ApprovalGate` | ✅ Already has RiskTier, RiskClass, PolicyVerdict, ApprovalStatus, ExecutionContext with data_classification and capabilities — core of Fleet tool policy |
| `src/modelgateway/` | `src/llm/` | `llm.Provider`, `llm.MultiProvider` | ✅ Rename/extend. Already has multi-provider, streaming, retry, TokenUsage. Add route policy, budget guard, circuit breaker. |
| `src/memory/` | `src/memory/` | `memory.SessionMemory`, `memory.VectorStore` | ✅ Already has scope isolation via `Document.Scope` and `SearchWithScope`. Extend for Ship memory boundaries. |
| `src/artifacts/` | `src/artifact/` | `artifact.Service`, `artifact.Repository` | ✅ Already has Artifact with ID, RunID, TaskID, Type, Hash, Content. Add `classification`, `shareable_to_fleet`, `ship_id`. |

### Existing `engine/src/` layout (current)

```text
engine/src/
├── artifact/    → extends to treasures + classification
├── crew/        → extends to Squad + CrewMember model
├── llm/         → extends to Model Gateway module
├── memory/      → extends with Ship memory scope
├── persistence/ → shared disk store
├── run/         → renames to voyages module
├── task/        → renames to job orders
├── tool/        → extends with Fleet policy hierarchy
└── workflow/    → renames to maps module
```
