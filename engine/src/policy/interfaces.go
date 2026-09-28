package policy

type RiskClass string

const (
	RiskClassReadOnly    RiskClass = "read_only"
	RiskClassDraft       RiskClass = "draft"
	RiskClassWrite       RiskClass = "write"
	RiskClassSensitive   RiskClass = "sensitive"
	RiskClassDestructive RiskClass = "destructive"
)

type FleetCode struct {
	RequireApprovalForWrite bool    `json:"require_approval_for_write"`
	MaxWeeklySpendUSD       float64 `json:"max_weekly_spend_usd"`
	CrossShipMemorySharing  bool    `json:"cross_ship_memory_sharing"`
}
