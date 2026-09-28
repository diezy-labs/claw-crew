package run

import (
	"context"
	"fmt"
	"sync"
	"sync/atomic"
	"time"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/core/id"
)

// MemoryStore implements thread-safe in-memory storage for runs
type MemoryStore struct {
	mu   sync.RWMutex
	runs map[string]*Run
}

// NewMemoryStore creates a new in-memory store
func NewMemoryStore() *MemoryStore {
	return &MemoryStore{
		runs: make(map[string]*Run),
	}
}

func (s *MemoryStore) Save(ctx context.Context, r *Run) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.runs[r.ID] = r
	return nil
}

func (s *MemoryStore) Get(ctx context.Context, runID string) (*Run, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	r, ok := s.runs[runID]
	if !ok {
		return nil, appErrors.New(appErrors.CodeNotFound, fmt.Sprintf("run not found: %s", runID), appErrors.LayerService)
	}
	cp := *r
	return &cp, nil
}

func (s *MemoryStore) UpdateStatus(ctx context.Context, runID string, status RunStatus, errMsg string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	r, ok := s.runs[runID]
	if !ok {
		return appErrors.New(appErrors.CodeNotFound, fmt.Sprintf("run not found: %s", runID), appErrors.LayerService)
	}

	// Validate monotonic state transitions (BUG-014)
	if !r.CanTransitionTo(status) {
		return appErrors.New(appErrors.CodeFailedPrecondition, fmt.Sprintf("invalid run state transition from %s to %s", r.Status, status), appErrors.LayerService)
	}

	r.Status = status
	if errMsg != "" {
		r.ErrorMessage = errMsg
	}
	now := time.Now().UTC()
	if status == StatusRunning && r.StartedAt == nil {
		r.StartedAt = &now
	}
	if status == StatusCompleted || status == StatusFailed || status == StatusCancelled {
		if r.CompletedAt == nil {
			r.CompletedAt = &now
		}
	}
	return nil
}

func (s *MemoryStore) List(ctx context.Context) ([]*Run, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	list := make([]*Run, 0, len(s.runs))
	for _, r := range s.runs {
		list = append(list, r)
	}
	return list, nil
}

// MemoryEventHub implements pub-sub for SSE events with history buffering
type MemoryEventHub struct {
	mu          sync.RWMutex
	subscribers map[string][]chan *RunEvent
	history     map[string][]*RunEvent
	sequences   map[string]*atomic.Int64
}

// NewEventHub creates a new event hub instance
func NewEventHub() *MemoryEventHub {
	return &MemoryEventHub{
		subscribers: make(map[string][]chan *RunEvent),
		history:     make(map[string][]*RunEvent),
		sequences:   make(map[string]*atomic.Int64),
	}
}

func (h *MemoryEventHub) nextSequence(runID string) int64 {
	h.mu.Lock()
	seq, ok := h.sequences[runID]
	if !ok {
		seq = &atomic.Int64{}
		h.sequences[runID] = seq
	}
	h.mu.Unlock()
	return seq.Add(1)
}

func (h *MemoryEventHub) Subscribe(runID string) (<-chan *RunEvent, func()) {
	return h.SubscribeSince(runID, "")
}

func (h *MemoryEventHub) SubscribeSince(runID, lastEventID string) (<-chan *RunEvent, func()) {
	ch := make(chan *RunEvent, 256)

	h.mu.Lock()
	h.subscribers[runID] = append(h.subscribers[runID], ch)
	// Replay history to subscriber strictly starting after lastEventID (BUG-006 deduplication)
	hist := h.history[runID]
	skip := lastEventID != ""
	for _, evt := range hist {
		if skip {
			if evt.EventID == lastEventID {
				skip = false
			}
			continue
		}
		select {
		case ch <- evt:
		default:
		}
	}
	h.mu.Unlock()

	unsubscribe := func() {
		h.mu.Lock()
		defer h.mu.Unlock()
		subs := h.subscribers[runID]
		for i, sub := range subs {
			if sub == ch {
				h.subscribers[runID] = append(subs[:i], subs[i+1:]...)
				close(ch)
				break
			}
		}
	}

	return ch, unsubscribe
}

func (h *MemoryEventHub) Publish(event *RunEvent) {
	h.mu.Lock()
	defer h.mu.Unlock()

	h.history[event.RunID] = append(h.history[event.RunID], event)
	for _, ch := range h.subscribers[event.RunID] {
		select {
		case ch <- event:
		default:
		}
	}
}

