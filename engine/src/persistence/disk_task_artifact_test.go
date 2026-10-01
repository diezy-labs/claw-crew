package persistence

import (
	"context"
	"os"
	"testing"

	"github.com/diezy-labs/claw-crew/engine/src/artifact"
	"github.com/diezy-labs/claw-crew/engine/src/task"
)

func TestDiskTaskStore_RoundTrip(t *testing.T) {
	dir, _ := os.MkdirTemp("", "disk_task_*")
	defer os.RemoveAll(dir)
	store, err := NewDiskTaskStore(dir)
	if err != nil {
		t.Fatalf("NewDiskTaskStore: %v", err)
	}
	ctx := context.Background()

	tk := &task.Task{ID: "task-1", RunID: "run-1", Title: "step", Status: task.StatusPending}
	if err := store.Save(ctx, tk); err != nil {
		t.Fatalf("Save: %v", err)
	}
	if err := store.UpdateStatus(ctx, "task-1", task.StatusCompleted, ""); err != nil {
		t.Fatalf("UpdateStatus: %v", err)
	}
	got, err := store.Get(ctx, "task-1")
	if err != nil || got.Status != task.StatusCompleted {
		t.Fatalf("Get after update: %+v err=%v", got, err)
	}
	list, err := store.ListByRun(ctx, "run-1")
	if err != nil || len(list) != 1 {
		t.Fatalf("ListByRun: %v len=%d", err, len(list))
	}
}

func TestDiskArtifactStore_RoundTrip(t *testing.T) {
	dir, _ := os.MkdirTemp("", "disk_artifact_*")
	defer os.RemoveAll(dir)
	repo, err := NewDiskArtifactStore(dir)
	if err != nil {
		t.Fatalf("NewDiskArtifactStore: %v", err)
	}
	ctx := context.Background()

	a := &artifact.Artifact{ID: "art-1", RunID: "run-1"}
	if err := repo.Save(ctx, a); err != nil {
		t.Fatalf("Save: %v", err)
	}
	got, err := repo.Get(ctx, "art-1")
	if err != nil || got.ID != "art-1" {
		t.Fatalf("Get: %+v err=%v", got, err)
	}
	list, err := repo.ListByRun(ctx, "run-1")
	if err != nil || len(list) != 1 {
		t.Fatalf("ListByRun: %v len=%d", err, len(list))
	}
}
