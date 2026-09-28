package fleet

import (
	"context"
	"errors"
	"sync"
)

type fleetService struct {
	mu     sync.RWMutex
	fleets map[string]*Fleet
}

func NewService() Service {
	return &fleetService{
		fleets: make(map[string]*Fleet),
	}
}

func (s *fleetService) CreateFleet(ctx context.Context, req CreateFleetRequest) (*Fleet, error) {
	if req.OwnerID == "" || req.Name == "" {
		return nil, errors.New("invalid fleet request")
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	id := "flt_" + req.Name // simplified ID generation
	fleet := &Fleet{
		ID:          id,
		OwnerID:     req.OwnerID,
		Name:        req.Name,
		ActiveShips: []string{},
	}

	s.fleets[id] = fleet
	return fleet, nil
}

func (s *fleetService) GetFleet(ctx context.Context, id string) (*Fleet, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()

	fleet, exists := s.fleets[id]
	if !exists {
		return nil, errors.New("fleet not found")
	}

	return fleet, nil
}
