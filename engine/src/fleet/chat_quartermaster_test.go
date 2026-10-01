package fleet

import (
	"context"
	"strings"
	"testing"

	"github.com/diezy-labs/claw-crew/engine/src/llm"
)

// TestChatQuartermaster_UsesLLM verifies F2-1: the chat branch streams a real
// completion from the provider instead of string-matching.
func TestChatQuartermaster_UsesLLM(t *testing.T) {
	mock := llm.NewMockProvider("test-model")
	mock.SetResponses([]string{"Aye, Pirate King — fleet nominal."})
	svc := NewService(mock)

	resp, err := svc.ChatQuartermaster(context.Background(), "how is the fleet?")
	if err != nil {
		t.Fatalf("ChatQuartermaster: %v", err)
	}
	if !strings.Contains(resp.Reply, "fleet nominal") {
		t.Errorf("expected LLM content in reply, got: %q", resp.Reply)
	}
}

// TestChatQuartermaster_FallbackWhenNoProvider verifies the safe fallback path
// (provider offline) never fails the turn.
func TestChatQuartermaster_FallbackWhenNoProvider(t *testing.T) {
	svc := NewService(nil)
	resp, err := svc.ChatQuartermaster(context.Background(), "hello")
	if err != nil {
		t.Fatalf("expected graceful fallback, got error: %v", err)
	}
	if resp == nil || resp.Reply == "" {
		t.Error("expected a non-empty fallback reply")
	}
}

// stubProposer satisfies ObjectiveProposer for the objective-branch test.
type stubProposer struct{ called bool }

func (p *stubProposer) ProposeFleetOrder(ctx context.Context, objective string) (*ProposedFleetOrder, error) {
	p.called = true
	return &ProposedFleetOrder{
		Objective:   objective,
		MissionName: "Build API",
		Summary:     "Drafted 2 roles.",
		Status:      "awaiting_pirate_king_approval",
	}, nil
}

// TestChatQuartermaster_ObjectiveUsesProposer verifies F2-3: an objective prompt
// routes to the proposer and surfaces a gated proposal (never executes).
func TestChatQuartermaster_ObjectiveUsesProposer(t *testing.T) {
	svc := NewService(nil)
	p := &stubProposer{}
	svc.SetObjectiveProposer(p)

	resp, err := svc.ChatQuartermaster(context.Background(), "build the new upload API")
	if err != nil {
		t.Fatalf("ChatQuartermaster: %v", err)
	}
	if !p.called {
		t.Fatal("objective branch did not call the proposer")
	}
	if !strings.Contains(resp.Reply, "Build API") || !strings.Contains(resp.Reply, "awaits your approval") {
		t.Errorf("expected gated proposal reply, got: %q", resp.Reply)
	}
	if len(resp.SuggestedActions) == 0 || resp.SuggestedActions[0].Payload["status"] != "awaiting_pirate_king_approval" {
		t.Errorf("expected gated status in suggested action, got: %+v", resp.SuggestedActions)
	}
}

// TestChatQuartermaster_ObjectiveFallbackWithoutProposer verifies the safe ack
// when no proposer is wired.
func TestChatQuartermaster_ObjectiveFallbackWithoutProposer(t *testing.T) {
	svc := NewService(nil) // no proposer injected
	resp, err := svc.ChatQuartermaster(context.Background(), "implement the parser")
	if err != nil {
		t.Fatalf("expected graceful ack, got error: %v", err)
	}
	if resp == nil || !strings.Contains(resp.Reply, "approval") {
		t.Errorf("expected safe acknowledge, got: %q", resp.Reply)
	}
}
