package crew

import (
	"context"
)

// Orchestrator defines the contract for managing multi-agent lifecycles and turns
type Orchestrator interface {
	StartTurn(ctx context.Context, req *TurnRequest, eventCh chan<- *TurnEvent) error
	CancelTurn(ctx context.Context, sessionID string) error
}
