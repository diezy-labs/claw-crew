package artifact

import (
	"context"
)

// Repository manages persistence for artifacts
type Repository interface {
	Save(ctx context.Context, a *Artifact) error
	Get(ctx context.Context, id string) (*Artifact, error)
	ListByRun(ctx context.Context, runID string) ([]*Artifact, error)
}

// Service coordinates artifact creation, retrieval, and semantic diffing
type Service interface {
	CreateArtifact(ctx context.Context, req *CreateArtifactRequest) (*Artifact, error)
	GetArtifact(ctx context.Context, id string) (*Artifact, error)
	ListArtifacts(ctx context.Context, runID string) ([]*Artifact, error)
	GenerateGitDiffArtifact(ctx context.Context, runID, taskID, filePath, original, modified string) (*Artifact, error)
}
