package workflow

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/diezy-labs/claw-crew/engine/src/run"
	"github.com/diezy-labs/claw-crew/engine/src/task"
)

func setupTestWorkflow(t *testing.T) (Service, task.Service, *HTTPHandler) {
	t.Helper()
	runStore := run.NewMemoryStore()
	runHub := run.NewEventHub()
	runSvc := run.NewService(runStore, runHub)

	taskStore := task.NewMemoryTaskStore()
	taskScheduler := task.NewScheduler()
	taskSvc := task.NewService(taskStore, taskScheduler, runSvc)

	reg := NewRegistry()
	wfSvc := NewService(reg, taskSvc)
	handler := NewHTTPHandler(wfSvc)

	return wfSvc, taskSvc, handler
}

func TestWorkflow_ListAndGetDefaults(t *testing.T) {
	wfSvc, _, _ := setupTestWorkflow(t)
	ctx := context.Background()

	list, err := wfSvc.ListWorkflows(ctx)
	if err != nil {
		t.Fatalf("ListWorkflows failed: %v", err)
	}
	if len(list) < 4 {
		t.Errorf("expected at least 4 default workflows, got %d", len(list))
	}

	sopDev, err := wfSvc.GetWorkflow(ctx, "sop_feature_dev")
	if err != nil {
		t.Fatalf("GetWorkflow(sop_feature_dev) failed: %v", err)
	}
	if sopDev.Name != "Feature Development SOP" {
		t.Errorf("expected Feature Development SOP, got %s", sopDev.Name)
	}
	if len(sopDev.Steps) != 4 {
		t.Errorf("expected 4 steps in feature dev SOP, got %d", len(sopDev.Steps))
	}
}

func TestWorkflow_InstantiateWorkflow(t *testing.T) {
	wfSvc, taskSvc, _ := setupTestWorkflow(t)
	ctx := context.Background()

	// Instantiate sop_feature_dev for run-123
	resp, err := wfSvc.InstantiateWorkflow(ctx, "sop_feature_dev", &InstantiateWorkflowRequest{
		RunID: "run-123",
	})
	if err != nil {
		t.Fatalf("InstantiateWorkflow failed: %v", err)
	}

	if resp.RunID != "run-123" {
		t.Errorf("expected run-123, got %s", resp.RunID)
	}
	if len(resp.TaskIDs) != 4 {
		t.Fatalf("expected 4 task IDs, got %d", len(resp.TaskIDs))
	}

	// Verify all tasks exist in task.Service
	tasks, err := taskSvc.ListTasks(ctx, "run-123")
	if err != nil {
		t.Fatalf("ListTasks failed: %v", err)
	}
	if len(tasks) != 4 {
		t.Fatalf("expected 4 tasks in run-123, got %d", len(tasks))
	}

	// Verify the DAG dependencies link correctly
	// Step 0: requirements (no deps, should be ready)
	t0, err := taskSvc.GetTask(ctx, resp.TaskIDs[0])
	if err != nil || t0 == nil {
		t.Fatalf("failed to get task 0: %v", err)
	}
	if len(t0.Dependencies) != 0 {
		t.Errorf("expected step 0 to have 0 deps, got %d", len(t0.Dependencies))
	}
	if t0.Status != task.StatusReady {
		t.Errorf("expected step 0 to be ready, got %s", t0.Status)
	}

	// Step 1: implementation (depends on step 0, should be pending)
	t1, err := taskSvc.GetTask(ctx, resp.TaskIDs[1])
	if err != nil || t1 == nil {
		t.Fatalf("failed to get task 1: %v", err)
	}
	if len(t1.Dependencies) != 1 || t1.Dependencies[0] != t0.ID {
		t.Errorf("expected step 1 to depend on task 0 (%s), got %v", t0.ID, t1.Dependencies)
	}
	if t1.Status != task.StatusPending {
		t.Errorf("expected step 1 to be pending, got %s", t1.Status)
	}
}

func TestWorkflow_HTTPHandler(t *testing.T) {
	_, _, handler := setupTestWorkflow(t)

	// 1. GET /api/v1/workflows
	req := httptest.NewRequest(http.MethodGet, "/api/v1/workflows", nil)
	rr := httptest.NewRecorder()
	handler.handleWorkflows(rr, req)

	if rr.Code != http.StatusOK {
		t.Fatalf("GET /api/v1/workflows returned %d: %s", rr.Code, rr.Body.String())
	}
	var workflows []*WorkflowTemplate
	if err := json.Unmarshal(rr.Body.Bytes(), &workflows); err != nil {
		t.Fatalf("failed to unmarshal workflows: %v", err)
	}
	if len(workflows) < 4 {
		t.Errorf("expected at least 4 workflows, got %d", len(workflows))
	}

	// 2. GET /api/v1/workflows/sop_bug_fix
	req = httptest.NewRequest(http.MethodGet, "/api/v1/workflows/sop_bug_fix", nil)
	rr = httptest.NewRecorder()
	handler.handleWorkflowByID(rr, req)

	if rr.Code != http.StatusOK {
		t.Fatalf("GET /api/v1/workflows/sop_bug_fix returned %d: %s", rr.Code, rr.Body.String())
	}

	// 3. POST /api/v1/workflows/sop_bug_fix/instantiate
	body, _ := json.Marshal(InstantiateWorkflowRequest{RunID: "run-sop-http"})
	req = httptest.NewRequest(http.MethodPost, "/api/v1/workflows/sop_bug_fix/instantiate", bytes.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	rr = httptest.NewRecorder()
	handler.handleWorkflowByID(rr, req)

	if rr.Code != http.StatusCreated {
		t.Fatalf("POST /api/v1/workflows/sop_bug_fix/instantiate returned %d: %s", rr.Code, rr.Body.String())
	}
	var instResp InstantiateWorkflowResponse
	if err := json.Unmarshal(rr.Body.Bytes(), &instResp); err != nil {
		t.Fatalf("failed to unmarshal response: %v", err)
	}
	if instResp.RunID != "run-sop-http" || len(instResp.TaskIDs) != 3 {
		t.Errorf("unexpected instantiate response: %+v", instResp)
	}
}
