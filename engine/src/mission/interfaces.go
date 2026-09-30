package mission

type Quest struct {
	ID           string `json:"id"`
	Status       string `json:"status"`
	TargetShipID string `json:"target_ship_id"`
	WorkflowID   string `json:"workflow_id"`
}
