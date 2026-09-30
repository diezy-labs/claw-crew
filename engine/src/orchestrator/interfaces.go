package orchestrator

import "context"

type Service interface {
	ProcessObjective(ctx context.Context, req ObjectiveRequest) (*ObjectiveResponse, error)
	CoordinateFleet(ctx context.Context, fleetID string) error
}
type ObjectiveRequest struct {
	FleetID   string
	Objective string
}
type ObjectiveResponse struct {
	Status string
}
