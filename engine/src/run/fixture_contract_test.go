package run_test

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/src/run"
)

func TestFixtureContracts(t *testing.T) {
	fixturesDir := filepath.Join("..", "..", "..", "fixtures")

	t.Run("RunCreatedFixture", func(t *testing.T) {
		data, err := os.ReadFile(filepath.Join(fixturesDir, "run-created.json"))
		if err != nil {
			t.Fatalf("failed reading fixture: %v", err)
		}
		var r run.Run
		if err := json.Unmarshal(data, &r); err != nil {
			t.Fatalf("failed unmarshaling run-created: %v", err)
		}
		if r.Status != run.StatusQueued {
			t.Errorf("expected status %s, got %s", run.StatusQueued, r.Status)
		}
	})

	t.Run("RunRunningFixture", func(t *testing.T) {
		data, err := os.ReadFile(filepath.Join(fixturesDir, "run-running.json"))
		if err != nil {
			t.Fatalf("failed reading fixture: %v", err)
		}
		var r run.Run
		if err := json.Unmarshal(data, &r); err != nil {
			t.Fatalf("failed unmarshaling run-running: %v", err)
		}
		if r.Status != run.StatusRunning {
			t.Errorf("expected status %s, got %s", run.StatusRunning, r.Status)
		}
	})

	t.Run("RunCancelledFixture", func(t *testing.T) {
		data, err := os.ReadFile(filepath.Join(fixturesDir, "run-cancelled.json"))
		if err != nil {
			t.Fatalf("failed reading fixture: %v", err)
		}
		var r run.Run
		if err := json.Unmarshal(data, &r); err != nil {
			t.Fatalf("failed unmarshaling run-cancelled: %v", err)
		}
		if r.Status != run.StatusCancelled {
			t.Errorf("expected status %s, got %s", run.StatusCancelled, r.Status)
		}
	})

	t.Run("StructuredErrorFixture", func(t *testing.T) {
		data, err := os.ReadFile(filepath.Join(fixturesDir, "structured-error.json"))
		if err != nil {
			t.Fatalf("failed reading fixture: %v", err)
		}
		var env appErrors.ErrorEnvelope
		if err := json.Unmarshal(data, &env); err != nil {
			t.Fatalf("failed unmarshaling structured error: %v", err)
		}
		if env.Error.Code != appErrors.CodeFailedPrecondition {
			t.Errorf("expected code %s, got %s", appErrors.CodeFailedPrecondition, env.Error.Code)
		}
	})

	t.Run("EventStreamReplayFixture", func(t *testing.T) {
		data, err := os.ReadFile(filepath.Join(fixturesDir, "event-stream-replay.json"))
		if err != nil {
			t.Fatalf("failed reading fixture: %v", err)
		}
		var events []*run.RunEvent
		if err := json.Unmarshal(data, &events); err != nil {
			t.Fatalf("failed unmarshaling event stream replay: %v", err)
		}
		if len(events) != 3 {
			t.Fatalf("expected 3 events, got %d", len(events))
		}
		if events[1].Sequence != 2 {
			t.Errorf("expected sequence 2, got %d", events[1].Sequence)
		}
	})
}
