package fleet

import (
	"context"
	"time"
)

type Fleet struct {
	ID          string   `json:"id"`
	OwnerID     string   `json:"owner_id"`
	Name        string   `json:"name"`
	ActiveShips []string `json:"active_ships"`
}

type CreateFleetRequest struct {
	OwnerID string `json:"owner_id"`
	Name    string `json:"name"`
}

type FleetMetrics struct {
	ActiveVessels        int     `json:"activeVessels"`
	UnderwayQuests       int     `json:"underwayQuests"`
	PendingApprovals     int     `json:"pendingApprovals"`
	TotalTreasuryLedgers int     `json:"totalTreasuryLedgers"`
	TotalArtifacts       int     `json:"totalArtifacts"`
	TotalSpecialists     int     `json:"totalSpecialists"`
	TotalSquads          int     `json:"totalSquads"`
	SystemUptime         string  `json:"systemUptime"`
	GatewayLatencyMs     int     `json:"gatewayLatencyMs"`
	ActiveWorkers        int     `json:"activeWorkers"`
	MemoryUsedMB         float64 `json:"memoryUsedMB"`
}

type ExecutiveBriefing struct {
	Title            string    `json:"title"`
	Date             string    `json:"date"`
	AdmiralStatus    string    `json:"admiralStatus"`
	KeyAchievements  []string  `json:"keyAchievements"`
	ActiveAlerts     []string  `json:"activeAlerts"`
	TreasuryBurnRate string    `json:"treasuryBurnRate"`
	RecommendedFocus string    `json:"recommendedFocus"`
	GeneratedAt      time.Time `json:"generatedAt"`
}

type HarborProvider struct {
	ID          string   `json:"id"`
	Name        string   `json:"name"`
	Vendor      string   `json:"vendor"`
	Status      string   `json:"status"`
	Models      []string `json:"models"`
	LatencyMs   int      `json:"latencyMs"`
	Capabilities []string `json:"capabilities"`
}

type DiagnosticItem struct {
	ID        string `json:"id"`
	Component string `json:"component"`
	Status    string `json:"status"` // healthy | warning | error
	Latency   string `json:"latency"`
	Detail    string `json:"detail"`
}

type SnapshotItem struct {
	ID          string   `json:"id"`
	CreatedAt   string   `json:"createdAt"`
	Label       string   `json:"label"`
	SizeMB      float64  `json:"sizeMB"`
	EntityCount int      `json:"entityCount"`
	Checksum    string   `json:"checksum"`
	Includes    []string `json:"includes"`
}

type SuggestedAction struct {
	Label      string         `json:"label"`
	ActionType string         `json:"actionType"`
	Payload    map[string]any `json:"payload"`
}

type QuartermasterChatResponse struct {
	Reply                    string            `json:"reply"`
	SuggestedActions         []SuggestedAction `json:"suggestedActions,omitempty"`
	GeneratedArtifactPreview string            `json:"generatedArtifactPreview,omitempty"`
}

type Service interface {
	CreateFleet(ctx context.Context, req CreateFleetRequest) (*Fleet, error)
	GetFleet(ctx context.Context, id string) (*Fleet, error)
	GetMetrics(ctx context.Context) (*FleetMetrics, error)
	RingDeckBell(ctx context.Context) (string, error)
	GetExecutiveBriefing(ctx context.Context) (*ExecutiveBriefing, error)
	GetHarborProviders(ctx context.Context) ([]HarborProvider, error)
	GetDiagnostics(ctx context.Context) ([]DiagnosticItem, error)
	ApplyRemedy(ctx context.Context) ([]DiagnosticItem, error)
	GetSnapshots(ctx context.Context) ([]SnapshotItem, error)
	CreateSnapshot(ctx context.Context, label string) (*SnapshotItem, error)
	GetFleetPolicies(ctx context.Context) (map[string]any, error)
	GetCollection(ctx context.Context, name string) ([]byte, error)
	SaveCollection(ctx context.Context, name string, rawJSON []byte) error
	ChatQuartermaster(ctx context.Context, prompt string) (*QuartermasterChatResponse, error)
	GetSeedData(ctx context.Context) (map[string]any, error)
	// SetObjectiveProposer injects the objective-branch dependency after
	// construction (orchestrator imports fleet, so it is wired post-hoc).
	SetObjectiveProposer(p ObjectiveProposer)
}
