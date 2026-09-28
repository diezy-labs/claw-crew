package tool

import (
	"context"
	"fmt"
	"strings"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
)

// defaultPolicyEngine evaluates policy rules on tool execution requests
type defaultPolicyEngine struct{}

// NewPolicyEngine creates a new PolicyEngine instance
func NewPolicyEngine() PolicyEngine {
	return &defaultPolicyEngine{}
}

func (p *defaultPolicyEngine) Evaluate(ctx context.Context, execCtx *ExecutionContext, toolDef *ToolDefinition, args string) (PolicyVerdict, error) {
	if toolDef == nil {
		return VerdictDeny, appErrors.New(appErrors.CodeInvalidArgument, "missing tool definition for policy evaluation", appErrors.LayerService)
	}

	// 1. Mandatory ActorID check
	if execCtx == nil || strings.TrimSpace(execCtx.ActorID) == "" {
		return VerdictDeny, appErrors.New(appErrors.CodePermissionDenied, "missing actor_id: tool execution requires an authenticated actor context", appErrors.LayerService)
	}

	// 2. Capability verification
	if len(toolDef.Capabilities) > 0 {
		if !hasRequiredCapabilities(execCtx.Capabilities, toolDef.Capabilities) {
			return VerdictDeny, appErrors.New(
				appErrors.CodePermissionDenied,
				fmt.Sprintf("actor %s lacks required capabilities %v for tool %s", execCtx.ActorID, toolDef.Capabilities, toolDef.ID),
				appErrors.LayerService,
			)
		}
	}

	// 3. Data Classification constraints (Prevent prompt injection / data exfiltration)
	if isHighClassification(execCtx.DataClassification) {
		if toolDef.RiskClass == RiskClassNetworkRead || toolDef.RiskClass == RiskClassExternalAction {
			return VerdictDeny, appErrors.New(
				appErrors.CodePermissionDenied,
				fmt.Sprintf("network egress/external actions blocked for classified data context: %s", execCtx.DataClassification),
				appErrors.LayerService,
			)
		}
	}

	// 4. Approval requirements based on RiskTier, RiskClass, and definition flags
	if toolDef.RequiresApproval || toolDef.RiskTier == RiskTierWrite || toolDef.RiskTier == RiskTierExecute ||
		toolDef.RiskClass == RiskClassWriteWorkspace || toolDef.RiskClass == RiskClassExecutePrivileged ||
		toolDef.RiskClass == RiskClassExternalAction {
		return VerdictRequireApproval, nil
	}

	return VerdictAllow, nil
}

// hasRequiredCapabilities checks if all required capabilities are present in granted capabilities
func hasRequiredCapabilities(granted []string, required []string) bool {
	// If caller did not restrict capabilities (empty slice), default allow in unconstrained environment,
	// UNLESS capabilities were explicitly defined as restricted.
	// But in strict mode: if granted is empty and tool requires capabilities,
	// let's check if wildcard is present.
	if len(granted) == 0 {
		return true // Inherits full ambient capabilities if none specified
	}

	grantMap := make(map[string]bool, len(granted))
	for _, g := range granted {
		if g == "*" {
			return true
		}
		grantMap[strings.ToLower(g)] = true
	}

	for _, req := range required {
		reqLower := strings.ToLower(req)
		if !grantMap[reqLower] {
			// Check wildcard prefix e.g. "workspace.*"
			parts := strings.Split(reqLower, ".")
			if len(parts) > 1 && grantMap[parts[0]+".*"] {
				continue
			}
			return false
		}
	}

	return true
}

func isHighClassification(classification string) bool {
	c := strings.ToLower(strings.TrimSpace(classification))
	return c == "confidential" || c == "secret" || c == "restricted" || c == "top_secret"
}
