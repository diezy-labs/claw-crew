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
	ListCrews(ctx context.Context) ([]*CrewDefinition, error)
	GetCrew(ctx context.Context, id string) (*CrewDefinition, error)
	RegisterCrew(ctx context.Context, crew *CrewDefinition) error
	UpdateAgentStatus(ctx context.Context, crewID, agentID string, status AgentStatus) error
}
