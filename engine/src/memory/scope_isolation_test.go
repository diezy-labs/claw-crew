package memory_test

import (
	"context"
	"testing"

	"github.com/diezy-labs/claw-crew/engine/src/memory"
)

func TestScopeIsolation(t *testing.T) {
	ctx := context.Background()
	store := memory.NewVectorStore()

	// 1. Store document in workspace A
	err := store.Store(ctx, &memory.Document{
		ID:        "doc_ws_a",
		Content:   "Secret project for Workspace A",
		Embedding: []float32{1.0, 0.0, 0.0},
		Scope: map[string]string{
			"workspace_id": "ws_alpha",
		},
	})
	if err != nil {
		t.Fatalf("failed to store doc in ws_a: %v", err)
	}

	// 2. Store document in workspace B
	err = store.Store(ctx, &memory.Document{
		ID:        "doc_ws_b",
		Content:   "Secret project for Workspace B",
		Embedding: []float32{1.0, 0.0, 0.0},
		Scope: map[string]string{
			"workspace_id": "ws_beta",
		},
	})
	if err != nil {
		t.Fatalf("failed to store doc in ws_b: %v", err)
	}

	// 3. Query with scope for Workspace A -> must not return Workspace B document
	resultsA, err := store.SearchWithScope(ctx, []float32{1.0, 0.0, 0.0}, 10, map[string]string{
		"workspace_id": "ws_alpha",
	})
	if err != nil {
		t.Fatalf("search with scope failed: %v", err)
	}

	if len(resultsA) != 1 {
		t.Fatalf("expected exactly 1 result for ws_alpha, got %d", len(resultsA))
	}
	if resultsA[0].Document.ID != "doc_ws_a" {
		t.Fatalf("expected doc_ws_a, got %s", resultsA[0].Document.ID)
	}

	// 4. Query with scope for Workspace B -> must not return Workspace A document
	resultsB, err := store.SearchWithScope(ctx, []float32{1.0, 0.0, 0.0}, 10, map[string]string{
		"workspace_id": "ws_beta",
	})
	if err != nil {
		t.Fatalf("search with scope failed: %v", err)
	}

	if len(resultsB) != 1 {
		t.Fatalf("expected exactly 1 result for ws_beta, got %d", len(resultsB))
	}
	if resultsB[0].Document.ID != "doc_ws_b" {
		t.Fatalf("expected doc_ws_b, got %s", resultsB[0].Document.ID)
	}
}

func TestDeepCopyAliasingHazard(t *testing.T) {
	ctx := context.Background()
	store := memory.NewVectorStore()

	origEmbedding := []float32{1.0, 0.0, 0.0}
	doc := &memory.Document{
		ID:        "doc_alias",
		Content:   "immutable content",
		Embedding: origEmbedding,
	}

	if err := store.Store(ctx, doc); err != nil {
		t.Fatalf("store failed: %v", err)
	}

	// Mutate caller's slice after storing
	origEmbedding[0] = 999.0

	// Query from store
	res, err := store.Search(ctx, []float32{1.0, 0.0, 0.0}, 1)
	if err != nil {
		t.Fatalf("search failed: %v", err)
	}
	if len(res) == 0 {
		t.Fatalf("expected 1 result")
	}

	// Internal stored embedding must remain 1.0, not 999.0
	if res[0].Document.Embedding[0] != 1.0 {
		t.Fatalf("aliasing hazard detected: internal embedding was modified by caller: %f", res[0].Document.Embedding[0])
	}

	// Mutate result's embedding
	res[0].Document.Embedding[0] = 888.0

	// Query again
	res2, err := store.Search(ctx, []float32{1.0, 0.0, 0.0}, 1)
	if err != nil {
		t.Fatalf("search 2 failed: %v", err)
	}
	if res2[0].Document.Embedding[0] != 1.0 {
		t.Fatalf("aliasing hazard detected: internal embedding was modified by mutating query result: %f", res2[0].Document.Embedding[0])
	}
}
