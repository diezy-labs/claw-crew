package memory

import "context"

// VectorStore defines the storage and similarity search contract for RAG
type VectorStore interface {
	Store(ctx context.Context, doc *Document) error
	Search(ctx context.Context, queryEmbedding []float32, topK int) ([]*SearchResult, error)
}

// Document contains a text chunk and its dense vector embedding
type Document struct {
	ID        string            `json:"id"`
	Content   string            `json:"content"`
	Embedding []float32         `json:"embedding"`
	Metadata  map[string]string `json:"metadata"`
}

// SearchResult represents a similarity match result
type SearchResult struct {
	Document *Document `json:"document"`
	Score    float32   `json:"score"`
}
