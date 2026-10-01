package orchestrator_test

import (
	"context"
	"testing"

	"github.com/diezy-labs/claw-crew/engine/src/llm"
	"github.com/diezy-labs/claw-crew/engine/src/orchestrator"
)

// F2-3: an objective yields a gated FleetOrderProposal — never execution.
func TestIntakeObjective_RawFallbackIsGated(t *testing.T) {
	mockLLM := llm.NewMockProvider("test-model")
	mockLLM.SetResponses([]string{"We will need a backend engineer and a QA specialist."})
	svc := orchestrator.NewService(mockLLM, &mockFleet{})

	p, err := svc.IntakeObjective(context.Background(), orchestrator.ObjectiveRequest{Objective: "ship the API"})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if p.Status != orchestrator.ProposalStatusAwaitingApproval {
		t.Fatalf("proposal must be gated; got status %q", p.Status)
	}
	if p.Objective != "ship the API" {
		t.Errorf("objective not carried through: %q", p.Objective)
	}
	if p.Summary == "" {
		t.Errorf("expected a summary from the raw LLM draft")
	}
}

// A clean JSON draft is parsed into structured proposed roles, still gated.
func TestIntakeObjective_JSONDraftParsesRoles(t *testing.T) {
	mockLLM := llm.NewMockProvider("test-model")
	mockLLM.SetResponses([]string{`{"mission_name":"Launch","required_crew":[{"role":"Engineer","capabilities":["go","grpc"]}]}`})
	svc := orchestrator.NewService(mockLLM, &mockFleet{})

	p, err := svc.IntakeObjective(context.Background(), orchestrator.ObjectiveRequest{Objective: "launch it"})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if p.MissionName != "Launch" {
		t.Errorf("expected parsed mission name, got %q", p.MissionName)
	}
	if len(p.ProposedCrew) != 1 || p.ProposedCrew[0].Role != "Engineer" {
		t.Fatalf("expected one Engineer role, got %+v", p.ProposedCrew)
	}
	if p.Status != orchestrator.ProposalStatusAwaitingApproval {
		t.Errorf("proposal must stay gated; got %q", p.Status)
	}
}

// ProposeFleetOrder adapts to the fleet-facing view, preserving the gate.
func TestProposeFleetOrder_MapsAndGates(t *testing.T) {
	mockLLM := llm.NewMockProvider("test-model")
	mockLLM.SetResponses([]string{`{"mission_name":"Recon","required_crew":[]}`})
	svc := orchestrator.NewService(mockLLM, &mockFleet{})

	order, err := svc.ProposeFleetOrder(context.Background(), "scout the sector")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if order.MissionName != "Recon" {
		t.Errorf("expected mission Recon, got %q", order.MissionName)
	}
	if order.Status != orchestrator.ProposalStatusAwaitingApproval {
		t.Errorf("fleet-facing order must be gated; got %q", order.Status)
	}
}
