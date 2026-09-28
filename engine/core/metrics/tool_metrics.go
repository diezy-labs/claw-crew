package metrics

import "github.com/prometheus/client_golang/prometheus"

var (
	// ToolRequestsTotal tracks all tool execution requests categorized by tool, tier, and status
	ToolRequestsTotal = prometheus.NewCounterVec(prometheus.CounterOpts{
		Namespace: "clawcrew",
		Subsystem: "tool",
		Name:      "requests_total",
		Help:      "Total tool requests categorized by tool name, risk tier, and status.",
	}, []string{"tool_name", "risk_tier", "status"})

	// ToolExecutionDuration measures duration of tool executions in seconds
	ToolExecutionDuration = prometheus.NewHistogramVec(prometheus.HistogramOpts{
		Namespace: "clawcrew",
		Subsystem: "tool",
		Name:      "execution_duration_seconds",
		Help:      "Execution duration of tools in seconds.",
		Buckets:   []float64{0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 15.0, 30.0},
	}, []string{"tool_name", "risk_tier"})

	// ToolPolicyDenialsTotal counts invocations blocked by policy engine
	ToolPolicyDenialsTotal = prometheus.NewCounterVec(prometheus.CounterOpts{
		Namespace: "clawcrew",
		Subsystem: "tool",
		Name:      "policy_denials_total",
		Help:      "Count of tool invocations blocked by policy engine.",
	}, []string{"tool_name", "reason"})

	// ToolApprovalsTotal counts approval gate requests by final status
	ToolApprovalsTotal = prometheus.NewCounterVec(prometheus.CounterOpts{
		Namespace: "clawcrew",
		Subsystem: "tool",
		Name:      "approvals_total",
		Help:      "Total approval requests categorized by status (approved, denied, expired, timed_out).",
	}, []string{"tool_name", "status"})
)
