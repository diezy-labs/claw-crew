package persistence

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"sync"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/src/task"
)

// DiskTaskStore implements task.Store with one JSON file per task on disk,
// so task state survives engine restarts (F1-1b).
type DiskTaskStore struct {
	baseDir string
	mu      sync.RWMutex
}

// NewDiskTaskStore constructs a disk-backed task store under baseDir/tasks.
func NewDiskTaskStore(baseDir string) (*DiskTaskStore, error) {
	dir := filepath.Join(baseDir, "tasks")
	if err := os.MkdirAll(dir, 0755); err != nil {
		return nil, appErrors.Wrap(err, appErrors.CodeInternal, "init task store dir", appErrors.LayerRepository)
	}
	return &DiskTaskStore{baseDir: dir}, nil
}

func (s *DiskTaskStore) file(taskID string) string {
	return filepath.Join(s.baseDir, taskID+".json")
}

func (s *DiskTaskStore) Save(ctx context.Context, t *task.Task) error {
	if t == nil || t.ID == "" {
		return appErrors.New(appErrors.CodeInvalidArgument, "task or task ID empty", appErrors.LayerRepository)
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	data, err := json.MarshalIndent(t, "", "  ")
	if err != nil {
		return appErrors.Wrap(err, appErrors.CodeInternal, "marshal task", appErrors.LayerRepository)
	}
	if err := os.WriteFile(s.file(t.ID), data, 0644); err != nil {
		return appErrors.Wrap(err, appErrors.CodeInternal, "write task file", appErrors.LayerRepository)
	}
	return nil
}

func (s *DiskTaskStore) Get(ctx context.Context, taskID string) (*task.Task, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	data, err := os.ReadFile(s.file(taskID))
	if err != nil {
		if os.IsNotExist(err) {
			return nil, appErrors.New(appErrors.CodeNotFound, "task not found: "+taskID, appErrors.LayerRepository)
		}
		return nil, appErrors.Wrap(err, appErrors.CodeInternal, "read task file", appErrors.LayerRepository)
	}
	var t task.Task
	if err := json.Unmarshal(data, &t); err != nil {
		return nil, appErrors.Wrap(err, appErrors.CodeInternal, "unmarshal task", appErrors.LayerRepository)
	}
	return &t, nil
}

func (s *DiskTaskStore) ListByRun(ctx context.Context, runID string) ([]*task.Task, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	entries, err := os.ReadDir(s.baseDir)
	if err != nil {
		return nil, appErrors.Wrap(err, appErrors.CodeInternal, "list tasks dir", appErrors.LayerRepository)
	}
	var out []*task.Task
	for _, e := range entries {
		if e.IsDir() || filepath.Ext(e.Name()) != ".json" {
			continue
		}
		data, err := os.ReadFile(filepath.Join(s.baseDir, e.Name()))
		if err != nil {
			continue
		}
		var t task.Task
		if err := json.Unmarshal(data, &t); err != nil {
			continue
		}
		if t.RunID == runID {
			tc := t
			out = append(out, &tc)
		}
	}
	return out, nil
}

func (s *DiskTaskStore) UpdateStatus(ctx context.Context, taskID string, status task.TaskStatus, errMsg string) error {
	t, err := s.Get(ctx, taskID)
	if err != nil {
		return err
	}
	t.Status = status
	if errMsg != "" {
		t.ErrorMessage = errMsg
	}
	return s.Save(ctx, t)
}
