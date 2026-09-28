package task

import (
	"context"
	"fmt"
	"sync"
	"time"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/core/id"
	"github.com/diezy-labs/claw-crew/engine/src/run"
)

// MemoryTaskStore stores tasks in-memory
type MemoryTaskStore struct {
	mu         sync.RWMutex
	tasks      map[string]*Task
	tasksByRun map[string][]string
}

// NewMemoryTaskStore creates a new in-memory task store
func NewMemoryTaskStore() *MemoryTaskStore {
	return &MemoryTaskStore{
		tasks:      make(map[string]*Task),
		tasksByRun: make(map[string][]string),
	}
}

func (s *MemoryTaskStore) Save(ctx context.Context, t *Task) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.tasks[t.ID] = t
	s.tasksByRun[t.RunID] = append(s.tasksByRun[t.RunID], t.ID)
	return nil
}

func (s *MemoryTaskStore) Get(ctx context.Context, taskID string) (*Task, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	t, ok := s.tasks[taskID]
	if !ok {
		return nil, appErrors.New(appErrors.CodeNotFound, fmt.Sprintf("task not found: %s", taskID), appErrors.LayerService)
	}
	return t, nil
}

func (s *MemoryTaskStore) ListByRun(ctx context.Context, runID string) ([]*Task, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	ids := s.tasksByRun[runID]
	result := make([]*Task, 0, len(ids))
	for _, id := range ids {
		if t, ok := s.tasks[id]; ok {
			result = append(result, t)
		}
	}
	return result, nil
}

func (s *MemoryTaskStore) UpdateStatus(ctx context.Context, taskID string, status TaskStatus, errMsg string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	t, ok := s.tasks[taskID]
	if !ok {
		return appErrors.New(appErrors.CodeNotFound, fmt.Sprintf("task not found: %s", taskID), appErrors.LayerService)
	}

	// Validate monotonic state transitions (BUG-014)
	if !t.CanTransitionTo(status) {
		return appErrors.New(appErrors.CodeFailedPrecondition, fmt.Sprintf("invalid task state transition from %s to %s", t.Status, status), appErrors.LayerService)
	}

	t.Status = status
	if errMsg != "" {
		t.ErrorMessage = errMsg
	}
	now := time.Now().UTC()
	if status == StatusRunning && t.StartedAt == nil {
		t.StartedAt = &now
	}
	if status == StatusCompleted || status == StatusFailed || status == StatusCancelled {
		if t.CompletedAt == nil {
			t.CompletedAt = &now
		}
	}
	return nil
}

// DAGScheduler implements DAG topological sort and concurrent dependency execution
type DAGScheduler struct{}

// NewScheduler creates a DAG scheduler
func NewScheduler() *DAGScheduler {
	return &DAGScheduler{}
}

// ValidateDAG performs Kahn's algorithm for topological sorting and cycle detection
func (s *DAGScheduler) ValidateDAG(tasks []*Task) ([]*Task, error) {
	inDegree := make(map[string]int)
	graph := make(map[string][]string) // dependency -> list of dependents
	taskMap := make(map[string]*Task)

	for _, t := range tasks {
		taskMap[t.ID] = t
		inDegree[t.ID] = len(t.Dependencies)
	}

	for _, t := range tasks {
		for _, dep := range t.Dependencies {
			if _, exists := taskMap[dep]; !exists {
				return nil, appErrors.New(appErrors.CodeInvalidArgument, fmt.Sprintf("task %s has unknown dependency %s", t.ID, dep), appErrors.LayerService)
			}
			graph[dep] = append(graph[dep], t.ID)
		}
	}

	queue := make([]string, 0)
	for id, deg := range inDegree {
		if deg == 0 {
			queue = append(queue, id)
		}
	}

	sorted := make([]*Task, 0, len(tasks))
	for len(queue) > 0 {
		currID := queue[0]
		queue = queue[1:]
		sorted = append(sorted, taskMap[currID])

		for _, dependentID := range graph[currID] {
			inDegree[dependentID]--
			if inDegree[dependentID] == 0 {
				queue = append(queue, dependentID)
			}
		}
	}

	if len(sorted) != len(tasks) {
		return nil, appErrors.New(appErrors.CodeInvalidArgument, "cyclical dependency detected in task graph", appErrors.LayerService)
	}

	return sorted, nil
}

// Execute schedules tasks with fan-out for independent tasks and synchronization for dependents
func (s *DAGScheduler) Execute(ctx context.Context, tasks []*Task, exec ExecutorFunc) error {
	if len(tasks) == 0 {
		return nil
	}

	_, err := s.ValidateDAG(tasks)
	if err != nil {
		return err
	}

	completed := make(map[string]bool)
	var mu sync.Mutex
	cond := sync.NewCond(&mu)

	var execErr error
	var wg sync.WaitGroup

	taskMap := make(map[string]*Task)
	for _, t := range tasks {
		taskMap[t.ID] = t
	}

	for _, t := range tasks {
		wg.Add(1)
		go func(target *Task) {
			defer wg.Done()

			// Wait until all dependencies are completed or context cancelled
			mu.Lock()
			for {
				if ctx.Err() != nil {
					mu.Unlock()
					return
				}
				allDepsMet := true
				for _, dep := range target.Dependencies {
					if !completed[dep] {
						allDepsMet = false
						break
					}
				}
				if allDepsMet {
					break
				}
				cond.Wait()
			}
			mu.Unlock()

			select {
			case <-ctx.Done():
				return
			default:
			}

			err := exec(ctx, target)

			mu.Lock()
			if err != nil && execErr == nil {
				execErr = err
			}
			completed[target.ID] = (err == nil)
			cond.Broadcast()
			mu.Unlock()
		}(t)
	}

	wg.Wait()
	return execErr
}

