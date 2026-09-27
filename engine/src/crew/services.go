package crew

import (
	"context"
	"fmt"
	"sync"
	"time"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/core/metrics"
)

type service struct {
	mu          sync.RWMutex
	activeTurns map[string]context.CancelFunc
}

// NewService creates a new crew.Orchestrator instance
func NewService() Orchestrator {
	return &service{
		activeTurns: make(map[string]context.CancelFunc),
	}
}

func (s *service) StartTurn(ctx context.Context, req *TurnRequest, eventCh chan<- *TurnEvent) error {
	if req.SessionID == "" {
		return appErrors.New(appErrors.CodeInvalidArgument, "session_id cannot be empty", appErrors.LayerService)
	}
	if req.Prompt == "" {
		return appErrors.New(appErrors.CodeInvalidArgument, "prompt cannot be empty", appErrors.LayerService)
	}

	turnCtx, cancel := context.WithCancel(ctx)
	s.mu.Lock()
	s.activeTurns[req.SessionID] = cancel
	s.mu.Unlock()

	defer func() {
		s.mu.Lock()
		delete(s.activeTurns, req.SessionID)
		s.mu.Unlock()
		cancel()
	}()

	metrics.ActiveAgents.Inc()
	defer metrics.ActiveAgents.Dec()

	start := time.Now()
	agentID := req.AgentID
	if agentID == "" {
		agentID = "default_agent"
	}

	defer func() {
		duration := time.Since(start).Seconds()
		metrics.AgentTurnDuration.WithLabelValues(agentID, "completed").Observe(duration)
	}()

	// Event 1: Thought Chunk
	select {
	case <-turnCtx.Done():
		return appErrors.New(appErrors.CodeTimeout, "turn cancelled or timed out", appErrors.LayerService)
	case eventCh <- &TurnEvent{
		Type:    EventThoughtChunk,
		Content: fmt.Sprintf("Analyzing user prompt for agent '%s'...", agentID),
	}:
	}

	// Event 2: Text Chunk
	select {
	case <-turnCtx.Done():
		return appErrors.New(appErrors.CodeTimeout, "turn cancelled or timed out", appErrors.LayerService)
	case eventCh <- &TurnEvent{
		Type:    EventTextChunk,
		Content: fmt.Sprintf("Go 1.27 Agent Engine received prompt: \"%s\".", req.Prompt),
	}:
	}

	// Event 3: Turn Completed
	select {
	case <-turnCtx.Done():
		return appErrors.New(appErrors.CodeTimeout, "turn cancelled or timed out", appErrors.LayerService)
	case eventCh <- &TurnEvent{
		Type:    EventTurnCompleted,
		Content: "Turn completed successfully.",
	}:
	}

	return nil
}

func (s *service) CancelTurn(ctx context.Context, sessionID string) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	cancel, found := s.activeTurns[sessionID]
	if !found {
		return appErrors.New(appErrors.CodeNotFound, fmt.Sprintf("no active turn found for session %s", sessionID), appErrors.LayerService)
	}

	cancel()
	delete(s.activeTurns, sessionID)
	return nil
}
