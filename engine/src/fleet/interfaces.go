package fleet

import "context"

type Fleet struct {
	ID          string   `json:"id"`
	OwnerID     string   `json:"owner_id"`
	Name        string   `json:"name"`
	ActiveShips []string `json:"active_ships"`
}

type Service interface {
	CreateFleet(ctx context.Context, req CreateFleetRequest) (*Fleet, error)
	GetFleet(ctx context.Context, id string) (*Fleet, error)
}

type CreateFleetRequest struct {
	OwnerID string
	Name    string
}
