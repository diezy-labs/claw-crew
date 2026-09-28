# API Contracts

> Part of [Quartermaster — Fleet Command Architecture](00-overview.md)

---

## Fleet APIs

```http
GET    /api/v1/fleets
POST   /api/v1/fleets
GET    /api/v1/fleets/{fleet_id}
PATCH  /api/v1/fleets/{fleet_id}
POST   /api/v1/fleets/{fleet_id}/pause
POST   /api/v1/fleets/{fleet_id}/resume
GET    /api/v1/fleets/{fleet_id}/command-deck
```

### Create fleet

```http
POST /api/v1/fleets
Content-Type: application/json
```

```json
{
  "name": "Personal Product Fleet",
  "description": "Development, marketing, research, and operations ships.",
  "quartermaster": {
    "display_name": "Quartermaster",
    "codename": "Quarterclaw",
    "model_profile": "fleet-balanced"
  }
}
```

## Ship APIs

```http
GET    /api/v1/fleets/{fleet_id}/ships
POST   /api/v1/fleets/{fleet_id}/ships
GET    /api/v1/ships/{ship_id}
PATCH  /api/v1/ships/{ship_id}
POST   /api/v1/ships/{ship_id}/dock
POST   /api/v1/ships/{ship_id}/activate
POST   /api/v1/ships/{ship_id}/pause
GET    /api/v1/ships/{ship_id}/summary
```

### Create ship

```json
{
  "name": "Development Ship",
  "domain": "development",
  "workspace_ref": "workspace://claw-crew",
  "captain_template": "engineering-lead",
  "budget": {
    "currency": "USD",
    "soft_limit": 10.0,
    "hard_limit": 20.0
  },
  "policy_profile": "development-standard"
}
```

## Quartermaster APIs

```http
GET  /api/v1/fleets/{fleet_id}/quartermaster
POST /api/v1/fleets/{fleet_id}/quartermaster/intake
POST /api/v1/fleets/{fleet_id}/quartermaster/reports
POST /api/v1/fleets/{fleet_id}/quartermaster/decision-briefs
GET  /api/v1/fleets/{fleet_id}/quartermaster/queue
```

### Submit strategic objective

```http
POST /api/v1/fleets/{fleet_id}/quartermaster/intake
```

```json
{
  "objective": "Prepare Phase 3 release plan covering tool calling, provider routing, and Crew Members.",
  "priority": "high",
  "constraints": {
    "max_budget_usd": 15.0,
    "deadline": "2026-10-10T00:00:00Z",
    "data_classification": "internal"
  }
}
```

Response:

```json
{
  "proposal_id": "fleetproposal_01J...",
  "status": "awaiting_pirate_king_approval",
  "proposed_ships": [
    "ship_development",
    "ship_research"
  ],
  "summary": "Development Ship will design implementation; Research Ship will validate patterns and risks.",
  "budget_estimate_usd": 8.5
}
```

## Fleet order APIs

```http
GET  /api/v1/fleet-orders/{fleet_order_id}
POST /api/v1/fleet-orders/{fleet_order_id}/approve
POST /api/v1/fleet-orders/{fleet_order_id}/reject
POST /api/v1/fleet-orders/{fleet_order_id}/revise
```

## Crew Member APIs

```http
GET    /api/v1/ships/{ship_id}/crew-members
POST   /api/v1/ships/{ship_id}/crew-members
GET    /api/v1/crew-members/{member_id}
PATCH  /api/v1/crew-members/{member_id}
PATCH  /api/v1/crew-members/{member_id}/model-profile
POST   /api/v1/crew-members/{member_id}/pause
POST   /api/v1/crew-members/{member_id}/resume
GET    /api/v1/crew-members/{member_id}/activity
GET    /api/v1/crew-members/{member_id}/performance
```

### Override Crew Member model (Pirate King)

```http
PATCH /api/v1/crew-members/{member_id}/model-profile
Content-Type: application/json
```

```json
{
  "override_model": "gemini-2.5-pro",
  "provider_preference": ["google_ai"],
  "reason": "Need higher quality output for this role"
}
```

Response `200 OK`:

```json
{
  "member_id": "member_seo_01J...",
  "model_route_profile": {
    "recommended_model": "gemini-2.5-flash",
    "recommended_reason": "Lookup/analysis role — fast model sufficient",
    "override_model": "gemini-2.5-pro",
    "override_by": "pirate_king",
    "override_reason": "Need higher quality output for this role",
    "effective_model": "gemini-2.5-pro",
    "provider_preference": ["google_ai"],
    "estimated_cost_per_day_usd": 0.80
  }
}
```

