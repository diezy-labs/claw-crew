package memory_test

import (
	"context"
	"testing"

	"github.com/diezy-labs/claw-crew/engine/src/memory"
)

func TestDeterministicRanking(t *testing.T) {
	ctx := context.Background()
	store := memory.NewVectorStore()

	// Store multiple documents with identical embeddings (same cosine similarity score)
	identicalEmbedding := []float32{1.0, 1.0, 1.0}

	ids := []string{"doc_z", "doc_a", "doc_m", "doc_b", "doc_c"}
	for _, id := range ids {
		err := store.Store(ctx, &memory.Document{
			ID:        id,
			Content:   "Document " + id,
			Embedding: identicalEmbedding,
		})
		if err != nil {
			t.Fatalf("failed to store doc %s: %v", id, err)
		}
	}

	// Query multiple times, check if order is deterministically doc_a, doc_b, doc_c, doc_m, doc_z (tie-breaker by ID)
	for trial := 0; trial < 10; trial++ {
		results, err := store.Search(ctx, identicalEmbedding, 10)
		if err != nil {
			t.Fatalf("search trial %d failed: %v", trial, err)
		}
		if len(results) != len(ids) {
			t.Fatalf("trial %d: expected %d results, got %d", trial, len(ids), len(results))
		}

		expectedOrder := []string{"doc_a", "doc_b", "doc_c", "doc_m", "doc_z"}
		for i, expID := range expectedOrder {
			if results[i].Document.ID != expID {
				t.Fatalf("trial %d: expected index %d to be %s, got %s (nondeterministic sort)",
					trial, i, expID, results[i].Document.ID)
			}
		}
	}
}
