package run

import "time"

// RunStatus defines the lifecycle status of an execution run
type RunStatus string

const (
	StatusQueued          RunStatus = "queued"
	StatusPlanning        RunStatus = "planning"
	StatusRunning         RunStatus = "running"
	StatusWaitingForInput RunStatus = "waiting_for_input"
	StatusCancelling      RunStatus = "cancelling"
	StatusCompleted       RunStatus = "completed"
	StatusFailed          RunStatus = "failed"
	StatusCancelled       RunStatus = "cancelled"
)

// CanTransitionTo enforces the monotonic state machine for Run lifecycle (BUG-014)
func (r *Run) CanTransitionTo(next RunStatus) bool {
	if r == nil {
		return false
	}
	if r.Status == next {
		return true // Idempotent same-state
	}
	switch r.Status {
	case StatusQueued:
		return next == StatusPlanning || next == StatusRunning || next == StatusFailed || next == StatusCancelling || next == StatusCancelled
	case StatusPlanning:
		return next == StatusRunning || next == StatusFailed || next == StatusCancelling || next == StatusCancelled
	case StatusRunning:
		return next == StatusWaitingForInput || next == StatusCompleted || next == StatusFailed || next == StatusCancelling || next == StatusCancelled
	case StatusWaitingForInput:
		return next == StatusRunning || next == StatusCompleted || next == StatusFailed || next == StatusCancelling || next == StatusCancelled
	case StatusCancelling:
		return next == StatusCancelled || next == StatusFailed
	case StatusCompleted, StatusFailed, StatusCancelled:
		return false // Terminal states are strictly immutable and cannot be overwritten
	default:
		return false
	}
}

// RunInput carries input arguments for a run
type RunInput struct {
	Prompt      string   `json:"prompt"`
	TargetFiles []string `json:"target_files,omitempty"`
}

// WorkspaceConfig specifies workspace boundaries
type WorkspaceConfig struct {
	RootURI string `json:"root_uri"`
}

// RunOptions configures runtime features for this run
type RunOptions struct {
	Stream              bool `json:"stream"`
	RequireToolApproval bool `json:"require_tool_approval"`
}

// TasksSummary provides counters for tasks in a run
type TasksSummary struct {
	Total     int `json:"total"`
	Completed int `json:"completed"`
	Running   int `json:"running"`
	Pending   int `json:"pending"`
}

// Canonical DTO: Run
type Run struct {
	ID           string          `json:"id"`
	CrewID       string          `json:"crew_id"`
	WorkflowID   string          `json:"workflow_id,omitempty"`
	Status       RunStatus       `json:"status"`
	Input        RunInput        `json:"input"`
	Workspace    WorkspaceConfig `json:"workspace"`
	Options      RunOptions      `json:"options"`
	TasksSummary TasksSummary    `json:"tasks_summary"`
	CreatedAt    time.Time       `json:"created_at"`
	StartedAt    *time.Time      `json:"started_at,omitempty"`
	CompletedAt  *time.Time      `json:"completed_at,omitempty"`
	ErrorMessage string          `json:"error_message,omitempty"`
}

// Canonical DTO: RunEvent
type RunEvent struct {
	EventID   string    `json:"event_id"`
	RunID     string    `json:"run_id"`
	Sequence  int64     `json:"sequence"`
	Type      string    `json:"type"`
	Timestamp time.Time `json:"timestamp"`
	Payload   any       `json:"payload,omitempty"`
	Error     string    `json:"error,omitempty"`
}

// Canonical DTO: Agent
type Agent struct {
	ID           string   `json:"id"`
	Name         string   `json:"name"`
	Role         string   `json:"role"`
	Status       string   `json:"status"` // idle, thinking, executing_tool, waiting_approval, completed, error
	Capabilities []string `json:"capabilities"`
}

// Canonical DTO: Task
type Task struct {
	ID           string     `json:"id"`
	RunID        string     `json:"run_id"`
	Title        string     `json:"title"`
	Description  string     `json:"description,omitempty"`
	AssignedTo   string     `json:"assigned_to,omitempty"`
	Status       string     `json:"status"` // pending, ready, assigned, running, waiting_for_input, completed, failed, cancelled
	Dependencies []string   `json:"dependencies,omitempty"`
	CreatedAt    time.Time  `json:"created_at"`
	CompletedAt  *time.Time `json:"completed_at,omitempty"`
}

// Canonical DTO: Artifact
type Artifact struct {
	ID        string    `json:"id"`
	RunID     string    `json:"run_id"`
	TaskID    string    `json:"task_id,omitempty"`
	Type      string    `json:"type"` // git_diff, file, report, diagram
	Path      string    `json:"path,omitempty"`
	Summary   string    `json:"summary"`
	MimeType  string    `json:"mime_type"`
	CreatedAt time.Time `json:"created_at"`
}

// CreateRunRequest payload for POST /api/v1/runs
type CreateRunRequest struct {
	CrewID     string          `json:"crew_id"`
	WorkflowID string          `json:"workflow_id,omitempty"`
	Input      RunInput        `json:"input"`
	Workspace  WorkspaceConfig `json:"workspace"`
	Options    RunOptions      `json:"options"`
}

// CreateRunResponse payload for 202 Accepted
type CreateRunResponse struct {
	ID        string    `json:"id"`
	CrewID    string    `json:"crew_id"`
	Status    RunStatus `json:"status"`
	EventsURL string    `json:"events_url"`
	CreatedAt time.Time `json:"created_at"`
}
