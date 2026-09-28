# Data Model

> Part of [Quartermaster — Fleet Command Architecture](00-overview.md)
>
> Struct conventions follow the existing `engine/src/crew/dto.go` pattern.

---

## Core entities

```go
package fleet

import "time"

// PirateKing represents the human owner and final authority
type PirateKing struct {
	ID        string    `json:"id"`
	Name      string    `json:"name"`
	CreatedAt time.Time `json:"created_at"`
	UpdatedAt time.Time `json:"updated_at"`
}

// Fleet represents a portfolio of Ships under one owner
type Fleet struct {
	ID              string      `json:"id"`
	PirateKingID    string      `json:"pirate_king_id"`
	Name            string      `json:"name"`
	Description     string      `json:"description"`
	QuartermasterID string      `json:"quartermaster_id"`
	PolicyRef       string      `json:"policy_ref"`
	BudgetRef       string      `json:"budget_ref"`
	Status          FleetStatus `json:"status"`
	CreatedAt       time.Time   `json:"created_at"`
	UpdatedAt       time.Time   `json:"updated_at"`
}

// Quartermaster represents the fleet-level coordination agent
type Quartermaster struct {
	ID                  string    `json:"id"`
	FleetID             string    `json:"fleet_id"`
	DisplayName         string    `json:"display_name"`
	Codename            string    `json:"codename"`
	Mission             string    `json:"mission"`
	SkillRefs           []SkillRef `json:"skill_refs"`
	ToolPolicyRef       string    `json:"tool_policy_ref"`
	ModelProfileRef     string    `json:"model_profile_ref"`
	MemoryPolicyRef     string    `json:"memory_policy_ref"`
	EscalationPolicyRef string    `json:"escalation_policy_ref"`
	MaxConcurrency      int       `json:"max_concurrency"`
	Enabled             bool      `json:"enabled"`
	Version             int64     `json:"version"`
}

// Ship represents an isolated workspace/project boundary
type Ship struct {
	ID              string     `json:"id"`
	FleetID         string     `json:"fleet_id"`
	Name            string     `json:"name"`
	Domain          ShipDomain `json:"domain"`
	Description     string     `json:"description"`
	CaptainMemberID string     `json:"captain_member_id"`
	WorkspaceRef    string     `json:"workspace_ref"`
	PolicyRef       string     `json:"policy_ref"`
	BudgetRef       string     `json:"budget_ref"`
	MemoryScopeRef  string     `json:"memory_scope_ref"`
	Status          ShipStatus `json:"status"`
	Version         int64      `json:"version"`
	CreatedAt       time.Time  `json:"created_at"`
	UpdatedAt       time.Time  `json:"updated_at"`
}

// Squad represents a functional group inside a Ship
type Squad struct {
	ID                     string      `json:"id"`
	ShipID                 string      `json:"ship_id"`
	Name                   string      `json:"name"`
	LeadMemberID           string      `json:"lead_member_id"`
	DefaultToolPolicyRef   string      `json:"default_tool_policy_ref"`
	DefaultModelProfileRef string      `json:"default_model_profile_ref"`
	Status                 SquadStatus `json:"status"`
}

// CrewMember represents a specialist AI agent with bounded role/skills/tools
type CrewMember struct {
	ID                string           `json:"id"`
	ShipID            string           `json:"ship_id"`
	SquadID           string           `json:"squad_id"`
	RoleID            string           `json:"role_id"`
	DisplayName       string           `json:"display_name"`
	Mission           string           `json:"mission"`
	Status            CrewMemberStatus `json:"status"`
	SkillRefs         []SkillRef       `json:"skill_refs"`
	ToolPolicyRef     string           `json:"tool_policy_ref"`
	ModelProfileRef   string           `json:"model_profile_ref"`
	MemoryPolicyRef   string           `json:"memory_policy_ref"`
	EvaluationProfile string           `json:"evaluation_profile"`
	BudgetCeiling     BudgetLimit      `json:"budget_ceiling"`
	MaxConcurrency    int              `json:"max_concurrency"`
	Enabled           bool             `json:"enabled"`
	Version           int64            `json:"version"`
}
```

## Work entities

