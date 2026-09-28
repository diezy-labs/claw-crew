package crew_test

import (
	"context"
	"testing"

	"github.com/diezy-labs/claw-crew/engine/src/run"
)

func TestRunIdempotency(t *testing.T) {
	store := run.NewMemoryStore()
	hub := run.NewEventHub()
	svc := run.NewService(store, hub)

	ctx := context.Background()
	idempotencyKey := "idemp-key-unique-12345"

	req := &run.CreateRunRequest{
		CrewID: "crew_idemp",
		Input:  run.RunInput{Prompt: "Initial attempt"},
	}

	// First creation attempt
	run1, err := svc.CreateRun(ctx, req, "req-1", idempotencyKey)
	if err != nil {
		t.Fatalf("first CreateRun failed: %v", err)
	}

	// Second creation attempt with the same idempotency key
	run2, err := svc.CreateRun(ctx, req, "req-2", idempotencyKey)
	if err != nil {
		t.Fatalf("second CreateRun failed: %v", err)
	}

	// Must return the exact same run instance and ID
	if run1.ID != run2.ID {
		t.Fatalf("expected duplicate request with key %s to return identical run ID, got %s and %s",
			idempotencyKey, run1.ID, run2.ID)
	}
}
