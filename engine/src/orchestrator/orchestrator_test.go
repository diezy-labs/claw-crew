package orchestrator_test

import (
	"context"
	"testing"

	"github.com/diezy-labs/claw-crew/engine/src/fleet"
	"github.com/diezy-labs/claw-crew/engine/src/llm"
	"github.com/diezy-labs/claw-crew/engine/src/orchestrator"
)

// A simple mock for fleet service
type mockFleet struct {
	fleet.Service
}

func (m *mockFleet) GetFleet(ctx context.Context, id string) (*fleet.Fleet, error) {
	return &fleet.Fleet{ID: id, Name: "Test Fleet"}, nil
}

func TestQuartermasterObjective(t *testing.T) {
	mockLLM := llm.NewMockProvider("test-model", "Here is a drafted Squad for your objective.")
	fleetSvc := &mockFleet{}
	
	service := orchestrator.NewService(mockLLM, fleetSvc)
	
	req := orchestrator.ObjectiveRequest{
		FleetID:   "flt_123",
		Objective: "Build a new marketing campaign",
	}
	
	resp, err := service.ProcessObjective(context.Background(), req)
	if err != nil {
		t.Fatalf("expected no error, got %v", err)
	}
	
	if resp.Status == "" {
		t.Errorf("expected status response, got empty")
	}
}
