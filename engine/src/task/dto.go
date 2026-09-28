package task

import "time"

// TaskStatus defines the execution lifecycle state of a task
type TaskStatus string

const (
	StatusPending         TaskStatus = "pending"
	StatusReady           TaskStatus = "ready"
	StatusAssigned        TaskStatus = "assigned"
	StatusRunning         TaskStatus = "running"
	StatusWaitingForInput TaskStatus = "waiting_for_input"
	StatusRetrying        TaskStatus = "retrying"
	StatusCompleted       TaskStatus = "completed"
	StatusFailed          TaskStatus = "failed"
	StatusCancelled       TaskStatus = "cancelled"
)

// CanTransitionTo enforces the monotonic task state machine (BUG-014)
func (t *Task) CanTransitionTo(next TaskStatus) bool {
	if t == nil {
		return false
	}
	if t.Status == next {
		return true // Idempotent same-state
	}
	switch t.Status {
	case StatusPending:
		return next == StatusReady || next == StatusCancelled || next == StatusFailed
	case StatusReady:
		return next == StatusAssigned || next == StatusRunning || next == StatusCancelled || next == StatusFailed
	case StatusAssigned:
		return next == StatusRunning || next == StatusCancelled || next == StatusFailed
	case StatusRunning:
		return next == StatusWaitingForInput || next == StatusCompleted || next == StatusFailed || next == StatusCancelled
	case StatusWaitingForInput:
		return next == StatusRunning || next == StatusCompleted || next == StatusFailed || next == StatusCancelled
	case StatusFailed:
		return next == StatusRetrying || next == StatusReady || next == StatusCancelled
	case StatusRetrying:
		return next == StatusRunning || next == StatusFailed || next == StatusCancelled
	case StatusCompleted:
		return next == StatusRetrying || next == StatusReady
	case StatusCancelled:
		return false // Cancelled tasks are strictly terminal
	default:
		return false
	}
}

// Task represents a unit of work inside a DAG task graph
type Task struct {
	ID           string     `json:"id"`
	RunID        string     `json:"run_id"`
	Title        string     `json:"title"`
	Description  string     `json:"description,omitempty"`
	AssignedTo   string     `json:"assigned_to,omitempty"`
	Status       TaskStatus `json:"status"`
	Dependencies []string   `json:"dependencies,omitempty"`
	CreatedAt    time.Time  `json:"created_at"`
	StartedAt    *time.Time `json:"started_at,omitempty"`
	CompletedAt  *time.Time `json:"completed_at,omitempty"`
	ErrorMessage string     `json:"error_message,omitempty"`
}

// CreateTaskRequest DTO
type CreateTaskRequest struct {
	RunID        string   `json:"run_id"`
	Title        string   `json:"title"`
	Description  string   `json:"description,omitempty"`
	AssignedTo   string   `json:"assigned_to,omitempty"`
	Dependencies []string `json:"dependencies,omitempty"`
}

// TasksListResponse DTO
type TasksListResponse struct {
	RunID string  `json:"run_id"`
	Tasks []*Task `json:"tasks"`
}
