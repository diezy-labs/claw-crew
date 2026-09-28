package id

import (
	"strings"
	"testing"
)

func TestIDGenerators(t *testing.T) {
	tests := []struct {
		name     string
		gen      func() string
		expected string
	}{
		{"RequestID", NewRequestID, PrefixRequest},
		{"RunID", NewRunID, PrefixRun},
		{"TaskID", NewTaskID, PrefixTask},
		{"EventID", NewEventID, PrefixEvent},
		{"CorrelationID", NewCorrelationID, PrefixCorrelation},
		{"ArtifactID", NewArtifactID, PrefixArtifact},
		{"WorkflowID", NewWorkflowID, PrefixWorkflow},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			id1 := tt.gen()
			id2 := tt.gen()

			if !strings.HasPrefix(id1, tt.expected) {
				t.Fatalf("expected prefix %s, got %s", tt.expected, id1)
			}
			if id1 == id2 {
				t.Fatalf("expected unique IDs, got duplicate: %s", id1)
			}
		})
	}
}
