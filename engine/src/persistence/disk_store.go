package persistence

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sync"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/src/run"
	"github.com/diezy-labs/claw-crew/engine/src/task"
)

// DiskStore provides persistent JSON and JSONL storage on disk for runs, tasks, and event replay
type DiskStore struct {
	baseDir string
	mu      sync.RWMutex
}

// NewDiskStore constructs a DiskStore in the specified base directory
func NewDiskStore(baseDir string) (*DiskStore, error) {
	if err := os.MkdirAll(baseDir, 0755); err != nil {
		return nil, fmt.Errorf("failed to initialize disk store directory %s: %w", baseDir, err)
	}
	return &DiskStore{baseDir: baseDir}, nil
}

func (s *DiskStore) runDir(runID string) string {
	return filepath.Join(s.baseDir, "runs", runID)
}

func (s *DiskStore) runFile(runID string) string {
	return filepath.Join(s.runDir(runID), "run.json")
}

func (s *DiskStore) tasksFile(runID string) string {
	return filepath.Join(s.runDir(runID), "tasks.json")
}

func (s *DiskStore) eventsFile(runID string) string {
	return filepath.Join(s.runDir(runID), "events.jsonl")
}

// SaveRun persists a Run entity to disk
func (s *DiskStore) SaveRun(ctx context.Context, r *run.Run) error {
	if r == nil || r.ID == "" {
		return appErrors.New(appErrors.CodeInvalidArgument, "run or run ID cannot be empty", appErrors.LayerRepository)
	}

	select {
	case <-ctx.Done():
		return ctx.Err()
	default:
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	dir := s.runDir(r.ID)
	if err := os.MkdirAll(dir, 0755); err != nil {
		return appErrors.Wrap(err, appErrors.CodeInternal, "failed to create run directory", appErrors.LayerRepository)
	}

	data, err := json.MarshalIndent(r, "", "  ")
	if err != nil {
		return appErrors.Wrap(err, appErrors.CodeInternal, "failed to marshal run", appErrors.LayerRepository)
	}

	if err := os.WriteFile(s.runFile(r.ID), data, 0644); err != nil {
		return appErrors.Wrap(err, appErrors.CodeInternal, "failed to write run file", appErrors.LayerRepository)
	}

	return nil
}

// GetRun reads a Run entity from disk
func (s *DiskStore) GetRun(ctx context.Context, id string) (*run.Run, error) {
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	default:
	}

	s.mu.RLock()
	defer s.mu.RUnlock()

	filePath := s.runFile(id)
	data, err := os.ReadFile(filePath)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, appErrors.New(appErrors.CodeNotFound, fmt.Sprintf("run not found on disk: %s", id), appErrors.LayerRepository)
		}
		return nil, appErrors.Wrap(err, appErrors.CodeInternal, "failed to read run file", appErrors.LayerRepository)
	}

	var r run.Run
	if err := json.Unmarshal(data, &r); err != nil {
		return nil, appErrors.Wrap(err, appErrors.CodeInternal, "failed to unmarshal run", appErrors.LayerRepository)
	}

	return &r, nil
}

// AppendEvent writes an event to the append-only JSONL log for replay (TASK-7.3)
func (s *DiskStore) AppendEvent(ctx context.Context, event *run.RunEvent) error {
	if event == nil || event.RunID == "" {
		return appErrors.New(appErrors.CodeInvalidArgument, "event or run ID cannot be empty", appErrors.LayerRepository)
	}

	select {
	case <-ctx.Done():
		return ctx.Err()
	default:
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	dir := s.runDir(event.RunID)
	_ = os.MkdirAll(dir, 0755)

	f, err := os.OpenFile(s.eventsFile(event.RunID), os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0644)
	if err != nil {
		return appErrors.Wrap(err, appErrors.CodeInternal, "failed to open events file", appErrors.LayerRepository)
	}
	defer f.Close()

	data, err := json.Marshal(event)
	if err != nil {
		return appErrors.Wrap(err, appErrors.CodeInternal, "failed to marshal event", appErrors.LayerRepository)
	}

	if _, err := f.Write(append(data, '\n')); err != nil {
		return appErrors.Wrap(err, appErrors.CodeInternal, "failed to append event", appErrors.LayerRepository)
	}

	return nil
}

// ReplayEvents reads all persisted events for a run in chronological sequence (TASK-7.3)
func (s *DiskStore) ReplayEvents(ctx context.Context, runID string) ([]*run.RunEvent, error) {
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	default:
	}

	s.mu.RLock()
	defer s.mu.RUnlock()

	filePath := s.eventsFile(runID)
	f, err := os.Open(filePath)
	if err != nil {
		if os.IsNotExist(err) {
			return []*run.RunEvent{}, nil
		}
		return nil, appErrors.Wrap(err, appErrors.CodeInternal, "failed to open events file for replay", appErrors.LayerRepository)
	}
	defer f.Close()

	var events []*run.RunEvent
	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		line := scanner.Bytes()
		if len(line) == 0 {
			continue
		}
		var ev run.RunEvent
		if err := json.Unmarshal(line, &ev); err == nil {
			events = append(events, &ev)
		}
	}

	if err := scanner.Err(); err != nil {
		return nil, appErrors.Wrap(err, appErrors.CodeInternal, "error reading events file", appErrors.LayerRepository)
	}

	return events, nil
}

// SaveTasks persists tasks for a run
func (s *DiskStore) SaveTasks(ctx context.Context, runID string, tasks []*task.Task) error {
	select {
	case <-ctx.Done():
		return ctx.Err()
	default:
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	dir := s.runDir(runID)
	_ = os.MkdirAll(dir, 0755)

	data, err := json.MarshalIndent(tasks, "", "  ")
	if err != nil {
		return appErrors.Wrap(err, appErrors.CodeInternal, "failed to marshal tasks", appErrors.LayerRepository)
	}

	return os.WriteFile(s.tasksFile(runID), data, 0644)
}

// LoadTasks reads tasks for a run from disk
func (s *DiskStore) LoadTasks(ctx context.Context, runID string) ([]*task.Task, error) {
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	default:
	}

	s.mu.RLock()
	defer s.mu.RUnlock()

	data, err := os.ReadFile(s.tasksFile(runID))
	if err != nil {
		if os.IsNotExist(err) {
			return []*task.Task{}, nil
		}
		return nil, appErrors.Wrap(err, appErrors.CodeInternal, "failed to read tasks file", appErrors.LayerRepository)
	}

	var tasks []*task.Task
	if err := json.Unmarshal(data, &tasks); err != nil {
		return nil, appErrors.Wrap(err, appErrors.CodeInternal, "failed to unmarshal tasks", appErrors.LayerRepository)
	}

	return tasks, nil
}

// ResumeInterruptedRuns scans disk on startup and recovers interrupted runs (TASK-7.2)
func (s *DiskStore) ResumeInterruptedRuns(ctx context.Context) ([]*run.Run, error) {
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	default:
	}

	runsRoot := filepath.Join(s.baseDir, "runs")
	entries, err := os.ReadDir(runsRoot)
	if err != nil {
		if os.IsNotExist(err) {
			return []*run.Run{}, nil
		}
		return nil, err
	}

	var recovered []*run.Run
	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		r, err := s.GetRun(ctx, entry.Name())
		if err != nil || r == nil {
			continue
		}

		// If run was left in running or planning state during restart
		if r.Status == run.StatusRunning || r.Status == run.StatusPlanning {
			r.ErrorMessage = "resumed after engine restart"
			_ = s.SaveRun(ctx, r)
			recovered = append(recovered, r)
		}
	}

	return recovered, nil
}