// taskService coordinates tasks
type taskService struct {
	store      Store
	scheduler  Scheduler
	runService run.Service
}

// NewService creates a task service
func NewService(store Store, scheduler Scheduler, runService run.Service) Service {
	return &taskService{
		store:      store,
		scheduler:  scheduler,
		runService: runService,
	}
}

func (s *taskService) CreateTask(ctx context.Context, req *CreateTaskRequest) (*Task, error) {
	if req.RunID == "" {
		return nil, appErrors.New(appErrors.CodeInvalidArgument, "run_id is required", appErrors.LayerService)
	}
	if req.Title == "" {
		return nil, appErrors.New(appErrors.CodeInvalidArgument, "title is required", appErrors.LayerService)
	}

	taskID := id.NewTaskID()
	now := time.Now().UTC()

	initialStatus := StatusPending
	if len(req.Dependencies) == 0 {
		initialStatus = StatusReady
	}

	t := &Task{
		ID:           taskID,
		RunID:        req.RunID,
		Title:        req.Title,
		Description:  req.Description,
		AssignedTo:   req.AssignedTo,
		Status:       initialStatus,
		Dependencies: req.Dependencies,
		CreatedAt:    now,
	}

	if err := s.store.Save(ctx, t); err != nil {
		return nil, err
	}

	if s.runService != nil {
		s.runService.PublishEvent(req.RunID, "task.created", t, "")
	}

	return t, nil
}

func (s *taskService) GetTask(ctx context.Context, taskID string) (*Task, error) {
	return s.store.Get(ctx, taskID)
}

func (s *taskService) ListTasks(ctx context.Context, runID string) ([]*Task, error) {
	return s.store.ListByRun(ctx, runID)
}

func (s *taskService) ExecuteRunTasks(ctx context.Context, runID string, exec ExecutorFunc) error {
	tasks, err := s.store.ListByRun(ctx, runID)
	if err != nil {
		return err
	}

	execCtx := ctx
	if s.runService != nil {
		if runCtx, ok := s.runService.GetRunContext(runID); ok {
			var cancel context.CancelFunc
			execCtx, cancel = context.WithCancel(runCtx)
			defer cancel()
		}
	}

	wrappedExec := func(tCtx context.Context, t *Task) error {
		_ = s.store.UpdateStatus(tCtx, t.ID, StatusRunning, "")
		if s.runService != nil {
			s.runService.PublishEvent(t.RunID, "task.status_changed", map[string]any{"task_id": t.ID, "status": StatusRunning}, "")
		}

		err := exec(tCtx, t)
		if err != nil {
			_ = s.store.UpdateStatus(tCtx, t.ID, StatusFailed, err.Error())
			if s.runService != nil {
				s.runService.PublishEvent(t.RunID, "task.status_changed", map[string]any{"task_id": t.ID, "status": StatusFailed, "error": err.Error()}, err.Error())
			}
			return err
		}

		_ = s.store.UpdateStatus(tCtx, t.ID, StatusCompleted, "")
		if s.runService != nil {
			s.runService.PublishEvent(t.RunID, "task.status_changed", map[string]any{"task_id": t.ID, "status": StatusCompleted}, "")
		}
		return nil
	}

	return s.scheduler.Execute(execCtx, tasks, wrappedExec)
}

func (s *taskService) RetryTask(ctx context.Context, taskID string, exec ExecutorFunc) error {
	t, err := s.store.Get(ctx, taskID)
	if err != nil {
		return err
	}

	// Validate task can transition to StatusRetrying or StatusReady (BUG-015)
	if !t.CanTransitionTo(StatusRetrying) && !t.CanTransitionTo(StatusReady) {
		return appErrors.New(appErrors.CodeFailedPrecondition, fmt.Sprintf("task %s cannot be retried from status %s", taskID, t.Status), appErrors.LayerService)
	}

	_ = s.store.UpdateStatus(ctx, taskID, StatusRetrying, "")
	_ = s.store.UpdateStatus(ctx, taskID, StatusReady, "")
	if s.runService != nil {
		s.runService.PublishEvent(t.RunID, "task.status_changed", map[string]any{"task_id": t.ID, "status": StatusReady}, "")
	}

	// Inherit cancellation from root run context if active (BUG-002)
	retryCtx := ctx
	if s.runService != nil {
		if runCtx, ok := s.runService.GetRunContext(t.RunID); ok {
			retryCtx = runCtx
		}
	}

	go func() {
		_ = s.store.UpdateStatus(retryCtx, t.ID, StatusRunning, "")
		if s.runService != nil {
			s.runService.PublishEvent(t.RunID, "task.status_changed", map[string]any{"task_id": t.ID, "status": StatusRunning}, "")
		}

		if err := exec(retryCtx, t); err != nil {
			_ = s.store.UpdateStatus(retryCtx, t.ID, StatusFailed, err.Error())
			if s.runService != nil {
				s.runService.PublishEvent(t.RunID, "task.status_changed", map[string]any{"task_id": t.ID, "status": StatusFailed, "error": err.Error()}, err.Error())
			}
		} else {
			_ = s.store.UpdateStatus(retryCtx, t.ID, StatusCompleted, "")
			if s.runService != nil {
				s.runService.PublishEvent(t.RunID, "task.status_changed", map[string]any{"task_id": t.ID, "status": StatusCompleted}, "")
			}
		}
	}()

	return nil
}
