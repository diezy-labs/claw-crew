package crew_test

import (
	"context"
	"sync"
	"testing"

	"github.com/diezy-labs/claw-crew/engine/src/run"
)

func TestConcurrentRunFinalization(t *testing.T) {
	hub := run.NewEventHub()
	store := run.NewMemoryStore()
	svc := run.NewService(store, hub)

	ctx := context.Background()
	r, err := svc.CreateRun(ctx, &run.CreateRunRequest{
		CrewID: "crew_concurrent_final",
		Input:  run.RunInput{Prompt: "Concurrency test"},
	}, "req_conc", "")
	if err != nil {
		t.Fatalf("failed to create run: %v", err)
	}

	// Move to running first
	if err := store.UpdateStatus(ctx, r.ID, run.StatusRunning, "started"); err != nil {
		t.Fatalf("failed to transition to running: %v", err)
	}

	const callers = 20
	var wg sync.WaitGroup
	wg.Add(callers)

	// Half try to Cancel, half try to Complete
	for i := 0; i < callers; i++ {
		go func(idx int) {
			defer wg.Done()
			if idx%2 == 0 {
				_ = svc.CancelRun(ctx, r.ID)
			} else {
				_ = store.UpdateStatus(ctx, r.ID, run.StatusCompleted, "completed by worker")
			}
		}(i)
	}

	wg.Wait()

	finalRun, err := svc.GetRun(ctx, r.ID)
	if err != nil {
		t.Fatalf("failed to get run: %v", err)
	}

	// Must be in a valid terminal state (either Cancelled or Completed)
	if finalRun.Status != run.StatusCancelled && finalRun.Status != run.StatusCompleted {
		t.Fatalf("unexpected final status: %s", finalRun.Status)
	}
}
