package id

import (
	"crypto/rand"
	"encoding/hex"
	"fmt"
	"time"
)

// Standard ID prefixes per Phase 2 API specification
const (
	PrefixRequest     = "req_"
	PrefixRun         = "run_"
	PrefixTask        = "task_"
	PrefixEvent       = "evt_"
	PrefixCorrelation = "corr_"
	PrefixArtifact    = "art_"
	PrefixWorkflow    = "wf_"
)

// Generate generates a time-sortable random identifier with the specified prefix
func Generate(prefix string) string {
	now := time.Now().UTC().UnixMilli()
	var randomBytes [8]byte
	_, _ = rand.Read(randomBytes[:])
	return fmt.Sprintf("%s%012x%s", prefix, now, hex.EncodeToString(randomBytes[:]))
}

// NewExecutionID generates a standardized execution_id (e.g. exec_...)
func NewExecutionID() string {
	return Generate("exec_")
}

// NewRequestID generates a standardized request_id (e.g. req_...)
func NewRequestID() string {
	return Generate(PrefixRequest)
}

// NewRunID generates a standardized run_id (e.g. run_...)
func NewRunID() string {
	return Generate(PrefixRun)
}

// NewTaskID generates a standardized task_id (e.g. task_...)
func NewTaskID() string {
	return Generate(PrefixTask)
}

// NewEventID generates a standardized event_id (e.g. evt_...)
func NewEventID() string {
	return Generate(PrefixEvent)
}

// NewCorrelationID generates a standardized correlation_id (e.g. corr_...)
func NewCorrelationID() string {
	return Generate(PrefixCorrelation)
}

// NewArtifactID generates a standardized artifact_id (e.g. art_...)
func NewArtifactID() string {
	return Generate(PrefixArtifact)
}

// NewWorkflowID generates a standardized workflow_id (e.g. wf_...)
func NewWorkflowID() string {
	return Generate(PrefixWorkflow)
}
