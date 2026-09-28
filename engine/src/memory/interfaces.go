package memory

import (
	"context"
	"time"
)

// Message represents a conversational turn in short-term session memory
type Message struct {
	Role       string    `json:"role"`
	Content    string    `json:"content"`
	TokenCount int       `json:"token_count"`
	CreatedAt  time.Time `json:"created_at"`
}

// SessionMemory defines the short-term working memory per session/run
type SessionMemory interface {
	Append(ctx context.Context, sessionID string, msg *Message) error
	GetHistory(ctx context.Context, sessionID string) ([]*Message, error)
	Clear(ctx context.Context, sessionID string) error
}

// VectorStore defines the storage and similarity search contract for RAG
type VectorStore interface {
	Store(ctx context.Context, doc *Document) error
	Search(ctx context.Context, queryEmbedding []float32, topK int) ([]*SearchResult, error)
	SearchWithScope(ctx context.Context, queryEmbedding []float32, topK int, scope map[string]string) ([]*SearchResult, error)
	SearchByText(ctx context.Context, text string, topK int) ([]*SearchResult, error)
}

// Document contains a text chunk and its dense vector embedding
type Document struct {
	ID        string            `json:"id"`
	Content   string            `json:"content"`
	Embedding []float32         `json:"embedding"`
	Metadata  map[string]string `json:"metadata"`
	Scope     map[string]string `json:"scope,omitempty"`
}

// Clone creates a deep copy of the document to avoid pointer/slice aliasing
func (d *Document) Clone() *Document {
	if d == nil {
		return nil
	}
	res := &Document{
		ID:        d.ID,
		Content:   d.Content,
		Embedding: append([]float32(nil), d.Embedding...),
		Metadata:  make(map[string]string, len(d.Metadata)),
		Scope:     make(map[string]string, len(d.Scope)),
	}
	for k, v := range d.Metadata {
		res.Metadata[k] = v
	}
	for k, v := range d.Scope {
		res.Scope[k] = v
	}
	return res
}

// MatchesScope checks if the document satisfies the provided tenant/workspace scope filters
func (d *Document) MatchesScope(scope map[string]string) bool {
	if len(scope) == 0 {
		return true
	}
	for k, v := range scope {
		val, ok := d.Scope[k]
		if !ok && d.Metadata != nil {
			val, ok = d.Metadata[k]
		}
		if !ok || val != v {
			return false
		}
	}
	return true
}

// SearchResult represents a similarity match result
type SearchResult struct {
	Document *Document `json:"document"`
	Score    float32   `json:"score"`
}

// ContextPacker packs retrieved memory documents into a bounded prompt context window
type ContextPacker interface {
	Pack(results []*SearchResult, maxTokens int) (string, int)
}

// Service coordinates session memory, vector store, retrieval tracing, and context packing
type Service interface {
	Session() SessionMemory
	Vector() VectorStore
	Packer() ContextPacker
	RetrieveWithTrace(ctx context.Context, runID string, query string, topK int, maxTokens int) (string, error)
}
