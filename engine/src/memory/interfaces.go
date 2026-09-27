package memory

import "context"

// VectorStore mendefinisikan antarmuka penyimpanan dan pencarian vektor RAG
type VectorStore interface {
	Store(ctx context.Context, doc *Document) error
	Search(ctx context.Context, queryEmbedding []float32, topK int) ([]*SearchResult, error)
}

// Document memuat potongan teks dan representasi vektornya
type Document struct {
	ID        string            `json:"id"`
	Content   string            `json:"content"`
	Embedding []float32         `json:"embedding"`
	Metadata  map[string]string `json:"metadata"`
}

// SearchResult hasil pencarian similarity
type SearchResult struct {
	Document *Document `json:"document"`
	Score    float32   `json:"score"`
}
