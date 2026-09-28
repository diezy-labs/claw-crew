package memory_test

import (
	"context"
	"testing"

	"github.com/diezy-labs/claw-crew/engine/src/memory"
)

func TestVectorValidation(t *testing.T) {
	ctx := context.Background()
	store := memory.NewVectorStore()

	// 1. Insert empty vector -> must return explicit validation error
	err := store.Store(ctx, &memory.Document{
		ID:        "doc_empty",
		Content:   "empty",
		Embedding: []float32{},
	})
	if err == nil {
		t.Fatalf("expected error storing document with empty embedding, got nil")
	}

	// 2. Insert valid vector
	err = store.Store(ctx, &memory.Document{
		ID:        "doc_valid",
		Content:   "valid 3d",
		Embedding: []float32{1.0, 0.0, 0.0},
	})
	if err != nil {
		t.Fatalf("unexpected error storing valid doc: %v", err)
	}

	// 3. Insert mismatched dimension vector -> must reject
	err = store.Store(ctx, &memory.Document{
		ID:        "doc_mismatch",
		Content:   "mismatched 2d",
		Embedding: []float32{1.0, 0.0},
	})
	if err == nil {
		t.Fatalf("expected error storing document with mismatched dimension, got nil")
	}

	// 4. Query with mismatched dimension -> must reject
	_, err = store.Search(ctx, []float32{1.0, 0.0}, 5)
	if err == nil {
		t.Fatalf("expected error querying with mismatched dimension, got nil")
	}

	// 5. Query with topK = 0 -> returns empty slice safely
	res, err := store.Search(ctx, []float32{1.0, 0.0, 0.0}, 0)
	if err != nil {
		t.Fatalf("unexpected error with topK=0: %v", err)
	}
	if len(res) != 0 {
		t.Fatalf("expected 0 results with topK=0, got %d", len(res))
	}

	// 6. Query with topK < 0 -> returns validation error
	_, err = store.Search(ctx, []float32{1.0, 0.0, 0.0}, -1)
	if err == nil {
		t.Fatalf("expected error querying with topK < 0, got nil")
	}

	// 7. Query with topK > stored items -> returns all items safely without panic
	res, err = store.Search(ctx, []float32{1.0, 0.0, 0.0}, 100)
	if err != nil {
		t.Fatalf("unexpected error querying topK > items: %v", err)
	}
	if len(res) != 1 {
		t.Fatalf("expected 1 result, got %d", len(res))
	}

	// 8. Zero-magnitude vector -> must not panic or return NaN
	err = store.Store(ctx, &memory.Document{
		ID:        "doc_zero_norm",
		Content:   "zero norm vector",
		Embedding: []float32{0.0, 0.0, 0.0},
	})
	if err != nil {
		t.Fatalf("unexpected error storing zero-norm vector: %v", err)
	}

	res, err = store.Search(ctx, []float32{0.0, 0.0, 0.0}, 5)
	if err != nil {
		t.Fatalf("unexpected error querying with zero-norm: %v", err)
	}
	for _, r := range res {
		if r.Score != 0.0 {
			t.Errorf("expected 0 score for zero-norm, got %f", r.Score)
		}
	}
}
