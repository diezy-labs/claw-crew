package tool_test

import (
	"context"
	"testing"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/src/run"
	"github.com/diezy-labs/claw-crew/engine/src/tool"
)

func TestDirectDispatchDenial(t *testing.T) {
	runStore := run.NewMemoryStore()
	runHub := run.NewEventHub()
	runSvc := run.NewService(runStore, runHub)

	registry := tool.NewRegistry()
	gate := tool.NewApprovalGate(runSvc)
	toolSvc := tool.NewService(registry, gate, runSvc)

	ctx := context.Background()

	t.Run("RejectEmptyActorID", func(t *testing.T) {
		_, err := toolSvc.ExecuteWithContext(ctx, &tool.ExecutionContext{
			ActorID: "",
			RunID:   "run_anon",
		}, "read_file", `{"path":"test.txt"}`, ".", false)
		if err == nil {
			t.Fatalf("expected error for empty ActorID, got nil")
		}
		if appErr, ok := err.(*appErrors.AppError); ok {
			if appErr.Code != appErrors.CodePermissionDenied {
				t.Errorf("expected CodePermissionDenied, got %v", appErr.Code)
			}
		}
	})

	t.Run("RejectNilExecutionContext", func(t *testing.T) {
		_, err := toolSvc.ExecuteWithContext(ctx, nil, "read_file", `{"path":"test.txt"}`, ".", false)
		if err == nil {
			t.Fatalf("expected error for nil ExecutionContext, got nil")
		}
		if appErr, ok := err.(*appErrors.AppError); ok {
			if appErr.Code != appErrors.CodePermissionDenied {
				t.Errorf("expected CodePermissionDenied, got %v", appErr.Code)
			}
		}
	})

	t.Run("RejectCancelledRunDirectExecution", func(t *testing.T) {
		activeRun, err := runSvc.CreateRun(ctx, &run.CreateRunRequest{
			CrewID: "crew_sec",
			Input:  run.RunInput{Prompt: "Security test"},
		}, "req_sec", "")
		if err != nil {
			t.Fatalf("failed to create run: %v", err)
		}

		if err := runSvc.CancelRun(ctx, activeRun.ID); err != nil {
			t.Fatalf("failed to cancel run: %v", err)
		}

		_, err = toolSvc.ExecuteWithContext(ctx, &tool.ExecutionContext{
			ActorID: "agent_qa",
			RunID:   activeRun.ID,
		}, "read_file", `{"path":"test.txt"}`, ".", false)
		if err == nil {
			t.Fatalf("expected rejection on cancelled run, got nil")
		}
		if appErr, ok := err.(*appErrors.AppError); ok {
			if appErr.Code != appErrors.CodeFailedPrecondition {
				t.Errorf("expected CodeFailedPrecondition, got %v", appErr.Code)
			}
		}
	})
}
