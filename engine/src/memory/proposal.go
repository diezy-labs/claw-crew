package memory

import (
	"context"
	"fmt"
	"sort"
	"sync"
	"time"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
)

// ProposalStatus is the consent lifecycle of a learned correction (F3-3).
// Learning-by-consent (docs/finalize/01, fundamentals): a correction becomes a
// rule only AFTER the Pirate King approves it — never auto-written.
type ProposalStatus string

const (
	ProposalPending  ProposalStatus = "pending"
	ProposalApproved ProposalStatus = "approved"
	ProposalRejected ProposalStatus = "rejected"
	ProposalReverted ProposalStatus = "reverted"
)

// MemoryProposal is a proposed learning (a rule) awaiting consent. It is scoped
// (Fleet/Workspace/Project/Ship/Crew/Run), versioned, and reversible — a correction
// is never silently self-applied.
type MemoryProposal struct {
	ID         string            `json:"id"`
	Rule       string            `json:"rule"`             // the correction/preference text
	Scope      map[string]string `json:"scope"`            // where it applies (reused by Document.Scope)
	Status     ProposalStatus    `json:"status"`
	Version    int               `json:"version"`          // monotonic per (ID lineage); supports supersede
	Reversible bool              `json:"reversible"`       // approved writes can be reverted
	DocumentID string            `json:"document_id,omitempty"` // set once approved+written
	CreatedAt  time.Time         `json:"created_at"`
	DecidedAt  *time.Time        `json:"decided_at,omitempty"`
}

// ProposalStore records memory proposals and gates their write behind approval.
type ProposalStore interface {
	// Propose records a correction as PENDING. It writes nothing to the vector store.
	Propose(ctx context.Context, rule string, scope map[string]string) (*MemoryProposal, error)
	// Approve writes the rule to the vector store (scoped) and marks it approved.
	// Only a pending proposal can be approved.
	Approve(ctx context.Context, id string) (*MemoryProposal, error)
	// Reject marks a pending proposal rejected; nothing is written.
	Reject(ctx context.Context, id string) (*MemoryProposal, error)
	// Revert undoes an approved proposal: removes the written document (reversible).
	Revert(ctx context.Context, id string) (*MemoryProposal, error)
	// Get returns one proposal by id.
	Get(ctx context.Context, id string) (*MemoryProposal, error)
	// List returns proposals, optionally filtered by status ("" = all), newest first.
	List(ctx context.Context, status ProposalStatus) ([]*MemoryProposal, error)
}

// revertableStore is the subset of VectorStore the proposal store needs. A store
// that also supports Delete is reversible; one that does not degrades to
// non-reversible approval (the proposal is still recorded, just not revertable).
type revertableStore interface {
	Delete(ctx context.Context, id string) error
}

type proposalStore struct {
	mu        sync.Mutex
	vectors   VectorStore
	proposals map[string]*MemoryProposal
	seq       int
}

// NewProposalStore builds a consent-gated learning store over a VectorStore.
func NewProposalStore(vectors VectorStore) ProposalStore {
	return &proposalStore{
		vectors:   vectors,
		proposals: make(map[string]*MemoryProposal),
	}
}

func (p *proposalStore) Propose(ctx context.Context, rule string, scope map[string]string) (*MemoryProposal, error) {
	if rule == "" {
		return nil, appErrors.New(appErrors.CodeInvalidArgument, "proposal rule cannot be empty", appErrors.LayerService)
	}
	p.mu.Lock()
	defer p.mu.Unlock()

	p.seq++
	// Version supersedes any earlier approved proposal with the identical rule+scope.
	version := 1
	for _, ex := range p.proposals {
		if ex.Rule == rule && scopeEqual(ex.Scope, scope) && ex.Version >= version {
			version = ex.Version + 1
		}
	}

	prop := &MemoryProposal{
		ID:         fmt.Sprintf("mp-%d", p.seq),
		Rule:       rule,
		Scope:      cloneScope(scope),
		Status:     ProposalPending,
		Version:    version,
		Reversible: true,
		CreatedAt:  time.Now(),
	}
	p.proposals[prop.ID] = prop
	return clonePro(prop), nil
}

