package memory_test

import (
	"context"
	"fmt"
	"sync"
	"testing"

	"github.com/diezy-labs/claw-crew/engine/src/memory"
)

func TestVectorStore_ConcurrentUpsertAndSearch(t *testing.T) {
	store := memory.NewVectorStore()
	ctx := context.Background()

	const workers = 100
	var wg sync.WaitGroup
	wg.Add(workers * 2)

	// Concurrently store 100 documents
	for i := 0; i < workers; i++ {
		go func(idx int) {
			defer wg.Done()
			doc := &memory.Document{
				ID:        fmt.Sprintf("doc_%d", idx),
				Content:   fmt.Sprintf("Concurrent document payload %d", idx),
				Embedding: []float32{float32(idx) * 0.01, float32(idx) * 0.02, 0.5, 0.8},
				Scope: map[string]string{
					"workspace_id": fmt.Sprintf("ws_%d", idx%5),
				},
			}
			_ = store.Store(ctx, doc)
		}(i)
	}

	// Concurrently query the store 100 times
	for i := 0; i < workers; i++ {
		go func(idx int) {
			defer wg.Done()
			query := []float32{0.1, 0.2, 0.5, 0.8}
			_, _ = store.SearchWithScope(ctx, query, 5, map[string]string{
				"workspace_id": fmt.Sprintf("ws_%d", idx%5),
			})
		}(i)
	}

	wg.Wait()

	// Verify all items are retrievable
	results, err := store.Search(ctx, []float32{0.1, 0.2, 0.5, 0.8}, workers)
	if err != nil {
		t.Fatalf("failed to search store: %v", err)
	}
	if len(results) != workers {
		t.Errorf("expected %d documents stored, got %d", workers, len(results))
	}
}
