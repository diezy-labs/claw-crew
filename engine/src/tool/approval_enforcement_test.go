package tool_test

import (
	"context"
	"strings"
	"testing"
	"time"

	"github.com/diezy-labs/claw-crew/engine/src/run"
	"github.com/diezy-labs/claw-crew/engine/src/tool"
)

func TestApprovalEnforcementAndIdempotency(t *testing.T) {
	runStore := run.NewMemoryStore()
	runHub := run.NewEventHub()
	runSvc := run.NewService(runStore, runHub)

	registry := tool.NewRegistry()
	gate := tool.NewApprovalGate(runSvc)
	toolSvc := tool.NewService(registry, gate, runSvc)

	ctx := context.Background()

	// 1. Missing ActorID -> must be denied by default (BUG-005)
	_, err := toolSvc.ExecuteWithContext(ctx, &tool.ExecutionContext{
		ActorID: "",
		RunID:   "run_no_actor",
	}, "read_file", `{"path":"test.txt"}`, ".", false)
	if err == nil {
		t.Fatalf("expected error executing tool without ActorID, got nil")
	}

	// 2. Denied tool on terminated / cancelled Run (ACT-P0-07)
	activeRun, err := runSvc.CreateRun(ctx, &run.CreateRunRequest{
		CrewID: "test_crew",
		Input:  run.RunInput{Prompt: "Cancel test"},
	}, "req_1", "")
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
		t.Fatalf("expected tool execution to be rejected on cancelled run, got nil")
	}

	// 3. Approval gate idempotency: second resolution attempt on same execution must fail
	gateExecCtx := &tool.ExecutionContext{
		ActorID: "agent_qa",
		RunID:   "run_idempotent_test",
	}

	go func() {
		time.Sleep(10 * time.Millisecond)
		execs := gate.ListExecutions("run_idempotent_test")
		if len(execs) > 0 {
			_ = gate.Resolve(execs[0].ID, true, "approved once")
			// Second resolution attempt must fail (idempotent / non-reentrant)
			err2 := gate.Resolve(execs[0].ID, true, "approved twice")
			if err2 == nil {
				t.Errorf("expected error on duplicate approval resolution, got nil")
			}
		}
	}()

	approved, _, err := gate.RequestApprovalWithContext(ctx, gateExecCtx, "write_file", `{"path":"out.txt"}`, tool.RiskTierWrite)
	if err != nil {
		t.Fatalf("unexpected error requesting approval: %v", err)
	}
	if !approved {
		t.Fatalf("expected approved=true")
	}

	// 4. Approval timeout: context cancellation marks status as timed_out
	timeoutCtx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()

	timedOutExecCtx := &tool.ExecutionContext{
		ActorID: "agent_qa",
		RunID:   "run_timeout_test",
	}

	_, _, err = gate.RequestApprovalWithContext(timeoutCtx, timedOutExecCtx, "write_file", `{"path":"timeout.txt"}`, tool.RiskTierWrite)
	if err == nil {
		t.Fatalf("expected error on timed-out approval, got nil")
	}

	execs := gate.ListExecutions("run_timeout_test")
	if len(execs) == 0 {
		t.Fatalf("expected recorded execution for timeout test")
	}
	if execs[0].ApprovalStatus != tool.ApprovalTimedOut {
		t.Fatalf("expected status %s, got %s", tool.ApprovalTimedOut, execs[0].ApprovalStatus)
	}

	// 5. Secret redaction in arguments
	secretArgs := `{"api_key": "sk-12345678901234567890", "command": "curl -H 'Authorization: Bearer secret-tok-12345678901234567890'"}`
	_, _, _ = gate.RequestApprovalWithContext(timeoutCtx, timedOutExecCtx, "execute_command", secretArgs, tool.RiskTierExecute)
	allExecs := gate.ListExecutions("run_timeout_test")
	for _, ex := range allExecs {
		if strings.Contains(ex.Arguments, "sk-12345678901234567890") {
			t.Errorf("secret API key leaked in stored arguments: %s", ex.Arguments)
		}
		if strings.Contains(ex.Arguments, "secret-tok-12345678901234567890") {
			t.Errorf("secret Bearer token leaked in stored arguments: %s", ex.Arguments)
		}
	}
}