func (p *proposalStore) Approve(ctx context.Context, id string) (*MemoryProposal, error) {
	p.mu.Lock()
	defer p.mu.Unlock()

	prop, ok := p.proposals[id]
	if !ok {
		return nil, appErrors.New(appErrors.CodeNotFound, "proposal not found", appErrors.LayerService)
	}
	if prop.Status != ProposalPending {
		return nil, appErrors.New(appErrors.CodeInvalidArgument, fmt.Sprintf("only a pending proposal can be approved; current status %q", prop.Status), appErrors.LayerService)
	}

	// Write the rule as a scoped document. Embedding is a single-dim placeholder:
	// the real embedding is computed by the retrieval pipeline on index; what matters
	// for the consent boundary is that the write happens ONLY on approval, scoped.
	docID := "mem-" + prop.ID
	doc := &Document{
		ID:        docID,
		Content:   prop.Rule,
		Embedding: []float32{1},
		Metadata:  map[string]string{"kind": "learned_rule", "proposal_id": prop.ID},
		Scope:     cloneScope(prop.Scope),
	}
	if err := p.vectors.Store(ctx, doc); err != nil {
		return nil, fmt.Errorf("approve: write learned rule: %w", err)
	}

	now := time.Now()
	prop.Status = ProposalApproved
	prop.DocumentID = docID
	prop.DecidedAt = &now
	// Reversible only if the backing store can delete the written document.
	if _, ok := p.vectors.(revertableStore); !ok {
		prop.Reversible = false
	}
	return clonePro(prop), nil
}

func (p *proposalStore) Reject(ctx context.Context, id string) (*MemoryProposal, error) {
	p.mu.Lock()
	defer p.mu.Unlock()

	prop, ok := p.proposals[id]
	if !ok {
		return nil, appErrors.New(appErrors.CodeNotFound, "proposal not found", appErrors.LayerService)
	}
	if prop.Status != ProposalPending {
		return nil, appErrors.New(appErrors.CodeInvalidArgument, fmt.Sprintf("only a pending proposal can be rejected; current status %q", prop.Status), appErrors.LayerService)
	}
	now := time.Now()
	prop.Status = ProposalRejected
	prop.DecidedAt = &now
	return clonePro(prop), nil
}

func (p *proposalStore) Revert(ctx context.Context, id string) (*MemoryProposal, error) {
	p.mu.Lock()
	defer p.mu.Unlock()

	prop, ok := p.proposals[id]
	if !ok {
		return nil, appErrors.New(appErrors.CodeNotFound, "proposal not found", appErrors.LayerService)
	}
	if prop.Status != ProposalApproved {
		return nil, appErrors.New(appErrors.CodeInvalidArgument, fmt.Sprintf("only an approved proposal can be reverted; current status %q", prop.Status), appErrors.LayerService)
	}
	rev, ok := p.vectors.(revertableStore)
	if !ok || !prop.Reversible {
		return nil, appErrors.New(appErrors.CodeInvalidArgument, "backing store does not support revert", appErrors.LayerService)
	}
	if err := rev.Delete(ctx, prop.DocumentID); err != nil {
		return nil, fmt.Errorf("revert: delete learned rule: %w", err)
	}
	now := time.Now()
	prop.Status = ProposalReverted
	prop.DecidedAt = &now
	return clonePro(prop), nil
}

func (p *proposalStore) Get(ctx context.Context, id string) (*MemoryProposal, error) {
	p.mu.Lock()
	defer p.mu.Unlock()
	prop, ok := p.proposals[id]
	if !ok {
		return nil, appErrors.New(appErrors.CodeNotFound, "proposal not found", appErrors.LayerService)
	}
	return clonePro(prop), nil
}

func (p *proposalStore) List(ctx context.Context, status ProposalStatus) ([]*MemoryProposal, error) {
	p.mu.Lock()
	defer p.mu.Unlock()
	out := make([]*MemoryProposal, 0, len(p.proposals))
	for _, prop := range p.proposals {
		if status == "" || prop.Status == status {
			out = append(out, clonePro(prop))
		}
	}
	// Newest first (seq is encoded in CreatedAt order; sort by CreatedAt desc then ID).
	sort.Slice(out, func(i, j int) bool {
		if !out[i].CreatedAt.Equal(out[j].CreatedAt) {
			return out[i].CreatedAt.After(out[j].CreatedAt)
		}
		return out[i].ID > out[j].ID
	})
	return out, nil
}

func cloneScope(s map[string]string) map[string]string {
	if s == nil {
		return nil
	}
	out := make(map[string]string, len(s))
	for k, v := range s {
		out[k] = v
	}
	return out
}

func scopeEqual(a, b map[string]string) bool {
	if len(a) != len(b) {
		return false
	}
	for k, v := range a {
		if b[k] != v {
			return false
		}
	}
	return true
}

func clonePro(p *MemoryProposal) *MemoryProposal {
	cp := *p
	cp.Scope = cloneScope(p.Scope)
	if p.DecidedAt != nil {
		t := *p.DecidedAt
		cp.DecidedAt = &t
	}
	return &cp
}
