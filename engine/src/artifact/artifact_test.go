package artifact

import (
	"context"
	"strings"
	"testing"

	"github.com/diezy-labs/claw-crew/engine/src/run"
)

func TestArtifactLifecycleAndDiff(t *testing.T) {
	repo := NewMemoryRepository()
	runStore := run.NewMemoryStore()
	runHub := run.NewEventHub()
	runSvc := run.NewService(runStore, runHub)

	svc := NewService(repo, runSvc)
	ctx := context.Background()

	// 1. Create standard report artifact
	art, err := svc.CreateArtifact(ctx, &CreateArtifactRequest{
		RunID:    "run_art_01",
		TaskID:   "task_01",
		Type:     "report",
		Summary:  "Architecture review summary",
		MimeType: "text/markdown",
		Content:  "# ClawCrew Review\nAll checks passed.",
	})
	if err != nil {
		t.Fatalf("failed to create artifact: %v", err)
	}

	if !strings.HasPrefix(art.ID, "art_") {
		t.Fatalf("expected art_ prefix, got %s", art.ID)
	}
	if art.Hash == "" {
		t.Fatal("expected content hash to be computed")
	}

	// 2. Generate semantic git diff artifact
	orig := "fn main() {\n    println!(\"hello\");\n}\n"
	mod := "fn main() {\n    println!(\"hello world\");\n}\n"

	diffArt, err := svc.GenerateGitDiffArtifact(ctx, "run_art_01", "task_02", "src/main.rs", orig, mod)
	if err != nil {
		t.Fatalf("failed to generate git diff artifact: %v", err)
	}

	if diffArt.Type != "git_diff" {
		t.Fatalf("expected type git_diff, got %s", diffArt.Type)
	}
	if !strings.Contains(diffArt.Content, "--- a/src/main.rs") {
		t.Fatalf("expected unified diff header, got %s", diffArt.Content)
	}
	if !strings.Contains(diffArt.Content, "+    println!(\"hello world\");") {
		t.Fatalf("expected addition in diff, got %s", diffArt.Content)
	}

	// 3. List artifacts by run
	list, err := svc.ListArtifacts(ctx, "run_art_01")
	if err != nil {
		t.Fatalf("failed to list artifacts: %v", err)
	}
	if len(list) != 2 {
		t.Fatalf("expected 2 artifacts, got %d", len(list))
	}
}
