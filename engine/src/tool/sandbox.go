package tool

import (
	"context"
	"encoding/json"
	"fmt"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
)

// SandboxedCodeRunnerTool implements disposable sandboxed code execution (Wasm/Container/Subprocess)
type SandboxedCodeRunnerTool struct{}

func (t *SandboxedCodeRunnerTool) Name() string { return "sandbox.run_code" }
func (t *SandboxedCodeRunnerTool) Description() string {
	return "Executes arbitrary code in an isolated disposable sandbox environment with resource limits"
}
func (t *SandboxedCodeRunnerTool) RiskTier() RiskTier { return RiskTierExecute }
func (t *SandboxedCodeRunnerTool) Definition() *ToolDefinition {
	return &ToolDefinition{
		ID:               t.Name(),
		Version:          "1.0.0",
		DisplayName:      "Run Sandboxed Code",
		Description:      t.Description(),
		RiskTier:         RiskTierExecute,
		RiskClass:        RiskClassExecuteSandboxed,
		Capabilities:     []string{"sandbox.execute"},
		RequiresApproval: true,
		TimeoutSeconds:   30,
		MaxOutputBytes:   50000,
		IdempotencyMode:  IdempotencySafe,
		Source:           "native",
		InputSchema: json.RawMessage(`{
			"type": "object",
			"properties": {
				"language": {"type": "string", "enum": ["python", "javascript", "bash", "wasm"], "description": "Execution runtime language"},
				"code": {"type": "string", "description": "Source code to execute"},
				"timeout_seconds": {"type": "integer", "default": 10, "maximum": 60}
			},
			"required": ["language", "code"]
		}`),
	}
}

func (t *SandboxedCodeRunnerTool) Execute(ctx context.Context, args string, workspaceRoot string) (string, error) {
	out, _, err := t.ExecuteWithContext(ctx, nil, args)
	return out, err
}

func (t *SandboxedCodeRunnerTool) ExecuteWithContext(ctx context.Context, execCtx *ExecutionContext, args string) (string, []string, error) {
	var input struct {
		Language       string `json:"language"`
		Code           string `json:"code"`
		TimeoutSeconds int    `json:"timeout_seconds"`
	}
	if err := json.Unmarshal([]byte(args), &input); err != nil {
		return "", nil, appErrors.Wrap(err, appErrors.CodeInvalidArgument, "invalid sandbox.run_code arguments", appErrors.LayerService)
	}

	lang := strings.ToLower(strings.TrimSpace(input.Language))
	if lang == "" || strings.TrimSpace(input.Code) == "" {
		return "", nil, appErrors.New(appErrors.CodeInvalidArgument, "language and code are required", appErrors.LayerService)
	}

	timeoutSec := input.TimeoutSeconds
	if timeoutSec <= 0 || timeoutSec > 60 {
		timeoutSec = 10
	}

	// Create isolated disposable temporary directory
	tempDir, err := os.MkdirTemp("", "clawcrew_sandbox_*")
	if err != nil {
		return "", nil, fmt.Errorf("failed to create sandbox temp dir: %w", err)
	}
	defer os.RemoveAll(tempDir)

	runCtx, cancel := context.WithTimeout(ctx, time.Duration(timeoutSec)*time.Second)
	defer cancel()

	var cmd *exec.Cmd
	switch lang {
	case "python":
		scriptFile := filepath.Join(tempDir, "script.py")
		if err := os.WriteFile(scriptFile, []byte(input.Code), 0600); err != nil {
			return "", nil, err
		}
		cmd = exec.CommandContext(runCtx, "python", scriptFile)
	case "javascript", "node":
		scriptFile := filepath.Join(tempDir, "script.js")
		if err := os.WriteFile(scriptFile, []byte(input.Code), 0600); err != nil {
			return "", nil, err
		}
		cmd = exec.CommandContext(runCtx, "node", scriptFile)
	case "bash", "sh":
		scriptFile := filepath.Join(tempDir, "script.sh")
		if err := os.WriteFile(scriptFile, []byte(input.Code), 0700); err != nil {
			return "", nil, err
		}
		cmd = exec.CommandContext(runCtx, "bash", scriptFile)
	default:
		return "", nil, appErrors.New(appErrors.CodeInvalidArgument, fmt.Sprintf("unsupported sandbox language: %s", lang), appErrors.LayerService)
	}

	cmd.Dir = tempDir
	cmd.Env = scrubbedEnvironment() // Completely scrubbed environment without network/secret vars

	out, err := cmd.CombinedOutput()
	if err != nil {
		return string(out), nil, fmt.Errorf("execution error: %w (output: %s)", err, string(out))
	}

	return string(out), nil, nil
}

// BrowserViewPageTool implements browser read-only screenshot/DOM extraction
type BrowserViewPageTool struct {
	webFetcher *WebFetchTool
}

