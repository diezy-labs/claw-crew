package artifact

import "time"

// Artifact represents a recorded output product of agent execution
type Artifact struct {
	ID        string    `json:"id"`
	RunID     string    `json:"run_id"`
	TaskID    string    `json:"task_id,omitempty"`
	Type      string    `json:"type"` // git_diff, file, report, diagram
	Path      string    `json:"path,omitempty"`
	Summary   string    `json:"summary"`
	MimeType  string    `json:"mime_type"`
	Content   string    `json:"content,omitempty"`
	Hash      string    `json:"hash"`
	SizeBytes int64     `json:"size_bytes"`
	CreatedAt time.Time `json:"created_at"`
}

// CreateArtifactRequest DTO
type CreateArtifactRequest struct {
	RunID    string `json:"run_id"`
	TaskID   string `json:"task_id,omitempty"`
	Type     string `json:"type"`
	Path     string `json:"path,omitempty"`
	Summary  string `json:"summary"`
	MimeType string `json:"mime_type"`
	Content  string `json:"content"`
}

// ArtifactListResponse DTO
type ArtifactListResponse struct {
	RunID     string      `json:"run_id"`
	Artifacts []*Artifact `json:"artifacts"`
}
