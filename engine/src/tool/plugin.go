package tool

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"os/exec"
	"strings"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
)

// PluginTool adapts an external CLI executable or script as a first-class Tool (TASK-7.9)
type PluginTool struct {
	toolName       string
	toolDesc       string
	tier           RiskTier
	executablePath string
}

// NewPluginTool creates an external executable tool plugin
func NewPluginTool(name, description string, tier RiskTier, executablePath string) *PluginTool {
	return &PluginTool{
		toolName:       name,
		toolDesc:       description,
		tier:           tier,
		executablePath: executablePath,
	}
}

func (p *PluginTool) Name() string {
	return p.toolName
}

func (p *PluginTool) Description() string {
	return p.toolDesc
}

func (p *PluginTool) RiskTier() RiskTier {
	return p.tier
}

func (p *PluginTool) Definition() *ToolDefinition {
	reqApproval := p.tier == RiskTierWrite || p.tier == RiskTierExecute
	return &ToolDefinition{
		ID:               p.toolName,
		Version:          "1.0.0",
		DisplayName:      p.toolName,
		Description:      p.toolDesc,
		RiskTier:         p.tier,
		RiskClass:        RiskClassExternalAction,
		Capabilities:     []string{"plugin.execute"},
		RequiresApproval: reqApproval,
		TimeoutSeconds:   30,
		MaxOutputBytes:   50000,
		IdempotencyMode:  IdempotencySafe,
		Source:           "plugin",
		InputSchema:      json.RawMessage(`{"type":"object"}`),
	}
}

func (p *PluginTool) Execute(ctx context.Context, args string, workspaceRoot string) (string, error) {
	cmd := exec.CommandContext(ctx, p.executablePath)
	if workspaceRoot != "" {
		cmd.Dir = workspaceRoot
	}

	cmd.Stdin = strings.NewReader(args)
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return "", appErrors.New(
			appErrors.CodeToolFailed,
			fmt.Sprintf("plugin %s failed: %v | stderr: %s", p.toolName, err, stderr.String()),
			appErrors.LayerExternal,
		)
	}

	return stdout.String(), nil
}

func (p *PluginTool) ExecuteWithContext(ctx context.Context, execCtx *ExecutionContext, args string) (string, []string, error) {
	root := getWorkspaceRoot(execCtx)
	out, err := p.Execute(ctx, args, root)
	return out, nil, err
}
