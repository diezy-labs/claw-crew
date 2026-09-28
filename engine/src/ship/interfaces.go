package ship

type Ship struct {
	ID        string   `json:"id"`
	FleetID   string   `json:"fleet_id"`
	CharterID string   `json:"charter_id"`
	SquadIDs  []string `json:"squad_ids"`
}
