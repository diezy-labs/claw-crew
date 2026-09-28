package artifact

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"strings"
	"sync"
	"time"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/core/id"
	"github.com/diezy-labs/claw-crew/engine/src/run"
)

// MemoryRepository stores artifacts in-memory
type MemoryRepository struct {
	mu             sync.RWMutex
	artifacts      map[string]*Artifact
	artifactsByRun map[string][]string
}

// NewMemoryRepository creates an in-memory artifact repository
func NewMemoryRepository() *MemoryRepository {
	return &MemoryRepository{
		artifacts:      make(map[string]*Artifact),
		artifactsByRun: make(map[string][]string),
	}
}

func (r *MemoryRepository) Save(ctx context.Context, a *Artifact) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.artifacts[a.ID] = a
	r.artifactsByRun[a.RunID] = append(r.artifactsByRun[a.RunID], a.ID)
	return nil
}

func (r *MemoryRepository) Get(ctx context.Context, id string) (*Artifact, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	a, ok := r.artifacts[id]
	if !ok {
		return nil, appErrors.New(appErrors.CodeNotFound, fmt.Sprintf("artifact not found: %s", id), appErrors.LayerService)
	}
	return a, nil
}

func (r *MemoryRepository) ListByRun(ctx context.Context, runID string) ([]*Artifact, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	ids := r.artifactsByRun[runID]
	list := make([]*Artifact, 0, len(ids))
	for _, id := range ids {
		if a, ok := r.artifacts[id]; ok {
			list = append(list, a)
		}
	}
	return list, nil
}

type artifactService struct {
	repo       Repository
	runService run.Service
}

// NewService creates a new artifact service
func NewService(repo Repository, runService run.Service) Service {
	return &artifactService{
		repo:       repo,
		runService: runService,
	}
}

func hashContent(content string) string {
	h := sha256.Sum256([]byte(content))
	return hex.EncodeToString(h[:])
}

func (s *artifactService) CreateArtifact(ctx context.Context, req *CreateArtifactRequest) (*Artifact, error) {
	if req.RunID == "" {
		return nil, appErrors.New(appErrors.CodeInvalidArgument, "run_id is required", appErrors.LayerService)
	}
	if req.Summary == "" {
		return nil, appErrors.New(appErrors.CodeInvalidArgument, "summary is required", appErrors.LayerService)
	}

	artID := id.NewArtifactID()
	now := time.Now().UTC()
	contentHash := hashContent(req.Content)

	mime := req.MimeType
	if mime == "" {
		mime = "text/plain"
	}

	art := &Artifact{
		ID:        artID,
		RunID:     req.RunID,
		TaskID:    req.TaskID,
		Type:      req.Type,
		Path:      req.Path,
		Summary:   req.Summary,
		MimeType:  mime,
		Content:   req.Content,
		Hash:      contentHash,
		SizeBytes: int64(len(req.Content)),
		CreatedAt: now,
	}

	if err := s.repo.Save(ctx, art); err != nil {
		return nil, err
	}

	if s.runService != nil {
		s.runService.PublishEvent(req.RunID, "artifact.created", map[string]any{
			"artifact_id": artID,
			"type":        art.Type,
			"path":        art.Path,
			"summary":     art.Summary,
		}, "")
	}

	return art, nil
}

func (s *artifactService) GetArtifact(ctx context.Context, id string) (*Artifact, error) {
	return s.repo.Get(ctx, id)
}

func (s *artifactService) ListArtifacts(ctx context.Context, runID string) ([]*Artifact, error) {
	return s.repo.ListByRun(ctx, runID)
}

func (s *artifactService) GenerateGitDiffArtifact(ctx context.Context, runID, taskID, filePath, original, modified string) (*Artifact, error) {
	diffBuilder := &strings.Builder{}
	fmt.Fprintf(diffBuilder, "--- a/%s\n+++ b/%s\n", filePath, filePath)

	origLines := strings.Split(original, "\n")
	modLines := strings.Split(modified, "\n")

	additions := 0
	deletions := 0

	// Simple semantic unified line diff calculation
	for _, l := range origLines {
		if !containsLine(modLines, l) && l != "" {
			fmt.Fprintf(diffBuilder, "-%s\n", l)
			deletions++
		}
	}
	for _, l := range modLines {
		if !containsLine(origLines, l) && l != "" {
			fmt.Fprintf(diffBuilder, "+%s\n", l)
			additions++
		}
	}

	diffContent := diffBuilder.String()
	summary := fmt.Sprintf("+%d -%d lines modified in %s", additions, deletions, filePath)

	return s.CreateArtifact(ctx, &CreateArtifactRequest{
		RunID:    runID,
		TaskID:   taskID,
		Type:     "git_diff",
		Path:     filePath,
		Summary:  summary,
		MimeType: "text/x-diff",
		Content:  diffContent,
	})
}

func containsLine(lines []string, target string) bool {
	for _, l := range lines {
		if l == target {
			return true
		}
	}
	return false
}
