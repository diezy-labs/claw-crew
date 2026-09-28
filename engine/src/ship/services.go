package ship

import (
	"context"
	"errors"
	"sync"
)

type Service interface {
	CreateShip(ctx context.Context, fleetID string, charterID string) (*Ship, error)
	GetShip(ctx context.Context, id string) (*Ship, error)
}

type shipService struct {
	mu    sync.RWMutex
	ships map[string]*Ship
}

func NewService() Service {
	return &shipService{
		ships: make(map[string]*Ship),
	}
}

func (s *shipService) CreateShip(ctx context.Context, fleetID string, charterID string) (*Ship, error) {
	if fleetID == "" {
		return nil, errors.New("invalid ship request")
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	id := "shp_" + fleetID // simplified ID generation
	ship := &Ship{
		ID:        id,
		FleetID:   fleetID,
		CharterID: charterID,
		SquadIDs:  []string{},
	}

	s.ships[id] = ship
	return ship, nil
}

func (s *shipService) GetShip(ctx context.Context, id string) (*Ship, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()

	ship, exists := s.ships[id]
	if !exists {
		return nil, errors.New("ship not found")
	}

	return ship, nil
}
