# Event Contracts

> Part of [Quartermaster — Fleet Command Architecture](00-overview.md)

---

## Fleet events

```text
fleet.created
fleet.updated
fleet.paused
fleet.resumed
fleet.policy_changed
fleet.budget_threshold_reached
fleet.report_created
fleet.decision_required
fleet.escalation_created
```

## Ship events

```text
ship.created
ship.activated
ship.docked
ship.paused
ship.status_changed
ship.report_submitted
ship.budget_threshold_reached
ship.provider_degraded
ship.blocked
```

## Quartermaster events

```text
quartermaster.intake_received
quartermaster.fleet_order_proposed
quartermaster.report_collection_started
quartermaster.report_collection_completed
quartermaster.fleet_report_created
quartermaster.decision_brief_created
quartermaster.escalation_created
quartermaster.handoff_proposed
quartermaster.lesson_promotion_proposed
```

## Crew events

```text
crew_member.created
crew_member.status_changed
crew_member.job_assigned
crew_member.job_completed
crew_member.blocked
crew_member.paused
```

## Event payload example

```json
{
  "event_id": "evt_01J...",
  "sequence": 84,
  "fleet_id": "fleet_01J...",
  "ship_id": "ship_development",
  "voyage_id": "voyage_01J...",
  "type": "ship.report_submitted",
  "occurred_at": "2026-09-28T11:28:00Z",
  "correlation_id": "corr_01J...",
  "data": {
    "report_id": "shipreport_01J...",
    "status": "attention_required",
    "summary": "QA matrix complete; MCP integration decision is blocking implementation."
  }
}
```

---

## Existing Engine Event Mapping

> The existing `RunEvent` struct in `engine/src/run/dto.go` already provides the event infrastructure.
> Fleet events extend this foundation.

### Existing event struct

```go
// engine/src/run/dto.go — already exists
type RunEvent struct {
    EventID   string      `json:"event_id"`
    RunID     string      `json:"run_id"`       // → add FleetID, ShipID, VoyageID
    Sequence  int64       `json:"sequence"`
    Type      string      `json:"type"`          // → fleet.*, ship.*, quartermaster.*, crew_member.*
    Timestamp time.Time   `json:"timestamp"`     // → matches occurred_at
    Payload   any         `json:"payload"`       // → matches data
    Error     string      `json:"error"`
}
```

### Existing event types (from services)

| Existing Event Type | Fleet Equivalent |
|---|---|
| `artifact.created` | `ship.artifact_created` (scoped to Ship) |
| `run.started` | `voyage.started` |
| `run.completed` | `voyage.completed` |
| `run.failed` | `voyage.failed` |
| `run.cancelled` | `voyage.cancelled` |
| `task.started` | `job_order.started` |
| `task.completed` | `job_order.completed` |
| `task.failed` | `job_order.failed` |
| `tool.approval_requested` | `crew_member.waiting_for_approval` |
| `tool.approved` | Fleet approval event |
| `tool.denied` | Fleet denial event |

> **New Fleet event categories** not mapped to existing events: `fleet.*`, `ship.*`, `quartermaster.*` — these are entirely new coordination-layer events.
