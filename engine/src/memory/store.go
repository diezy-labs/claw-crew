package memory

import (
	"context"
	"fmt"
	"math"
	"sort"
	"strings"
	"sync"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
)

type inMemoryVectorStore struct {
	mu        sync.RWMutex
	dimension int
	documents map[string]*Document
}

// NewVectorStore creates a new in-memory VectorStore instance
func NewVectorStore() VectorStore {
	return &inMemoryVectorStore{
		documents: make(map[string]*Document),
	}
}

func (s *inMemoryVectorStore) Store(ctx context.Context, doc *Document) error {
	if doc == nil {
		return appErrors.New(appErrors.CodeInvalidArgument, "document cannot be nil", appErrors.LayerRepository)
	}
	if doc.ID == "" {
		return appErrors.New(appErrors.CodeInvalidArgument, "document ID cannot be empty", appErrors.LayerRepository)
	}
	if len(doc.Embedding) == 0 {
		return appErrors.New(appErrors.CodeInvalidArgument, "embedding cannot be empty", appErrors.LayerRepository)
	}

	select {
	case <-ctx.Done():
		return ctx.Err()
	default:
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	// Validate vector dimension consistency across stored documents
	if s.dimension == 0 {
		s.dimension = len(doc.Embedding)
	} else if len(doc.Embedding) != s.dimension {
		return appErrors.New(appErrors.CodeInvalidArgument, fmt.Sprintf("embedding dimension mismatch: expected %d, got %d", s.dimension, len(doc.Embedding)), appErrors.LayerRepository)
	}

	// Deep clone document to avoid race conditions and aliasing on caller mutations
	s.documents[doc.ID] = doc.Clone()
	return nil
}

// Delete removes a document by ID. It is the reversibility hook for learned-rule
// proposals (memory.ProposalStore.Revert); deleting an absent ID is a no-op.
func (s *inMemoryVectorStore) Delete(ctx context.Context, id string) error {
	select {
	case <-ctx.Done():
		return ctx.Err()
	default:
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	delete(s.documents, id)
	return nil
}

func (s *inMemoryVectorStore) Search(ctx context.Context, queryEmbedding []float32, topK int) ([]*SearchResult, error) {
	return s.SearchWithScope(ctx, queryEmbedding, topK, nil)
}

func (s *inMemoryVectorStore) SearchWithScope(ctx context.Context, queryEmbedding []float32, topK int, scope map[string]string) ([]*SearchResult, error) {
	if len(queryEmbedding) == 0 {
		return nil, appErrors.New(appErrors.CodeInvalidArgument, "query embedding cannot be empty", appErrors.LayerRepository)
	}
	if topK < 0 {
		return nil, appErrors.New(appErrors.CodeInvalidArgument, "topK cannot be negative", appErrors.LayerRepository)
	}
	if topK == 0 {
		return []*SearchResult{}, nil
	}

	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	default:
	}

	s.mu.RLock()
	defer s.mu.RUnlock()

	if s.dimension > 0 && len(queryEmbedding) != s.dimension {
		return nil, appErrors.New(appErrors.CodeInvalidArgument, fmt.Sprintf("query embedding dimension mismatch: expected %d, got %d", s.dimension, len(queryEmbedding)), appErrors.LayerRepository)
	}

	results := make([]*SearchResult, 0, len(s.documents))

	for _, doc := range s.documents {
		if len(doc.Embedding) == 0 || len(doc.Embedding) != len(queryEmbedding) {
			continue
		}
		if !doc.MatchesScope(scope) {
			continue
		}

		score := cosineSimilarity(queryEmbedding, doc.Embedding)
		results = append(results, &SearchResult{
			Document: doc.Clone(), // Deep copy output to eliminate aliasing hazard
			Score:    score,
		})
	}

	// Sort results deterministically by score desc, then by document ID asc (tie-breaker)
	sort.Slice(results, func(i, j int) bool {
		if results[i].Score != results[j].Score {
			return results[i].Score > results[j].Score
		}
		return results[i].Document.ID < results[j].Document.ID
	})

	if len(results) > topK {
		results = results[:topK]
	}

	return results, nil
}

func (s *inMemoryVectorStore) SearchByText(ctx context.Context, text string, topK int) ([]*SearchResult, error) {
	if text == "" {
		return nil, appErrors.New(appErrors.CodeInvalidArgument, "query text cannot be empty", appErrors.LayerRepository)
	}
	if topK < 0 {
		return nil, appErrors.New(appErrors.CodeInvalidArgument, "topK cannot be negative", appErrors.LayerRepository)
	}
	if topK == 0 {
		return []*SearchResult{}, nil
	}

	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	default:
	}

	s.mu.RLock()
	defer s.mu.RUnlock()

	lowerQuery := strings.ToLower(text)
	words := strings.Fields(lowerQuery)
	var results []*SearchResult

	for _, doc := range s.documents {
		score := float32(0.0)
		lowerContent := strings.ToLower(doc.Content)
		if strings.Contains(lowerContent, lowerQuery) {
			score = 1.0
		} else if len(words) > 0 {
			matchCount := 0
			for _, w := range words {
				if strings.Contains(lowerContent, w) {
					matchCount++
				}
			}
			if matchCount > 0 {
				score = float32(matchCount) / float32(len(words))
			}
		}

		if score > 0 {
			results = append(results, &SearchResult{
				Document: doc.Clone(),
				Score:    score,
			})
		}
	}

	// Sort results deterministically by score desc, then by document ID asc
	sort.Slice(results, func(i, j int) bool {
		if results[i].Score != results[j].Score {
			return results[i].Score > results[j].Score
		}
		return results[i].Document.ID < results[j].Document.ID
	})

	if len(results) > topK {
		results = results[:topK]
	}

	return results, nil
}

// cosineSimilarity calculates cosine angle between two float32 vectors
func cosineSimilarity(a, b []float32) float32 {
	var dotProduct, normA, normB float64

	for i := 0; i < len(a); i++ {
		valA := float64(a[i])
		valB := float64(b[i])
		dotProduct += valA * valB
		normA += valA * valA
		normB += valB * valB
	}

	if normA == 0 || normB == 0 {
		return 0
	}

	return float32(dotProduct / (math.Sqrt(normA) * math.Sqrt(normB)))
}
