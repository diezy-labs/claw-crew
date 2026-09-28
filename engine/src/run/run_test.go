package run

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/diezy-labs/claw-crew/engine/core/metrics"
)

func TestRunLifecycle(t *testing.T) {
	store := NewMemoryStore()
	hub := NewEventHub()
	svc := NewService(store, hub)

	ctx := context.Background()

	// 1. Create Run
	req := &CreateRunRequest{
		CrewID: "crew_dev",
		Input: RunInput{
			Prompt: "Test run creation",
		},
		Workspace: WorkspaceConfig{
			RootURI: "file:///workspace",
		},
	}

	run, err := svc.CreateRun(ctx, req, "req_1", "idem_1")
	if err != nil {
		t.Fatalf("failed to create run: %v", err)
	}

	if run.Status != StatusQueued {
		t.Fatalf("expected status queued, got %s", run.Status)
	}
	if !strings.HasPrefix(run.ID, "run_") {
		t.Fatalf("expected run_ prefix, got %s", run.ID)
	}

	// 2. Idempotency test
	runDup, err := svc.CreateRun(ctx, req, "req_2", "idem_1")
	if err != nil {
		t.Fatalf("failed to create run with existing key: %v", err)
	}
	if runDup.ID != run.ID {
		t.Fatalf("expected idempotent return of same run, got %s vs %s", runDup.ID, run.ID)
	}

	// 3. Event subscription test
	eventCh, unsubscribe := svc.SubscribeEvents(run.ID)
	defer unsubscribe()

	// 4. Publish custom event
	pubEvt := svc.PublishEvent(run.ID, "agent.status_changed", map[string]string{"status": "thinking"}, "")
	if pubEvt.Sequence != 2 { // 1 was run.created, 2 is agent.status_changed
		t.Fatalf("expected sequence 2, got %d", pubEvt.Sequence)
	}

	select {
	case evt := <-eventCh:
		// Could be run.created or agent.status_changed
		if evt.RunID != run.ID {
			t.Fatalf("expected run ID %s, got %s", run.ID, evt.RunID)
		}
	case <-time.After(1 * time.Second):
		t.Fatal("timed out waiting for event")
	}

	// 5. Cancel Run
	if err := svc.CancelRun(ctx, run.ID); err != nil {
		t.Fatalf("failed to cancel run: %v", err)
	}

	updatedRun, err := svc.GetRun(ctx, run.ID)
	if err != nil {
		t.Fatalf("failed to get run: %v", err)
	}
	if updatedRun.Status != StatusCancelled {
		t.Fatalf("expected status cancelled, got %s", updatedRun.Status)
	}
}

func TestRunHTTPDelivery(t *testing.T) {
	store := NewMemoryStore()
	hub := NewEventHub()
	svc := NewService(store, hub)
	handler := NewHTTPHandler(svc)

	server := metrics.NewServer(9999)
	handler.RegisterHTTP(server)

	// Test POST /api/v1/runs
	body := `{"crew_id":"crew_test","input":{"prompt":"Hello"}}`
	req := httptest.NewRequest(http.MethodPost, "/api/v1/runs", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()

	server.RegisterRouteFunc("/test/runs", handler.handleRunsRoot)
	handler.handleRunsRoot(w, req)

	if w.Code != http.StatusAccepted {
		t.Fatalf("expected 202 Accepted, got %d: %s", w.Code, w.Body.String())
	}

	if !strings.Contains(w.Body.String(), `"status":"queued"`) {
		t.Fatalf("expected queued status in body: %s", w.Body.String())
	}
}
