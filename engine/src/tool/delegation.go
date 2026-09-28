package tool

import (
	"context"
	"encoding/json"
	"fmt"
	"sort"
	"strings"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/core/id"
)

// FilterToolsForIntent performs intent-aware filtering to conserve model prompt tokens (FR-01, Phase T4)
func FilterToolsForIntent(intent string, allTools []Tool, maxCount int) []Tool {
	if len(allTools) <= maxCount || maxCount <= 0 {
		return allTools
	}

	intentLower := strings.ToLower(intent)
	words := strings.Fields(intentLower)

	type scoredTool struct {
		tool  Tool
		score int
	}

	var scored []scoredTool
	for _, t := range allTools {
		def := t.Definition()
		score := 0
		if def == nil {
			scored = append(scored, scoredTool{tool: t, score: 0})
			continue
		}

		nameLower := strings.ToLower(def.ID)
		descLower := strings.ToLower(def.Description)

		for _, w := range words {
			if len(w) <= 2 {
				continue
			}
			if strings.Contains(nameLower, w) {
				score += 5
			}
			if strings.Contains(descLower, w) {
				score += 2
			}
			for _, cap := range def.Capabilities {
				if strings.Contains(strings.ToLower(cap), w) {
					score += 3
				}
			}
		}

		// Baseline bonus for core read tools
		if def.ID == "workspace.read_file" || def.ID == "workspace.list_files" {
			score += 1
		}

		scored = append(scored, scoredTool{tool: t, score: score})
	}

	sort.SliceStable(scored, func(i, j int) bool {
		return scored[i].score > scored[j].score
	})

	result := make([]Tool, 0, maxCount)
	for i := 0; i < maxCount && i < len(scored); i++ {
		result = append(result, scored[i].tool)
	}

	return result
}

// DelegateTaskTool implements agent-as-a-tool delegation with inherited context and policy ceiling
type DelegateTaskTool struct{}

func (t *DelegateTaskTool) Name() string { return "delegate_task" }
func (t *DelegateTaskTool) Description() string {
	return "Delegates a specific sub-task to a specialized sub-agent with policy ceiling enforcement"
}
func (t *DelegateTaskTool) RiskTier() RiskTier { return RiskTierExecute }
func (t *DelegateTaskTool) Definition() *ToolDefinition {
	return &ToolDefinition{
		ID:               t.Name(),
		Version:          "1.0.0",
		DisplayName:      "Delegate Task to Subagent",
		Description:      t.Description(),
		RiskTier:         RiskTierExecute,
		RiskClass:        RiskClassCompute,
		Capabilities:     []string{"agent.delegate"},
		RequiresApproval: false,
		TimeoutSeconds:   60,
		MaxOutputBytes:   50000,
		IdempotencyMode:  IdempotencySafe,
		Source:           "native",
		InputSchema: json.RawMessage(`{
			"type": "object",
			"properties": {
				"subagent_role": {"type": "string", "description": "Role of the subagent (e.g. Researcher, Code Reviewer)"},
				"task_prompt": {"type": "string", "description": "Specific task instructions for the subagent"},
				"requested_capabilities": {"type": "array", "items": {"type": "string"}, "description": "Subset of capabilities to grant"}
			},
			"required": ["subagent_role", "task_prompt"]
		}`),
	}
}

func (t *DelegateTaskTool) Execute(ctx context.Context, args string, workspaceRoot string) (string, error) {
	out, _, err := t.ExecuteWithContext(ctx, nil, args)
	return out, err
}

func (t *DelegateTaskTool) ExecuteWithContext(ctx context.Context, execCtx *ExecutionContext, args string) (string, []string, error) {
	var input struct {
		SubagentRole          string   `json:"subagent_role"`
		TaskPrompt            string   `json:"task_prompt"`
		RequestedCapabilities []string `json:"requested_capabilities"`
	}
	if err := json.Unmarshal([]byte(args), &input); err != nil {
		return "", nil, appErrors.Wrap(err, appErrors.CodeInvalidArgument, "invalid delegate_task arguments", appErrors.LayerService)
	}

	if strings.TrimSpace(input.SubagentRole) == "" || strings.TrimSpace(input.TaskPrompt) == "" {
		return "", nil, appErrors.New(appErrors.CodeInvalidArgument, "subagent_role and task_prompt are required", appErrors.LayerService)
	}

	// Policy Ceiling Enforcement (Security Invariant):
	// Child subagent cannot escalate permissions beyond parent crew permissions.
	if execCtx != nil && len(execCtx.Capabilities) > 0 {
		parentCapMap := make(map[string]bool)
		parentHasWildcard := false
		for _, c := range execCtx.Capabilities {
			if c == "*" {
				parentHasWildcard = true
			}
			parentCapMap[strings.ToLower(c)] = true
		}

		if !parentHasWildcard {
			for _, req := range input.RequestedCapabilities {
				reqLower := strings.ToLower(req)
				if !parentCapMap[reqLower] {
					return "", nil, appErrors.New(
						appErrors.CodePermissionDenied,
						fmt.Sprintf("privilege escalation blocked: child cannot request capability %s not possessed by parent", req),
						appErrors.LayerService,
					)
				}
			}
		}
	}

	subagentID := fmt.Sprintf("subagent_%s_%s", strings.ToLower(strings.ReplaceAll(input.SubagentRole, " ", "_")), id.Generate("sub_"))

	type delegationResult struct {
		SubagentID          string   `json:"subagent_id"`
		SubagentRole        string   `json:"subagent_role"`
		TaskPrompt          string   `json:"task_prompt"`
		GrantedCapabilities []string `json:"granted_capabilities"`
		Status              string   `json:"status"`
	}

	granted := input.RequestedCapabilities
	if len(granted) == 0 && execCtx != nil {
		granted = execCtx.Capabilities
	}

	res := delegationResult{
		SubagentID:          subagentID,
		SubagentRole:        input.SubagentRole,
		TaskPrompt:          input.TaskPrompt,
		GrantedCapabilities: granted,
		Status:              "delegated",
	}

	resBytes, err := json.Marshal(res)
	if err != nil {
		return fmt.Sprintf("Delegated to %s (%s)", input.SubagentRole, subagentID), nil, nil
	}
	return string(resBytes), nil, nil
}
