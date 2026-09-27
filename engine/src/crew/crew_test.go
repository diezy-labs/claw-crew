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

func TestCrewOrchestratorTurn(t *testing.T) {
	cfg := &config.AppConfig{}
	gateway, err := client.NewSystemGatewayClient("")
	if err != nil {
		t.Fatalf("failed to create gateway: %v", err)
	}

	provider := llm.NewProvider(cfg, gateway)
	dispatcher := llm.NewToolDispatcher(gateway)
	orchestrator := crew.NewService(provider, dispatcher, gateway)

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	turnReq := &crew.TurnRequest{
		SessionID: "test_session_1",
		AgentID:   "qa_agent",
		Prompt:    "Please delegate subagent to verify results",
	}

	eventCh := make(chan *crew.TurnEvent, 32)
	errCh := make(chan error, 1)

	go func() {
		defer close(eventCh)
		errCh <- orchestrator.StartTurn(ctx, turnReq, eventCh)
	}()

	var receivedThought bool
	var receivedText bool
	var receivedSubagent bool
	var receivedCompleted bool

	for event := range eventCh {
		switch event.Type {
		case crew.EventThoughtChunk:
			receivedThought = true
		case crew.EventTextChunk:
			receivedText = true
		case crew.EventSubagentSpawned:
			receivedSubagent = true
		case crew.EventTurnCompleted:
			receivedCompleted = true
		}
	}

	if err := <-errCh; err != nil {
		t.Fatalf("unexpected turn error: %v", err)
	}

	if !receivedThought {
		t.Errorf("expected thought chunk event")
	}
	if !receivedText {
		t.Errorf("expected text chunk event")
	}
	if !receivedSubagent {
		t.Errorf("expected subagent spawned event")
	}
	if !receivedCompleted {
		t.Errorf("expected turn completed event")
	}
}

func TestCrewCancelTurn(t *testing.T) {
	cfg := &config.AppConfig{}
	gateway, err := client.NewSystemGatewayClient("")
	if err != nil {
		t.Fatalf("failed to create gateway: %v", err)
	}

	provider := llm.NewProvider(cfg, gateway)
	dispatcher := llm.NewToolDispatcher(gateway)
	orchestrator := crew.NewService(provider, dispatcher, gateway)

	ctx := context.Background()
	turnReq := &crew.TurnRequest{
		SessionID: "cancel_session",
		AgentID:   "qa_agent",
		Prompt:    "Long running task",
	}

	eventCh := make(chan *crew.TurnEvent, 32)
	go func() {
		defer close(eventCh)
		_ = orchestrator.StartTurn(ctx, turnReq, eventCh)
	}()

	// Allow turn to register
	time.Sleep(10 * time.Millisecond)

	// Cancel the turn
	if err := orchestrator.CancelTurn(ctx, "cancel_session"); err != nil {
		t.Fatalf("failed to cancel turn: %v", err)
	}

	// Drain channel
	for range eventCh {
	}
}
