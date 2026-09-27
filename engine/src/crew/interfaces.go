package crew

import (
	"context"
)

// Orchestrator mendefinisikan antarmuka manajemen multi-agent
type Orchestrator interface {
	StartTurn(ctx context.Context, req *TurnRequest, eventCh chan<- *TurnEvent) error
	CancelTurn(ctx context.Context, sessionID string) error
}
