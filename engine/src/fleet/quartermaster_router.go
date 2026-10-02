package fleet

import (
	"regexp"
	"strings"

	"github.com/diezy-labs/claw-crew/engine/core/logger"
)

// QuartermasterIntent is the Layer-1 router classification (docs/finalize/01).
// SSOT: defined only here in Go; clients consume it, never guess it.
type QuartermasterIntent string

const (
	IntentChat       QuartermasterIntent = "chat"        // general question/conversation → answer directly
	IntentEngineRoom QuartermasterIntent = "engine_room" // machine status: RAM/CPU/health/budget → telemetry
	IntentReport     QuartermasterIntent = "report"      // fleet/ship status summary → report
	IntentObjective  QuartermasterIntent = "objective"   // work/quest request → FleetOrderProposal (gated)
)

// IntentClassification carries the routing decision plus the evidence needed to
// instrument it and to decide whether to ask a clarifying question (low confidence).
type IntentClassification struct {
	Intent     QuartermasterIntent `json:"intent"`
	Confidence float64             `json:"confidence"` // 0..1
	Reason     string              `json:"reason"`
}

// Rule-first signals. Cheap + deterministic (no LLM call per message), which fits
// the "don't let tokens make you down" vision. Ambiguity falls back to chat.
var (
	reObjective  = regexp.MustCompile(`(?i)\b(quest|misi|mission|siapkan|prepare|build|implement|deploy|rilis|release|kerjakan|execute|buatkan|rencana(kan)?|plan)\b`)
	reReport     = regexp.MustCompile(`(?i)\b(status|laporan|report|ringkas|summary|summarize|briefing|progress|semua ship|all ships|fleet report)\b`)
	reEngineRoom = regexp.MustCompile(`(?i)\b(ram|cpu|memory|memori|engine room|engine-room|health|uptime|budget|biaya|cost|resource|diagnos|provider health)\b`)
)

// ClassifyIntent runs the Layer-1 router. Order matters: objective (most specific
// action) → engine_room → report → chat (default). Returns confidence + reason.
func ClassifyIntent(prompt string) IntentClassification {
	p := strings.TrimSpace(prompt)
	switch {
	case reObjective.MatchString(p):
		return IntentClassification{IntentObjective, 0.8, "matched objective/quest verb"}
	case reEngineRoom.MatchString(p):
		return IntentClassification{IntentEngineRoom, 0.8, "matched engine-room/resource term"}
	case reReport.MatchString(p):
		return IntentClassification{IntentReport, 0.75, "matched report/status term"}
	default:
		return IntentClassification{IntentChat, 0.5, "no specific signal; default chat"}
	}
}

// logIntent records every routing decision (finalize/01 risk mitigation: a
// mis-route is a silent failure unless it is logged).
func logIntent(prompt string, c IntentClassification) {
	logger.Get().Info("quartermaster.route",
		"intent", string(c.Intent),
		"confidence", c.Confidence,
		"reason", c.Reason,
		"prompt_len", len(prompt),
	)
}
