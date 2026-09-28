package crew_test

import (
	"context"
	"testing"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/src/run"
)

func TestRunStateMachine_MonotonicTransitions(t *testing.T) {
	ctx := context.Background()
	store := run.NewMemoryStore()
	hub := run.NewEventHub()
	svc := run.NewService(store, hub)

	r, err := svc.CreateRun(ctx, &run.CreateRunRequest{
		CrewID: "test_crew",
		Input:  run.RunInput{Prompt: "test prompt"},
	}, "req_monotonic", "")
	if err != nil {
		t.Fatalf("failed to create run: %v", err)
	}

	if r.Status != run.StatusQueued {
		t.Fatalf("expected initial status %s, got %s", run.StatusQueued, r.Status)
	}

	// queued -> planning (valid)
	if err := store.UpdateStatus(ctx, r.ID, run.StatusPlanning, "planning phase"); err != nil {
		t.Fatalf("failed transition queued -> planning: %v", err)
	}

	// planning -> running (valid)
	if err := store.UpdateStatus(ctx, r.ID, run.StatusRunning, "execution started"); err != nil {
		t.Fatalf("failed transition planning -> running: %v", err)
	}

	// running -> waiting_for_input (valid)
	if err := store.UpdateStatus(ctx, r.ID, run.StatusWaitingForInput, "waiting for user input"); err != nil {
		t.Fatalf("failed transition running -> waiting_for_input: %v", err)
	}

	// waiting_for_input -> running (valid)
	if err := store.UpdateStatus(ctx, r.ID, run.StatusRunning, "resumed execution"); err != nil {
		t.Fatalf("failed transition waiting_for_input -> running: %v", err)
	}

	// running -> cancelling (valid)
	if err := store.UpdateStatus(ctx, r.ID, run.StatusCancelling, "cancelling"); err != nil {
		t.Fatalf("failed transition running -> cancelling: %v", err)
	}

	// cancelling -> cancelled (valid)
	if err := store.UpdateStatus(ctx, r.ID, run.StatusCancelled, "cancelled"); err != nil {
		t.Fatalf("failed transition cancelling -> cancelled: %v", err)
	}

	// cancelled -> running (ILLEGAL jump from terminal state)
	err = store.UpdateStatus(ctx, r.ID, run.StatusRunning, "resurrect")
	if err == nil {
		t.Fatalf("expected error transitioning cancelled -> running, got nil")
	}
	if appErr, ok := err.(*appErrors.AppError); ok {
		if appErr.Code != appErrors.CodeFailedPrecondition {
			t.Errorf("expected CodeFailedPrecondition, got %v", appErr.Code)
		}
	}

	// cancelled -> completed (ILLEGAL jump from terminal state)
	err = store.UpdateStatus(ctx, r.ID, run.StatusCompleted, "complete cancelled run")
	if err == nil {
		t.Fatalf("expected error transitioning cancelled -> completed, got nil")
	}
}

func TestRunStateMachine_IllegalTransitions(t *testing.T) {
	testCases := []struct {
		name     string
		initial  run.RunStatus
		next     run.RunStatus
		expected bool
	}{
		{"queued to planning", run.StatusQueued, run.StatusPlanning, true},
		{"queued to running", run.StatusQueued, run.StatusRunning, true},
		{"queued to cancelled", run.StatusQueued, run.StatusCancelled, true},
		{"queued to completed", run.StatusQueued, run.StatusCompleted, false},
		{"running to completed", run.StatusRunning, run.StatusCompleted, true},
		{"running to failed", run.StatusRunning, run.StatusFailed, true},
		{"completed to running", run.StatusCompleted, run.StatusRunning, false},
		{"completed to cancelled", run.StatusCompleted, run.StatusCancelled, false},
		{"failed to running", run.StatusFailed, run.StatusRunning, false},
		{"cancelled to running", run.StatusCancelled, run.StatusRunning, false},
	}

	for _, tc := range testCases {
		t.Run(tc.name, func(t *testing.T) {
			r := &run.Run{Status: tc.initial}
			allowed := r.CanTransitionTo(tc.next)
			if allowed != tc.expected {
				t.Errorf("expected CanTransitionTo(%s -> %s) = %v, got %v", tc.initial, tc.next, tc.expected, allowed)
			}
		})
	}
}