```go
package fleet

import "time"

// FleetOrderProposal represents a proposed multi-ship work plan
type FleetOrderProposal struct {
	ID              string          `json:"id"`
	FleetID         string          `json:"fleet_id"`
	RequestedBy     string          `json:"requested_by"`
	Objective       string          `json:"objective"`
	ProposedShipIDs []string        `json:"proposed_ship_ids"`
	Dependencies    []ShipDependency `json:"dependencies"`
	BudgetEstimate  BudgetEstimate  `json:"budget_estimate"`
	RiskSummary     RiskSummary     `json:"risk_summary"`
	Status          ProposalStatus  `json:"status"`
	CreatedAt       time.Time       `json:"created_at"`
}

// Voyage represents a workflow or run within a Ship
type Voyage struct {
	ID               string       `json:"id"`
	FleetID          string       `json:"fleet_id"`
	ShipID           string       `json:"ship_id"`
	ParentVoyageID   *string      `json:"parent_voyage_id,omitempty"`
	FleetOrderID     *string      `json:"fleet_order_id,omitempty"`
	Name             string       `json:"name"`
	Objective        string       `json:"objective"`
	Status           VoyageStatus `json:"status"`
	Budget           BudgetLimit  `json:"budget"`
	PolicySnapshotID string       `json:"policy_snapshot_id"`
	StartedAt        *time.Time   `json:"started_at,omitempty"`
	CompletedAt      *time.Time   `json:"completed_at,omitempty"`
}

// JobOrder represents a task assignment for a crew member
type JobOrder struct {
	ID                string        `json:"id"`
	VoyageID          string        `json:"voyage_id"`
	ParentJobOrderID  *string       `json:"parent_job_order_id,omitempty"`
	AssignedMemberID  string        `json:"assigned_member_id"`
	Objective         string        `json:"objective"`
	InputArtifacts    []ArtifactRef `json:"input_artifacts"`
	RequiredSkills    []string      `json:"required_skills"`
	RequiredTools     []string      `json:"required_tools"`
	Status            JobOrderStatus `json:"status"`
	Budget            BudgetLimit   `json:"budget"`
	ApprovalPolicyRef string        `json:"approval_policy_ref"`
	OutputContractRef string        `json:"output_contract_ref"`
}
```

## Reporting entities

```go
package fleet

import "time"

// ShipReport represents a status report submitted by a Ship Captain
type ShipReport struct {
	ID             string         `json:"id"`
	ShipID         string         `json:"ship_id"`
	VoyageID       *string        `json:"voyage_id,omitempty"`
	Status         ReportStatus   `json:"status"`
	Summary        string         `json:"summary"`
	Highlights     []string       `json:"highlights"`
	Blockers       []Blocker      `json:"blockers"`
	Risks          []RiskItem     `json:"risks"`
	BudgetSnapshot BudgetSnapshot `json:"budget_snapshot"`
	ArtifactRefs   []ArtifactRef  `json:"artifact_refs"`
	EscalationRefs []string       `json:"escalation_refs"`
	CreatedAt      time.Time      `json:"created_at"`
}

// FleetReport represents a consolidated executive summary
type FleetReport struct {
	ID               string         `json:"id"`
	FleetID          string         `json:"fleet_id"`
	WindowStart      time.Time      `json:"window_start"`
	WindowEnd        time.Time      `json:"window_end"`
	OverallStatus    FleetStatus    `json:"overall_status"`
	ExecutiveSummary string         `json:"executive_summary"`
	ShipSummaries    []ShipSummary  `json:"ship_summaries"`
	DecisionsNeeded  []DecisionItem `json:"decisions_needed"`
	TopRisks         []RiskItem     `json:"top_risks"`
	BudgetSnapshot   BudgetSnapshot `json:"budget_snapshot"`
	ArtifactRefs     []ArtifactRef  `json:"artifact_refs"`
	CreatedAt        time.Time      `json:"created_at"`
}

// Escalation represents a risk or decision that needs attention
type Escalation struct {
	ID                string             `json:"id"`
	FleetID           string             `json:"fleet_id"`
	ShipID            *string            `json:"ship_id,omitempty"`
	VoyageID          *string            `json:"voyage_id,omitempty"`
	Severity          EscalationSeverity `json:"severity"`
	Category          EscalationCategory `json:"category"`
	Summary           string             `json:"summary"`
	RecommendedAction string             `json:"recommended_action"`
	Status            EscalationStatus   `json:"status"`
	CreatedAt         time.Time          `json:"created_at"`
}
```

## Persistence tables

```text
pirate_kings
fleets
quartermasters
fleet_policies
fleet_budget_limits
ships
ship_policies
ship_budget_limits
squads
crew_members
role_templates
skills
skill_versions
crew_member_skills
fleet_orders
fleet_order_ship_links
voyages
job_orders
ship_reports
fleet_reports
escalations
artifact_handoff_proposals
artifact_handoff_receipts
fleet_knowledge_entries
lesson_promotion_proposals
audit_events
approval_requests
```

