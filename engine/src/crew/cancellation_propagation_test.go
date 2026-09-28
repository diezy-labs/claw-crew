package crew_test

import (
	"context"
	"testing"
	"time"

	"github.com/diezy-labs/claw-crew/engine/core/config"
	"github.com/diezy-labs/claw-crew/engine/pkg/client"
	"github.com/diezy-labs/claw-crew/engine/src/crew"
	"github.com/diezy-labs/claw-crew/engine/src/llm"
)

func TestCancellationPropagationToSubagents(t *testing.T) {
	cfg := &config.AppConfig{}
	gateway, err := client.NewSystemGatewayClient("")
	if err != nil {
		t.Fatalf("failed to create gateway: %v", err)
	}

	provider := llm.NewProvider(cfg, gateway)
	dispatcher := llm.NewToolDispatcher(gateway)
	orchestrator := crew.NewService(provider, dispatcher, gateway)

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	turnReq := &crew.TurnRequest{
		SessionID: "session_subagent_cancel",
		AgentID:   "parent_agent",
		Prompt:    "delegate subagent to run long task",
	}

	eventCh := make(chan *crew.TurnEvent, 64)
	errCh := make(chan error, 1)

	go func() {
		defer close(eventCh)
		errCh <- orchestrator.StartTurn(ctx, turnReq, eventCh)
	}()

	// Wait until subagent is spawned
	var subagentSpawned bool
	for event := range eventCh {
		if event.Type == crew.EventSubagentSpawned {
			subagentSpawned = true
			// Cancel parent context immediately upon subagent spawn
			cancel()
			break
		}
	}

	if !subagentSpawned {
		t.Fatalf("expected subagent to be spawned")
	}

	// Drain remaining events to prevent hanging
	for range eventCh {
	}

	// Ensure StartTurn exits cleanly in response to cancel
	select {
	case err := <-errCh:
		// Should return timeout/cancel error or nil
		t.Logf("StartTurn exited with: %v", err)
	case <-time.After(2 * time.Second):
		t.Fatalf("StartTurn timed out waiting for subagents to terminate upon cancellation")
	}
}
