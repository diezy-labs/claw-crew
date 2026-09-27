package memory_test

import (
	"context"
	"testing"
	"time"

	"github.com/diezy-labs/claw-crew/engine/src/memory"
)

func TestVectorStoreIndexAndSearch(t *testing.T) {
	store := memory.NewVectorStore()
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()

	// 1. Insert documents with distinct embeddings
	doc1 := &memory.Document{
		ID:        "doc_1",
		Content:   "Go 1.27 introduced standard library encoding/json/v2",
		Embedding: []float32{1.0, 0.0, 0.0, 0.0},
		Metadata:  map[string]string{"category": "golang"},
	}
	doc2 := &memory.Document{
		ID:        "doc_2",
		Content:   "Rust Tauri provides lightweight native desktop webview",
		Embedding: []float32{0.0, 1.0, 0.0, 0.0},
		Metadata:  map[string]string{"category": "rust"},
	}
	doc3 := &memory.Document{
		ID:        "doc_3",
		Content:   "Hybrid desktop agent combining Rust shell with Go AI engine",
		Embedding: []float32{0.7, 0.7, 0.0, 0.0},
		Metadata:  map[string]string{"category": "architecture"},
	}

	if err := store.Store(ctx, doc1); err != nil {
		t.Fatalf("failed to store doc1: %v", err)
	}
	if err := store.Store(ctx, doc2); err != nil {
		t.Fatalf("failed to store doc2: %v", err)
	}
	if err := store.Store(ctx, doc3); err != nil {
		t.Fatalf("failed to store doc3: %v", err)
	}

	// 2. Query with vector closest to doc1
	queryVector := []float32{0.9, 0.1, 0.0, 0.0}
	matches, err := store.Search(ctx, queryVector, 2)
	if err != nil {
		t.Fatalf("search failed: %v", err)
	}

	if len(matches) != 2 {
		t.Fatalf("expected 2 matches, got %d", len(matches))
	}

	// First match must be doc1
	if matches[0].Document.ID != "doc_1" {
		t.Errorf("expected top match to be doc_1, got %s (score: %f)", matches[0].Document.ID, matches[0].Score)
	}

	if matches[0].Score <= matches[1].Score {
		t.Errorf("expected matches to be ordered descending by score")
	}
}

func TestVectorStoreValidation(t *testing.T) {
	store := memory.NewVectorStore()
	ctx := context.Background()

	// Nil document
	if err := store.Store(ctx, nil); err == nil {
		t.Errorf("expected error for nil document")
	}

	// Empty ID
	if err := store.Store(ctx, &memory.Document{ID: ""}); err == nil {
		t.Errorf("expected error for document with empty ID")
	}

	// Empty query vector
	if _, err := store.Search(ctx, nil, 5); err == nil {
		t.Errorf("expected error for empty search vector")
	}
}