## Key relationships

```text
PirateKing 1 ─── * Fleet
Fleet      1 ─── 1 Quartermaster
Fleet      1 ─── * Ship
Ship       1 ─── * Squad
Squad      1 ─── * CrewMember
Ship       1 ─── * Voyage
Voyage     1 ─── * JobOrder
Ship       * ─── * Artifact via ownership/access policy
Fleet      1 ─── * FleetReport
Ship       1 ─── * ShipReport
```

---

## Existing Engine Entity Mapping

> The following shows how current `engine/src/` structs map to Fleet Command entities.

### Run → Voyage (rename + extend)

```go
// Existing: engine/src/run/dto.go
type Run struct {
    ID           string       `json:"id"`
    CrewID       string       `json:"crew_id"`        // → ShipID
    WorkflowID   string       `json:"workflow_id"`     // → FleetOrderID
    Status       RunStatus    `json:"status"`          // → VoyageStatus (extended)
    Input        RunInput     `json:"input"`           // → Objective
    Workspace    WorkspaceConfig `json:"workspace"`    // → resolved from Ship
    Options      RunOptions   `json:"options"`
    TasksSummary TasksSummary `json:"tasks_summary"`
    CreatedAt    time.Time    `json:"created_at"`
    StartedAt    *time.Time   `json:"started_at,omitempty"`
    CompletedAt  *time.Time   `json:"completed_at,omitempty"`
}
// → Voyage adds: FleetID, ParentVoyageID, Budget, PolicySnapshotID
```

### Task → JobOrder (rename + extend)

```go
// Existing: engine/src/task/dto.go
type Task struct {
    ID           string     `json:"id"`
    RunID        string     `json:"run_id"`          // → VoyageID
    Title        string     `json:"title"`            // → Objective
    AssignedTo   string     `json:"assigned_to"`      // → AssignedMemberID
    Status       TaskStatus `json:"status"`           // → JobOrderStatus
    Dependencies []string   `json:"dependencies"`
}
// → JobOrder adds: ParentJobOrderID, InputArtifacts, RequiredSkills, RequiredTools,
//   Budget, ApprovalPolicyRef, OutputContractRef
```

### AgentDefinition → CrewMember (extend)

```go
// Existing: engine/src/crew/dto.go
type AgentDefinition struct {
    ID           string      `json:"id"`
    Name         string      `json:"name"`         // → DisplayName
    Role         string      `json:"role"`         // → RoleID + Mission
    Status       AgentStatus `json:"status"`       // → CrewMemberStatus
    Capabilities []string    `json:"capabilities"` // → SkillRefs
}
// → CrewMember adds: ShipID, SquadID, ToolPolicyRef, ModelProfileRef,
//   MemoryPolicyRef, EvaluationProfile, BudgetCeiling, MaxConcurrency
```

### Artifact → with classification (extend)

```go
// Existing: engine/src/artifact/dto.go
type Artifact struct {
    ID        string    `json:"id"`
    RunID     string    `json:"run_id"`     // → VoyageID
    TaskID    string    `json:"task_id"`    // → JobOrderID
    Type      string    `json:"type"`
    Path      string    `json:"path"`
    Summary   string    `json:"summary"`
    MimeType  string    `json:"mime_type"`
    Content   string    `json:"content"`
    Hash      string    `json:"hash"`
    SizeBytes int64     `json:"size_bytes"`
    CreatedAt time.Time `json:"created_at"`
}
// → Add: ShipID, Classification, ShareableToFleet
```

### Existing Tool Policy (already aligns)

```go
// Existing: engine/src/tool/dto.go — already implements Fleet-compatible policy
type ExecutionContext struct {
    ActorID            string   `json:"actor_id"`
    WorkspaceID        string   `json:"workspace_id"`   // → ShipID scope
    CrewID             string   `json:"crew_id"`         // → SquadID scope
    RunID              string   `json:"run_id"`          // → VoyageID scope
    TaskID             string   `json:"task_id"`         // → JobOrderID scope
    DataClassification string   `json:"data_classification"` // ✅ Already exists!
    Capabilities       []string `json:"capabilities"`    // ✅ Already exists!
}

// PolicyVerdict: ALLOW, REQUIRE_APPROVAL, DENY — ✅ Already exists!
// RiskClass: read, compute, network_read, write_draft, write_workspace,
//   execute_sandboxed, execute_privileged, external_action,
//   credential_access — ✅ Already exists!
```