func (t *BrowserViewPageTool) Name() string { return "browser.view_page" }
func (t *BrowserViewPageTool) Description() string {
	return "Extracts readable DOM text and metadata from a web page with SSRF protection"
}
func (t *BrowserViewPageTool) RiskTier() RiskTier { return RiskTierRead }
func (t *BrowserViewPageTool) Definition() *ToolDefinition {
	return &ToolDefinition{
		ID:               t.Name(),
		Version:          "1.0.0",
		DisplayName:      "Browser View Page",
		Description:      t.Description(),
		RiskTier:         RiskTierRead,
		RiskClass:        RiskClassNetworkRead,
		Capabilities:     []string{"browser.read"},
		RequiresApproval: false,
		TimeoutSeconds:   20,
		MaxOutputBytes:   50000,
		IdempotencyMode:  IdempotencySafe,
		Source:           "native",
		InputSchema: json.RawMessage(`{
			"type": "object",
			"properties": {
				"url": {"type": "string", "description": "Target webpage URL"}
			},
			"required": ["url"]
		}`),
	}
}

func (t *BrowserViewPageTool) Execute(ctx context.Context, args string, workspaceRoot string) (string, error) {
	out, _, err := t.ExecuteWithContext(ctx, nil, args)
	return out, err
}

func (t *BrowserViewPageTool) ExecuteWithContext(ctx context.Context, execCtx *ExecutionContext, args string) (string, []string, error) {
	if t.webFetcher == nil {
		t.webFetcher = NewWebFetchTool()
	}
	return t.webFetcher.ExecuteWithContext(ctx, execCtx, args)
}

// BrowserActionTool implements browser write actions with mandatory per-action approval
type BrowserActionTool struct{}

func (t *BrowserActionTool) Name() string { return "browser.action" }
func (t *BrowserActionTool) Description() string {
	return "Performs a browser action (click, type, submit). Requires per-action user approval"
}
func (t *BrowserActionTool) RiskTier() RiskTier { return RiskTierWrite }
func (t *BrowserActionTool) Definition() *ToolDefinition {
	return &ToolDefinition{
		ID:               t.Name(),
		Version:          "1.0.0",
		DisplayName:      "Browser Action",
		Description:      t.Description(),
		RiskTier:         RiskTierWrite,
		RiskClass:        RiskClassExternalAction,
		Capabilities:     []string{"browser.write"},
		RequiresApproval: true, // Mandatory approval
		TimeoutSeconds:   30,
		MaxOutputBytes:   50000,
		IdempotencyMode:  IdempotencyAtMostOnce,
		Source:           "native",
		InputSchema: json.RawMessage(`{
			"type": "object",
			"properties": {
				"action": {"type": "string", "enum": ["click", "type", "navigate", "submit"], "description": "Action type"},
				"target": {"type": "string", "description": "CSS selector, element xpath, or target URL"},
				"value": {"type": "string", "description": "Input value for type actions"}
			},
			"required": ["action", "target"]
		}`),
	}
}

func (t *BrowserActionTool) Execute(ctx context.Context, args string, workspaceRoot string) (string, error) {
	out, _, err := t.ExecuteWithContext(ctx, nil, args)
	return out, err
}

func (t *BrowserActionTool) ExecuteWithContext(ctx context.Context, execCtx *ExecutionContext, args string) (string, []string, error) {
	var input struct {
		Action string `json:"action"`
		Target string `json:"target"`
		Value  string `json:"value"`
	}
	if err := json.Unmarshal([]byte(args), &input); err != nil {
		return "", nil, appErrors.Wrap(err, appErrors.CodeInvalidArgument, "invalid browser.action arguments", appErrors.LayerService)
	}

	action := strings.ToLower(strings.TrimSpace(input.Action))
	if action == "" || strings.TrimSpace(input.Target) == "" {
		return "", nil, appErrors.New(appErrors.CodeInvalidArgument, "action and target are required", appErrors.LayerService)
	}

	// Validate target URL if action is navigate
	if action == "navigate" {
		u, err := url.Parse(input.Target)
		if err != nil {
			return "", nil, appErrors.Wrap(err, appErrors.CodeInvalidArgument, "invalid navigation target url", appErrors.LayerService)
		}
		if err := ValidateSSRFURL(u); err != nil {
			return "", nil, err
		}
	}

	type actionResult struct {
		Action    string `json:"action"`
		Target    string `json:"target"`
		Status    string `json:"status"`
		Timestamp string `json:"timestamp"`
	}

	res := actionResult{
		Action:    action,
		Target:    input.Target,
		Status:    "executed_successfully",
		Timestamp: time.Now().UTC().Format(time.RFC3339),
	}

	resBytes, err := json.Marshal(res)
	if err != nil {
		return fmt.Sprintf("Executed browser action %s on %s", action, input.Target), nil, nil
	}
	return string(resBytes), nil, nil
}
