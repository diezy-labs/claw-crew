package task

import (
	"context"
	"errors"
	"sync"
	"testing"
	"time"

	"github.com/diezy-labs/claw-crew/engine/src/run"
)

func TestDAGValidationAndSorting(t *testing.T) {
	scheduler := NewScheduler()

	// 1. Valid linear graph: A -> B -> C
	tasks := []*Task{
		{ID: "task_C", Dependencies: []string{"task_B"}},
		{ID: "task_A", Dependencies: []string{}},
		{ID: "task_B", Dependencies: []string{"task_A"}},
	}

	sorted, err := scheduler.ValidateDAG(tasks)
	if err != nil {
		t.Fatalf("expected valid DAG, got error: %v", err)
	}

	if len(sorted) != 3 {
		t.Fatalf("expected 3 tasks, got %d", len(sorted))
	}
	if sorted[0].ID != "task_A" || sorted[1].ID != "task_B" || sorted[2].ID != "task_C" {
		t.Fatalf("expected order [A, B, C], got [%s, %s, %s]", sorted[0].ID, sorted[1].ID, sorted[2].ID)
	}

	// 2. Cycle detection: A -> B -> A
	cyclicTasks := []*Task{
		{ID: "task_A", Dependencies: []string{"task_B"}},
		{ID: "task_B", Dependencies: []string{"task_A"}},
	}

	_, err = scheduler.ValidateDAG(cyclicTasks)
	if err == nil {
		t.Fatal("expected cycle error, got nil")
	}
}

func TestDAGConcurrentExecution(t *testing.T) {
	scheduler := NewScheduler()

	// Graph: A -> (B, C parallel) -> D
	tasks := []*Task{
		{ID: "task_A", Dependencies: []string{}},
		{ID: "task_B", Dependencies: []string{"task_A"}},
		{ID: "task_C", Dependencies: []string{"task_A"}},
		{ID: "task_D", Dependencies: []string{"task_B", "task_C"}},
	}

	var mu sync.Mutex
	executionOrder := make([]string, 0)

	exec := func(ctx context.Context, task *Task) error {
		time.Sleep(10 * time.Millisecond)
		mu.Lock()
		executionOrder = append(executionOrder, task.ID)
		mu.Unlock()
		return nil
	}

	ctx := context.Background()
	if err := scheduler.Execute(ctx, tasks, exec); err != nil {
		t.Fatalf("expected successful execution, got: %v", err)
	}

	if len(executionOrder) != 4 {
		t.Fatalf("expected 4 executed tasks, got %d", len(executionOrder))
	}
	// A must be first
	if executionOrder[0] != "task_A" {
		t.Fatalf("expected task_A to be first, got %s", executionOrder[0])
	}
	// D must be last
	if executionOrder[3] != "task_D" {
		t.Fatalf("expected task_D to be last, got %s", executionOrder[3])
	}
}

func TestTaskServiceLifecycle(t *testing.T) {
	store := NewMemoryTaskStore()
	scheduler := NewScheduler()
	runStore := run.NewMemoryStore()
	runHub := run.NewEventHub()
	runSvc := run.NewService(runStore, runHub)

	svc := NewService(store, scheduler, runSvc)
	ctx := context.Background()

	// Create task
	task, err := svc.CreateTask(ctx, &CreateTaskRequest{
		RunID: "run_test_01",
		Title: "Analyze requirements",
	})
	if err != nil {
		t.Fatalf("failed to create task: %v", err)
	}
	if task.Status != StatusReady {
		t.Fatalf("expected status ready for zero-dep task, got %s", task.Status)
	}

	// Execute task successfully
	execCount := 0
	err = svc.ExecuteRunTasks(ctx, "run_test_01", func(c context.Context, t *Task) error {
		execCount++
		return nil
	})
	if err != nil {
		t.Fatalf("unexpected error executing tasks: %v", err)
	}
	if execCount != 1 {
		t.Fatalf("expected 1 task executed, got %d", execCount)
	}

	// Verify task completed
	tUpdated, err := svc.GetTask(ctx, task.ID)
	if err != nil {
		t.Fatalf("failed to get task: %v", err)
	}
	if tUpdated.Status != StatusCompleted {
		t.Fatalf("expected status completed, got %s", tUpdated.Status)
	}

	// Retry task with failure
	_ = svc.RetryTask(ctx, task.ID, func(c context.Context, t *Task) error {
		return errors.New("simulated retry error")
	})
	time.Sleep(50 * time.Millisecond)

	tRetried, _ := svc.GetTask(ctx, task.ID)
	if tRetried.Status != StatusFailed {
		t.Fatalf("expected failed status after error, got %s", tRetried.Status)
	}
}
