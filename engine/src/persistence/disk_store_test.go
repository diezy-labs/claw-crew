package persistence

import (
	"context"
	"os"
	"testing"
	"time"

	"github.com/diezy-labs/claw-crew/engine/src/run"
	"github.com/diezy-labs/claw-crew/engine/src/task"
)

func TestDiskStore_SaveGetAndResume(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "disk_store_test_*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	store, err := NewDiskStore(tempDir)
	if err != nil {
		t.Fatalf("NewDiskStore failed: %v", err)
	}

	ctx := context.Background()

	// 1. Save Run
	r := &run.Run{
		ID:        "run-durable-001",
		CrewID:    "crew-dev",
		Status:    run.StatusRunning,
		CreatedAt: time.Now().UTC(),
		Input: run.RunInput{
			Prompt: "Implement feature X",
		},
	}
	if err := store.SaveRun(ctx, r); err != nil {
		t.Fatalf("SaveRun failed: %v", err)
	}

	// 2. Read Run back
	loaded, err := store.GetRun(ctx, "run-durable-001")
	if err != nil {
		t.Fatalf("GetRun failed: %v", err)
	}
	if loaded.ID != r.ID || loaded.Status != run.StatusRunning {
		t.Errorf("unexpected loaded run: %+v", loaded)
	}

	// 3. Append events and replay
	ev1 := &run.RunEvent{
		EventID:   "ev-1",
		RunID:     "run-durable-001",
		Sequence:  1,
		Type:      "run.started",
		Timestamp: time.Now().UTC(),
	}
	ev2 := &run.RunEvent{
		EventID:   "ev-2",
		RunID:     "run-durable-001",
		Sequence:  2,
		Type:      "task.completed",
		Timestamp: time.Now().UTC(),
	}
	if err := store.AppendEvent(ctx, ev1); err != nil {
		t.Fatalf("AppendEvent 1 failed: %v", err)
	}
	if err := store.AppendEvent(ctx, ev2); err != nil {
		t.Fatalf("AppendEvent 2 failed: %v", err)
	}

	replayed, err := store.ReplayEvents(ctx, "run-durable-001")
	if err != nil {
		t.Fatalf("ReplayEvents failed: %v", err)
	}
	if len(replayed) != 2 {
		t.Fatalf("expected 2 replayed events, got %d", len(replayed))
	}
	if replayed[0].Type != "run.started" || replayed[1].Type != "task.completed" {
		t.Errorf("unexpected replayed events: %+v, %+v", replayed[0], replayed[1])
	}

	// 4. Save and load tasks
	tasks := []*task.Task{
		{
			ID:     "task-1",
			RunID:  "run-durable-001",
			Title:  "First step",
			Status: task.StatusCompleted,
		},
	}
	if err := store.SaveTasks(ctx, "run-durable-001", tasks); err != nil {
		t.Fatalf("SaveTasks failed: %v", err)
	}
	loadedTasks, err := store.LoadTasks(ctx, "run-durable-001")
	if err != nil {
		t.Fatalf("LoadTasks failed: %v", err)
	}
	if len(loadedTasks) != 1 || loadedTasks[0].ID != "task-1" {
		t.Errorf("unexpected loaded tasks: %+v", loadedTasks)
	}

	// 5. Test Resumable Runs after restart (TASK-7.2)
	recovered, err := store.ResumeInterruptedRuns(ctx)
	if err != nil {
		t.Fatalf("ResumeInterruptedRuns failed: %v", err)
	}
	if len(recovered) != 1 || recovered[0].ID != "run-durable-001" {
		t.Errorf("expected recovered run-durable-001, got: %v", recovered)
	}
}

// TestService_ResumeInterrupted verifies F1-3: a DiskStore-backed run.Service
// recovers interrupted runs through the ResumableStore capability.
func TestService_ResumeInterrupted(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "disk_resume_svc_*")
	if err != nil {
		t.Fatalf("temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	store, err := NewDiskStore(tempDir)
	if err != nil {
		t.Fatalf("NewDiskStore: %v", err)
	}
	ctx := context.Background()

	// Persist a run left mid-flight.
	if err := store.SaveRun(ctx, &run.Run{
		ID:        "run-svc-001",
		CrewID:    "crew-dev",
		Status:    run.StatusRunning,
		CreatedAt: time.Now().UTC(),
	}); err != nil {
		t.Fatalf("SaveRun: %v", err)
	}

	// A fresh service over the same disk store (simulates restart).
	svc := run.NewService(store, run.NewEventHub())
	recovered, err := svc.ResumeInterrupted(ctx)
	if err != nil {
		t.Fatalf("ResumeInterrupted: %v", err)
	}
	if len(recovered) != 1 || recovered[0].ID != "run-svc-001" {
		t.Fatalf("expected run-svc-001 resumed, got: %v", recovered)
	}
}
