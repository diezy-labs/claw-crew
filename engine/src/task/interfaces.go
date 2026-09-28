package task

import (
	"context"
)

// Store defines persistence for tasks
type Store interface {
	Save(ctx context.Context, t *Task) error
	Get(ctx context.Context, taskID string) (*Task, error)
	ListByRun(ctx context.Context, runID string) ([]*Task, error)
	UpdateStatus(ctx context.Context, taskID string, status TaskStatus, errMsg string) error
}

// ExecutorFunc defines the execution callback for a single task
type ExecutorFunc func(ctx context.Context, t *Task) error

// Scheduler handles DAG dependency sorting and sequential/parallel execution
type Scheduler interface {
	ValidateDAG(tasks []*Task) ([]*Task, error)
	Execute(ctx context.Context, tasks []*Task, exec ExecutorFunc) error
}

// Service provides high-level task coordination
type Service interface {
	CreateTask(ctx context.Context, req *CreateTaskRequest) (*Task, error)
	GetTask(ctx context.Context, taskID string) (*Task, error)
	ListTasks(ctx context.Context, runID string) ([]*Task, error)
	ExecuteRunTasks(ctx context.Context, runID string, exec ExecutorFunc) error
	RetryTask(ctx context.Context, taskID string, exec ExecutorFunc) error
}
