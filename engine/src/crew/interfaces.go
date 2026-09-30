package crew

import (
	"context"
)

// Orchestrator defines the contract for managing multi-agent lifecycles and turns
type Orchestrator interface {
	StartTurn(ctx context.Context, req *TurnRequest, eventCh chan<- *TurnEvent) error
	CancelTurn(ctx context.Context, sessionID string) error
}

// Registry defines storage and retrieval for crew and agent definitions
type Registry interface {
	ListSquads(ctx context.Context) ([]*Squad, error)
	GetSquad(ctx context.Context, id string) (*Squad, error)
	RegisterSquad(ctx context.Context, crew *Squad) error
	UpdateCrewMemberStatus(ctx context.Context, crewID, agentID string, status CrewMemberStatus) error
}
