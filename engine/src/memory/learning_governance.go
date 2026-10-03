package memory

import (
	"context"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
)

// LearningGovernanceService manages the consent-based learning lifecycle.
// It wraps ProposalStore to provide service-level operations with approval routing.
type LearningGovernanceService struct {
	proposalStore ProposalStore
}

// NewLearningGovernanceService creates a new learning governance service.
func NewLearningGovernanceService(proposalStore ProposalStore) *LearningGovernanceService {
	return &LearningGovernanceService{
		proposalStore: proposalStore,
	}
}

// ProposeLearning records a learning proposal (rule/correction) as pending.
// Returns the proposal ID for subsequent approval/rejection.
func (s *LearningGovernanceService) ProposeLearning(ctx context.Context, rule string, scope map[string]string) (string, error) {
	if rule == "" {
		return "", appErrors.New(appErrors.CodeInvalidArgument, "rule cannot be empty", appErrors.LayerService)
	}
	if scope == nil {
		scope = make(map[string]string)
	}
	prop, err := s.proposalStore.Propose(ctx, rule, scope)
	if err != nil {
		return "", err
	}
	return prop.ID, nil
}

// ApproveLearning approves a pending proposal and writes the rule to the scoped memory store.
// Only proposals with status "pending" can be approved.
func (s *LearningGovernanceService) ApproveLearning(ctx context.Context, proposalID string) error {
	prop, err := s.proposalStore.Approve(ctx, proposalID)
	if err != nil {
		return err
	}
	if prop.Status != ProposalApproved {
		return appErrors.New(appErrors.CodeInternal, "proposal not approved after approval call", appErrors.LayerService)
	}
	return nil
}

// RejectLearning rejects a pending proposal. Nothing is written to the memory store.
func (s *LearningGovernanceService) RejectLearning(ctx context.Context, proposalID string) error {
	prop, err := s.proposalStore.Reject(ctx, proposalID)
	if err != nil {
		return err
	}
	if prop.Status != ProposalRejected {
		return appErrors.New(appErrors.CodeInternal, "proposal not rejected after rejection call", appErrors.LayerService)
	}
	return nil
}

// GetProposal returns a proposal by ID.
func (s *LearningGovernanceService) GetProposal(ctx context.Context, proposalID string) (*MemoryProposal, error) {
	return s.proposalStore.Get(ctx, proposalID)
}

// ListProposals returns all proposals, optionally filtered by status.
func (s *LearningGovernanceService) ListProposals(ctx context.Context, status ProposalStatus) ([]*MemoryProposal, error) {
	return s.proposalStore.List(ctx, status)
}

// RevertLearning undoes an approved proposal by removing the written document.
// Only works if Reversible is true and the backing store supports deletion.
func (s *LearningGovernanceService) RevertLearning(ctx context.Context, proposalID string) error {
	prop, err := s.proposalStore.Revert(ctx, proposalID)
	if err != nil {
		return err
	}
	if prop.Status != ProposalReverted {
		return appErrors.New(appErrors.CodeInternal, "proposal not reverted after revert call", appErrors.LayerService)
	}
	return nil
}