## Squad composition APIs

```http
POST   /api/v1/ships/{ship_id}/squad-proposals
GET    /api/v1/squad-proposals/{proposal_id}
PATCH  /api/v1/squad-proposals/{proposal_id}
POST   /api/v1/squad-proposals/{proposal_id}/submit
POST   /api/v1/squad-proposals/{proposal_id}/cancel
```

### Request AI-assisted squad composition

```http
POST /api/v1/ships/{ship_id}/squad-proposals
Content-Type: application/json
```

```json
{
  "prompt": "Buatkan saya 1 squad tim marketing",
  "template_id": null,
  "constraints": {
    "max_members": 10,
    "budget_ceiling_usd": 5.0
  }
}
```

Response `202 Accepted`:

```json
{
  "proposal_id": "squadprop_01J...",
  "ship_id": "ship_development",
  "status": "researching",
  "prompt": "Buatkan saya 1 squad tim marketing",
  "events_url": "/api/v1/squad-proposals/squadprop_01J.../events"
}
```

### Get proposal result (after Quartermaster research completes)

```http
GET /api/v1/squad-proposals/{proposal_id}
```

Response `200 OK`:

```json
{
  "proposal_id": "squadprop_01J...",
  "ship_id": "ship_development",
  "status": "awaiting_review",
  "squad_name": "Marketing Squad",
  "domain": "marketing",
  "rationale": "Based on industry-standard digital marketing teams, a squad of 7 members covers strategy, research, content creation, SEO, analytics, and brand governance.",
  "proposed_captain_index": 0,
  "members": [
    {
      "index": 0,
      "display_name": "Marketing Lead",
      "mission": "Plan campaigns and prioritize marketing work",
      "skills": ["campaign", "positioning", "funnel"],
      "tool_permissions": ["research", "report", "plan"],
      "tool_restrictions": ["external_publish"],
      "model_route_profile": {
        "recommended_model": "gemini-2.5-pro",
        "recommended_reason": "Strategic/lead role — needs multi-step reasoning and planning",
        "provider_preference": ["google_ai", "openrouter"],
        "fallback_model": "gemini-2.5-flash",
        "estimated_cost_per_day_usd": 0.80
      },
      "approval_policy": "no_external_activation"
    },
    {
      "index": 1,
      "display_name": "Market Researcher",
      "mission": "Find evidence about market, audience, and competitors",
      "skills": ["source_evaluation", "synthesis", "competitive_analysis"],
      "tool_permissions": ["web_search", "fetch", "rag", "report"],
      "tool_restrictions": [],
      "model_route_profile": {
        "recommended_model": "gemini-2.5-flash",
        "recommended_reason": "Lookup/analysis role — fast model sufficient for retrieval and synthesis",
        "provider_preference": ["google_ai", "openrouter"],
        "fallback_model": "gemini-2.5-flash-lite",
        "estimated_cost_per_day_usd": 0.40
      },
      "approval_policy": "read_only"
    }
  ],
  "budget_estimate_usd_per_day": 5.0,
  "ship_budget_remaining_usd": 13.76,
  "created_at": "2026-09-28T12:00:00Z"
}
```

### Modify proposal (Pirate King edits members)

```http
PATCH /api/v1/squad-proposals/{proposal_id}
Content-Type: application/json
```

```json
{
  "squad_name": "Marketing Squad",
  "proposed_captain_index": 0,
  "members": [
    {
      "index": 0,
      "display_name": "Marketing Lead",
      "mission": "Plan campaigns and prioritize marketing work",
      "skills": ["campaign", "positioning", "funnel", "budget_management"],
      "tool_permissions": ["research", "report", "plan"],
      "tool_restrictions": ["external_publish"],
      "model_profile": "balanced"
    }
  ],
  "removed_indices": [6]
}
```

### Submit finalized proposal

```http
POST /api/v1/squad-proposals/{proposal_id}/submit
```

Response `201 Created`:

```json
{
  "squad_id": "squad_marketing_01J...",
  "ship_id": "ship_development",
  "name": "Marketing Squad",
  "member_count": 6,
  "captain_member_id": "member_marketing_lead_01J...",
  "budget_allocated_usd_per_day": 5.0,
  "members": [
    {
      "id": "member_marketing_lead_01J...",
      "display_name": "Marketing Lead",
      "status": "idle"
    }
  ]
}
```

### Cancel proposal

```http
POST /api/v1/squad-proposals/{proposal_id}/cancel
```

Response `200 OK`:

```json
{
  "proposal_id": "squadprop_01J...",
  "status": "cancelled"
}
```

## Reports and escalation APIs

```http
GET  /api/v1/fleets/{fleet_id}/reports
POST /api/v1/fleets/{fleet_id}/reports/generate
GET  /api/v1/fleets/{fleet_id}/escalations
POST /api/v1/escalations/{escalation_id}/acknowledge
POST /api/v1/escalations/{escalation_id}/resolve
```

## Cross-Ship handoff APIs

```http
POST /api/v1/artifact-handoffs
GET  /api/v1/artifact-handoffs/{handoff_id}
POST /api/v1/artifact-handoffs/{handoff_id}/approve
POST /api/v1/artifact-handoffs/{handoff_id}/deny
```

### Handoff proposal

```json
{
  "source_ship_id": "ship_research",
  "destination_ship_id": "ship_marketing",
  "artifact_id": "artifact_market_research_01J",
  "handoff_mode": "redacted_summary",
  "purpose": "Use approved competitor research to inform content brief.",
  "classification": "internal"
}
```

## Error envelope

```json
{
  "error": {
    "code": "CROSS_SHIP_POLICY_DENIED",
    "message": "The artifact classification does not permit transfer to the requested Ship.",
    "request_id": "req_01J...",
    "retryable": false,
    "details": {
      "source_ship_id": "ship_research",
      "destination_ship_id": "ship_marketing"
    }
  }
}
```

---

## Existing Engine API Mapping

> The following table shows how existing `engine/` API routes map to Fleet Command equivalents.
> Entities marked ✅ already exist and primarily need renaming/scoping.

| Existing Route | Method | Module | Fleet Equivalent | Status |
|---|---|---|---|---|
| `/api/v1/crews` | GET | `src/crew` | `GET /api/v1/ships/{ship_id}/crew-members` | ✅ Rename + scope to Ship |
| `/api/v1/crews/{id}` | GET, PUT | `src/crew` | `GET/PATCH /api/v1/crew-members/{id}` | ✅ Rename |
| `/api/v1/runs` | POST | `src/run` | Scoped via `POST /api/v1/ships/{ship_id}/voyages` | ✅ Rename `Run` → `Voyage` |
| `/api/v1/runs/{id}` | GET | `src/run` | `GET /api/v1/voyages/{id}` | ✅ Rename |
| `/api/v1/runs/{id}/events` | GET (SSE) | `src/run` | `GET /api/v1/voyages/{id}/events` | ✅ Rename |
| `/api/v1/runs/{id}/cancel` | POST | `src/run` | `POST /api/v1/voyages/{id}/cancel` | ✅ Rename |
| `/api/v1/tasks/{id}` | GET | `src/task` | `GET /api/v1/job-orders/{id}` | ✅ Rename `Task` → `JobOrder` |
| `/api/v1/tasks/{id}/retry` | POST | `src/task` | `POST /api/v1/job-orders/{id}/retry` | ✅ Rename |
| `/api/v1/artifacts/{id}` | GET | `src/artifact` | `GET /api/v1/treasures/{id}` | ✅ Rename `Artifact` → `Treasure` |
| `/api/v1/artifacts/{id}/content` | GET | `src/artifact` | `GET /api/v1/treasures/{id}/content` | ✅ Rename |
| `/api/v1/workflows` | GET | `src/workflow` | `GET /api/v1/maps` | ✅ Rename `Workflow` → `Map` |
| `/api/v1/workflows/{id}` | GET | `src/workflow` | `GET /api/v1/maps/{id}` | ✅ Rename |
| `/api/v1/workflows/{id}/instantiate` | POST | `src/workflow` | `POST /api/v1/maps/{id}/instantiate` | ✅ Rename |
| `/api/turn` | POST (SSE) | `src/crew` | Internal — crew orchestration turn | ✅ Keep as-is |
| `/api/query` | POST | `src/crew` | Internal — vector search | ✅ Keep as-is |
| gRPC `StartTurn` | streaming | `src/crew` | Internal — agent turn via Rust gateway | ✅ Keep as-is |
| gRPC `QuickQuery` | unary | `src/crew` | Internal — vector query | ✅ Keep as-is |
| gRPC `HealthCheck` | unary | `src/crew` | `GET /api/v1/health` | ✅ Keep as-is |

> **New Fleet APIs** (no existing equivalent): Fleet CRUD, Ship CRUD, Quartermaster intake/reports/decision-briefs/queue, Fleet Orders, Reports/Escalations, Cross-Ship Handoffs.

