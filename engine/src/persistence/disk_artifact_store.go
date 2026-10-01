package persistence

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"sync"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/src/artifact"
)

// DiskArtifactStore implements artifact.Repository with one JSON file per
// artifact on disk, so treasures survive engine restarts (F1-1b).
type DiskArtifactStore struct {
	baseDir string
	mu      sync.RWMutex
}

// NewDiskArtifactStore constructs a disk-backed artifact repo under baseDir/artifacts.
func NewDiskArtifactStore(baseDir string) (*DiskArtifactStore, error) {
	dir := filepath.Join(baseDir, "artifacts")
	if err := os.MkdirAll(dir, 0755); err != nil {
		return nil, appErrors.Wrap(err, appErrors.CodeInternal, "init artifact store dir", appErrors.LayerRepository)
	}
	return &DiskArtifactStore{baseDir: dir}, nil
}

func (s *DiskArtifactStore) file(id string) string {
	return filepath.Join(s.baseDir, id+".json")
}

func (s *DiskArtifactStore) Save(ctx context.Context, a *artifact.Artifact) error {
	if a == nil || a.ID == "" {
		return appErrors.New(appErrors.CodeInvalidArgument, "artifact or ID empty", appErrors.LayerRepository)
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	data, err := json.MarshalIndent(a, "", "  ")
	if err != nil {
		return appErrors.Wrap(err, appErrors.CodeInternal, "marshal artifact", appErrors.LayerRepository)
	}
	if err := os.WriteFile(s.file(a.ID), data, 0644); err != nil {
		return appErrors.Wrap(err, appErrors.CodeInternal, "write artifact file", appErrors.LayerRepository)
	}
	return nil
}

func (s *DiskArtifactStore) Get(ctx context.Context, id string) (*artifact.Artifact, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	data, err := os.ReadFile(s.file(id))
	if err != nil {
		if os.IsNotExist(err) {
			return nil, appErrors.New(appErrors.CodeNotFound, "artifact not found: "+id, appErrors.LayerRepository)
		}
		return nil, appErrors.Wrap(err, appErrors.CodeInternal, "read artifact file", appErrors.LayerRepository)
	}
	var a artifact.Artifact
	if err := json.Unmarshal(data, &a); err != nil {
		return nil, appErrors.Wrap(err, appErrors.CodeInternal, "unmarshal artifact", appErrors.LayerRepository)
	}
	return &a, nil
}

func (s *DiskArtifactStore) ListByRun(ctx context.Context, runID string) ([]*artifact.Artifact, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	entries, err := os.ReadDir(s.baseDir)
	if err != nil {
		return nil, appErrors.Wrap(err, appErrors.CodeInternal, "list artifacts dir", appErrors.LayerRepository)
	}
	var out []*artifact.Artifact
	for _, e := range entries {
		if e.IsDir() || filepath.Ext(e.Name()) != ".json" {
			continue
		}
		data, err := os.ReadFile(filepath.Join(s.baseDir, e.Name()))
		if err != nil {
			continue
		}
		var a artifact.Artifact
		if err := json.Unmarshal(data, &a); err != nil {
			continue
		}
		if a.RunID == runID {
			ac := a
			out = append(out, &ac)
		}
	}
	return out, nil
}
