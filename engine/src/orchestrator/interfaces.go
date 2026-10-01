package orchestrator

import (
	"context"

	"github.com/diezy-labs/claw-crew/engine/src/fleet"
)

// ProposalStatusAwaitingApproval is the only status IntakeObjective ever emits:
// the Quartermaster proposes, the Pirate King approves. Execution (Captain) is
// gated behind this (docs/finalize/01 "objective" branch, F2-3 acceptance).
const ProposalStatusAwaitingApproval = "awaiting_pirate_king_approval"

type Service interface {
	ProcessObjective(ctx context.Context, req ObjectiveRequest) (*ObjectiveResponse, error)
	// IntakeObjective turns a Pirate King objective into a structured, gated
	// FleetOrderProposal. It NEVER executes — the proposal awaits approval.
	IntakeObjective(ctx context.Context, req ObjectiveRequest) (*FleetOrderProposal, error)
	// ProposeFleetOrder is the fleet-facing adapter (satisfies fleet.ObjectiveProposer).
	ProposeFleetOrder(ctx context.Context, objective string) (*fleet.ProposedFleetOrder, error)
	CoordinateFleet(ctx context.Context, fleetID string) error
}
type ObjectiveRequest struct {
	FleetID   string
	Objective string
}
type ObjectiveResponse struct {
	Status string
}

// ProposedCrewRole is one role the Quartermaster suggests the Squad will need.
// Minimal by design (name + why): the full CrewMember is composed later, after
// approval — the proposal is a plan, not an assignment.
type ProposedCrewRole struct {
	Role   string `json:"role"`
	Reason string `json:"reason"`
}

// FleetOrderProposal is the typed output of the objective branch: a draft plan
// the Pirate King reviews. Status is always awaiting approval; it carries no
// side effects and starts no voyage until explicitly approved.
type FleetOrderProposal struct {
	Objective    string             `json:"objective"`
	MissionName  string             `json:"mission_name"`
	Summary      string             `json:"summary"`
	ProposedCrew []ProposedCrewRole `json:"proposed_crew"`
	Status       string             `json:"status"` // always ProposalStatusAwaitingApproval
}
