package policy

import (
	"context"
	"sync"
)

type Service interface {
	GetFleetCode(ctx context.Context, fleetID string) (*FleetCode, error)
	UpdateFleetCode(ctx context.Context, fleetID string, code *FleetCode) error
}

type policyService struct {
	mu         sync.RWMutex
	fleetCodes map[string]*FleetCode
}

func NewService() Service {
	return &policyService{
		fleetCodes: make(map[string]*FleetCode),
	}
}

func (s *policyService) GetFleetCode(ctx context.Context, fleetID string) (*FleetCode, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()

	code, exists := s.fleetCodes[fleetID]
	if !exists {
		// Return default policy if none set
		return &FleetCode{
			RequireApprovalForWrite: true,
			MaxWeeklySpendUSD:       0.0, // 0 means default/unlimited or restricted based on env
			CrossShipMemorySharing:  false,
		}, nil
	}
	return code, nil
}

func (s *policyService) UpdateFleetCode(ctx context.Context, fleetID string, code *FleetCode) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.fleetCodes[fleetID] = code
	return nil
}