func (h *MemoryEventHub) GetEvents(runID string) []*RunEvent {
	h.mu.RLock()
	defer h.mu.RUnlock()
	return h.history[runID]
}

// runService implements Service
type runService struct {
	store       Store
	eventHub    EventHub
	cancels     sync.Map
	contexts    sync.Map
	idempotency sync.Map
}

// NewService creates a new Run service
func NewService(store Store, eventHub EventHub) Service {
	return &runService{
		store:    store,
		eventHub: eventHub,
	}
}

func (s *runService) GetRunContext(runID string) (context.Context, bool) {
	val, ok := s.contexts.Load(runID)
	if !ok {
		return nil, false
	}
	ctx, ok := val.(context.Context)
	return ctx, ok
}

func (s *runService) CreateRun(ctx context.Context, req *CreateRunRequest, reqID, idempotencyKey string) (*Run, error) {
	if req.CrewID == "" {
		return nil, appErrors.New(appErrors.CodeInvalidArgument, "crew_id is required", appErrors.LayerService)
	}

	// Idempotency check
	if idempotencyKey != "" {
		if val, ok := s.idempotency.Load(idempotencyKey); ok {
			if existingRun, ok := val.(*Run); ok {
				return existingRun, nil
			}
		}
	}

	runID := id.NewRunID()
	now := time.Now().UTC()

	r := &Run{
		ID:         runID,
		CrewID:     req.CrewID,
		WorkflowID: req.WorkflowID,
		Status:     StatusQueued,
		Input:      req.Input,
		Workspace:  req.Workspace,
		Options:    req.Options,
		TasksSummary: TasksSummary{
			Total:     0,
			Completed: 0,
			Running:   0,
			Pending:   0,
		},
		CreatedAt: now,
	}

	if err := s.store.Save(ctx, r); err != nil {
		return nil, err
	}

	runCtx, cancel := context.WithCancel(context.Background())
	s.cancels.Store(runID, cancel)
	s.contexts.Store(runID, runCtx)

	if idempotencyKey != "" {
		s.idempotency.Store(idempotencyKey, r)
	}

	s.PublishEvent(runID, "run.created", map[string]any{
		"run_id":     runID,
		"crew_id":    req.CrewID,
		"status":     StatusQueued,
		"created_at": now,
	}, "")

	return r, nil
}

func (s *runService) GetRun(ctx context.Context, runID string) (*Run, error) {
	return s.store.Get(ctx, runID)
}

func (s *runService) CancelRun(ctx context.Context, runID string) error {
	// 1. Monotonically transition to StatusCancelling
	if err := s.store.UpdateStatus(ctx, runID, StatusCancelling, "cancellation requested"); err != nil {
		if appErr, ok := err.(*appErrors.AppError); ok && appErr.Code == appErrors.CodeFailedPrecondition {
			// Run is already in terminal state or cancelling; safe no-op
			return nil
		}
		return err
	}

	s.PublishEvent(runID, "run.cancelling", map[string]any{
		"run_id": runID,
		"status": StatusCancelling,
	}, "")

	// 2. Trigger context cancellation to stop all active tasks, tool execution, and LLM calls
	if cancelVal, ok := s.cancels.Load(runID); ok {
		if cancelFn, ok := cancelVal.(context.CancelFunc); ok {
			cancelFn()
		}
	}

	// 3. Monotonically transition to StatusCancelled
	if err := s.store.UpdateStatus(ctx, runID, StatusCancelled, "cancelled by user request"); err != nil {
		return err
	}

	s.PublishEvent(runID, "run.cancelled", map[string]any{
		"run_id": runID,
		"status": StatusCancelled,
	}, "")

	return nil
}

func (s *runService) SubscribeEvents(runID string) (<-chan *RunEvent, func()) {
	return s.eventHub.Subscribe(runID)
}

func (s *runService) SubscribeEventsSince(runID, lastEventID string) (<-chan *RunEvent, func()) {
	return s.eventHub.SubscribeSince(runID, lastEventID)
}

func (s *runService) PublishEvent(runID string, eventType string, payload any, errStr string) *RunEvent {
	seq := int64(1)
	if hub, ok := s.eventHub.(*MemoryEventHub); ok {
		seq = hub.nextSequence(runID)
	}

	evt := &RunEvent{
		EventID:   id.NewEventID(),
		RunID:     runID,
		Sequence:  seq,
		Type:      eventType,
		Timestamp: time.Now().UTC(),
		Payload:   payload,
		Error:     errStr,
	}

	s.eventHub.Publish(evt)
	return evt
}
