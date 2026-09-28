package run

import (
	"context"
)

// Store defines persistent or in-memory access for Run entities
type Store interface {
	Save(ctx context.Context, r *Run) error
	Get(ctx context.Context, id string) (*Run, error)
	UpdateStatus(ctx context.Context, id string, status RunStatus, errMsg string) error
	List(ctx context.Context) ([]*Run, error)
}

// EventHub manages real-time SSE event subscriptions and fan-out
type EventHub interface {
	Subscribe(runID string) (<-chan *RunEvent, func())
	SubscribeSince(runID, lastEventID string) (<-chan *RunEvent, func())
	Publish(event *RunEvent)
	GetEvents(runID string) []*RunEvent
}

// Service coordinates run lifecycle operations and events
type Service interface {
	CreateRun(ctx context.Context, req *CreateRunRequest, reqID, idempotencyKey string) (*Run, error)
	GetRun(ctx context.Context, runID string) (*Run, error)
	CancelRun(ctx context.Context, runID string) error
	GetRunContext(runID string) (context.Context, bool)
	SubscribeEvents(runID string) (<-chan *RunEvent, func())
	SubscribeEventsSince(runID, lastEventID string) (<-chan *RunEvent, func())
	PublishEvent(runID string, eventType string, payload any, errStr string) *RunEvent
}
