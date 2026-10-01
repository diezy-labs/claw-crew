package memory

import (
	"context"
	"testing"
)

// F3-3 learning-by-consent lifecycle: a correction is written to memory ONLY
// after approval, scoped and reversible.
func TestProposalStore_Lifecycle(t *testing.T) {
	ctx := context.Background()
	vs := NewVectorStore()
	ps := NewProposalStore(vs)

	scope := map[string]string{"ship": "dev-ship", "project": "galleon"}

	// Propose: pending, nothing written yet.
	prop, err := ps.Propose(ctx, "Always run cargo check -p before claiming done", scope)
	if err != nil {
		t.Fatalf("Propose: %v", err)
	}
	if prop.Status != ProposalPending {
		t.Fatalf("expected pending, got %q", prop.Status)
	}
	if res, _ := vs.SearchByText(ctx, "cargo check", 5); len(res) != 0 {
		t.Fatalf("nothing must be written before approval, found %d docs", len(res))
	}

	// Approve: now written, scoped, searchable.
	approved, err := ps.Approve(ctx, prop.ID)
	if err != nil {
		t.Fatalf("Approve: %v", err)
	}
	if approved.Status != ProposalApproved || approved.DocumentID == "" {
		t.Fatalf("expected approved+written, got %+v", approved)
	}
	scoped, _ := vs.SearchWithScope(ctx, []float32{1}, 5, scope)
	if len(scoped) != 1 {
		t.Fatalf("approved rule must be written in-scope, found %d", len(scoped))
	}
	// Out-of-scope search must NOT see it.
	other, _ := vs.SearchWithScope(ctx, []float32{1}, 5, map[string]string{"ship": "other-ship"})
	if len(other) != 0 {
		t.Fatalf("rule leaked outside its scope, found %d", len(other))
	}

	// Approving again is refused (not pending).
	if _, err := ps.Approve(ctx, prop.ID); err == nil {
		t.Fatal("re-approving an approved proposal must fail")
	}

	// Revert: reversible — the written rule is removed.
	reverted, err := ps.Revert(ctx, prop.ID)
	if err != nil {
		t.Fatalf("Revert: %v", err)
	}
	if reverted.Status != ProposalReverted {
		t.Fatalf("expected reverted, got %q", reverted.Status)
	}
	if res, _ := vs.SearchWithScope(ctx, []float32{1}, 5, scope); len(res) != 0 {
		t.Fatalf("reverted rule must be gone, found %d", len(res))
	}
}

// A rejected proposal never writes anything.
func TestProposalStore_RejectWritesNothing(t *testing.T) {
	ctx := context.Background()
	vs := NewVectorStore()
	ps := NewProposalStore(vs)

	prop, err := ps.Propose(ctx, "Never push to master", nil)
	if err != nil {
		t.Fatalf("Propose: %v", err)
	}
	if _, err := ps.Reject(ctx, prop.ID); err != nil {
		t.Fatalf("Reject: %v", err)
	}
	if res, _ := vs.SearchByText(ctx, "master", 5); len(res) != 0 {
		t.Fatalf("rejected proposal must write nothing, found %d", len(res))
	}
	// Approving a rejected proposal is refused.
	if _, err := ps.Approve(ctx, prop.ID); err == nil {
		t.Fatal("approving a rejected proposal must fail")
	}
}

// Re-proposing an identical rule+scope bumps the version (supersede, versioned).
func TestProposalStore_VersionBumpsOnSupersede(t *testing.T) {
	ctx := context.Background()
	ps := NewProposalStore(NewVectorStore())
	scope := map[string]string{"project": "galleon"}

	first, _ := ps.Propose(ctx, "prefer targeted builds", scope)
	second, _ := ps.Propose(ctx, "prefer targeted builds", scope)
	if second.Version <= first.Version {
		t.Fatalf("expected version bump, got v%d then v%d", first.Version, second.Version)
	}
}
