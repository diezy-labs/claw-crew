package tool

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
)

// RunLinterTool implements code.run_linter with environment scrubbing
type RunLinterTool struct{}

func (t *RunLinterTool) Name() string { return "code.run_linter" }
func (t *RunLinterTool) Description() string {
	return "Runs a code linter in an isolated, environment-scrubbed workspace process"
}
func (t *RunLinterTool) RiskTier() RiskTier { return RiskTierRead }
func (t *RunLinterTool) Definition() *ToolDefinition {
	return &ToolDefinition{
		ID:               t.Name(),
		Version:          "1.0.0",
		DisplayName:      "Run Code Linter",
		Description:      t.Description(),
		RiskTier:         RiskTierRead,
		RiskClass:        RiskClassCompute,
		Capabilities:     []string{"code.lint"},
		RequiresApproval: false,
		TimeoutSeconds:   60,
		MaxOutputBytes:   50000,
		IdempotencyMode:  IdempotencySafe,
		Source:           "native",
		InputSchema: json.RawMessage(`{
			"type": "object",
			"properties": {
				"linter": {"type": "string", "description": "Linter executable (e.g. golangci-lint, gofmt, cargo clippy)"},
				"args": {"type": "array", "items": {"type": "string"}},
				"path": {"type": "string", "description": "Working directory relative to workspace root"}
			},
			"required": ["linter"]
		}`),
	}
}

func (t *RunLinterTool) Execute(ctx context.Context, args string, workspaceRoot string) (string, error) {
	out, _, err := t.ExecuteWithContext(ctx, nil, args)
	return out, err
}

func (t *RunLinterTool) ExecuteWithContext(ctx context.Context, execCtx *ExecutionContext, args string) (string, []string, error) {
	// Fallback: if gateway available, route through gRPC; else exec.Command
	if execCtx != nil && execCtx.Gateway != nil {
		resp, err := execCtx.Gateway.ExecuteNativeTool(ctx, t.Name(), args)
		if err != nil {
			return "", nil, err
		}
		var res struct {
			Success bool   `json:"success"`
			Output  string `json:"output"`
			Error   string `json:"error"`
		}
		if err := json.Unmarshal([]byte(resp), &res); err != nil {
			return "", nil, fmt.Errorf("invalid gateway response: %w", err)
		}
		if !res.Success {
			return "", nil, fmt.Errorf("gateway execution failed: %s", res.Error)
		}
		return res.Output, nil, nil
	}

	var input struct {
		Linter string   `json:"linter"`
		Args   []string `json:"args"`
		Path   string   `json:"path"`
	}
	if err := json.Unmarshal([]byte(args), &input); err != nil {
		return "", nil, appErrors.Wrap(err, appErrors.CodeInvalidArgument, "invalid run_linter arguments", appErrors.LayerService)
	}

	linterName := strings.TrimSpace(input.Linter)
	if linterName == "" {
		return "", nil, appErrors.New(appErrors.CodeInvalidArgument, "linter name required", appErrors.LayerService)
	}

	// Validate linter against allowed list
	allowedLinters := map[string]bool{
		"golangci-lint": true,
		"gofmt":         true,
		"go":            true,
		"cargo":         true,
		"clippy":        true,
		"eslint":        true,
		"flake8":        true,
		"rubocop":       true,
		"prettier":      true,
		"rustfmt":       true,
	}

	binBase := filepath.Base(linterName)
	binBase = strings.TrimSuffix(binBase, ".exe")
	if !allowedLinters[binBase] {
		return "", nil, appErrors.New(appErrors.CodePermissionDenied, fmt.Sprintf("unsupported or unauthorized linter binary: %s", linterName), appErrors.LayerService)
	}

	workspaceRoot := getWorkspaceRoot(execCtx)
	targetDir := workspaceRoot
	if input.Path != "" {
		safeDir, err := ValidateSandboxPath(workspaceRoot, input.Path)
		if err != nil {
			return "", nil, err
		}
		targetDir = safeDir
	}

	cmdCtx, cancel := context.WithTimeout(ctx, 60*time.Second)
	defer cancel()

	cmd := exec.CommandContext(cmdCtx, linterName, input.Args...)
	cmd.Dir = targetDir
	cmd.Env = scrubbedEnvironment()

	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	err := cmd.Run()
	output := stdout.String()
	if stderr.Len() > 0 {
		if output != "" {
			output += "\n--- STDERR ---\n"
		}
		output += stderr.String()
	}

	if err != nil {
		// Linters return non-zero exit code on lint issues, which is informative rather than fatal tool failure
		if output != "" {
			return fmt.Sprintf("Linter issues found:\n%s", output), nil, nil
		}
		return "", nil, fmt.Errorf("linter %s failed: %w", linterName, err)
	}

	if strings.TrimSpace(output) == "" {
		return "No lint issues found.", nil, nil
	}
	return output, nil, nil
}

// RunTestsTool implements code.run_tests with process sandboxing and artifact spillover
type RunTestsTool struct{}

func (t *RunTestsTool) Name() string { return "code.run_tests" }
func (t *RunTestsTool) Description() string {
	return "Executes unit tests in an isolated, sandboxed process with artifact spillover for large outputs"
}
func (t *RunTestsTool) RiskTier() RiskTier { return RiskTierExecute }
func (t *RunTestsTool) Definition() *ToolDefinition {
	return &ToolDefinition{
		ID:               t.Name(),
		Version:          "1.0.0",
		DisplayName:      "Run Unit Tests",
		Description:      t.Description(),
		RiskTier:         RiskTierExecute,
		RiskClass:        RiskClassExecuteSandboxed,
		Capabilities:     []string{"code.test"},
		RequiresApproval: false,
		TimeoutSeconds:   120,
		MaxOutputBytes:   50000,
		IdempotencyMode:  IdempotencySafe,
		Source:           "native",
		InputSchema: json.RawMessage(`{
			"type": "object",
			"properties": {
				"command": {"type": "string", "description": "Test command (e.g. go, cargo, npm, pytest)"},
				"args": {"type": "array", "items": {"type": "string"}},
				"path": {"type": "string", "description": "Working directory relative to workspace root"},
				"timeout_seconds": {"type": "integer", "default": 60, "maximum": 300}
			},
			"required": ["command"]
		}`),
	}
}

func (t *RunTestsTool) Execute(ctx context.Context, args string, workspaceRoot string) (string, error) {
	out, _, err := t.ExecuteWithContext(ctx, nil, args)
	return out, err
}

func (t *RunTestsTool) ExecuteWithContext(ctx context.Context, execCtx *ExecutionContext, args string) (string, []string, error) {
	// Fallback: if gateway available, route through gRPC; else exec.Command
	if execCtx != nil && execCtx.Gateway != nil {
		resp, err := execCtx.Gateway.ExecuteNativeTool(ctx, t.Name(), args)
		if err != nil {
			return "", nil, err
		}
		var res struct {
			Success bool   `json:"success"`
			Output  string `json:"output"`
			Error   string `json:"error"`
		}
		if err := json.Unmarshal([]byte(resp), &res); err != nil {
			return "", nil, fmt.Errorf("invalid gateway response: %w", err)
		}
		if !res.Success {
			return "", nil, fmt.Errorf("gateway execution failed: %s", res.Error)
		}
		return res.Output, nil, nil
	}

	var input struct {
		Command        string   `json:"command"`
		Args           []string `json:"args"`
		Path           string   `json:"path"`
		TimeoutSeconds int      `json:"timeout_seconds"`
	}
	if err := json.Unmarshal([]byte(args), &input); err != nil {
		return "", nil, appErrors.Wrap(err, appErrors.CodeInvalidArgument, "invalid run_tests arguments", appErrors.LayerService)
	}

	cmdName := strings.TrimSpace(input.Command)
	if cmdName == "" {
		return "", nil, appErrors.New(appErrors.CodeInvalidArgument, "test command required", appErrors.LayerService)
	}

	allowedCommands := map[string]bool{
		"go":     true,
		"cargo":  true,
		"npm":    true,
		"pytest": true,
		"python": true,
		"ctest":  true,
		"dotnet": true,
	}

	binBase := filepath.Base(cmdName)
	binBase = strings.TrimSuffix(binBase, ".exe")
	if !allowedCommands[binBase] {
		return "", nil, appErrors.New(appErrors.CodePermissionDenied, fmt.Sprintf("unauthorized test command: %s", cmdName), appErrors.LayerService)
	}

	workspaceRoot := getWorkspaceRoot(execCtx)
	targetDir := workspaceRoot
	if input.Path != "" {
		safeDir, err := ValidateSandboxPath(workspaceRoot, input.Path)
		if err != nil {
			return "", nil, err
		}
		targetDir = safeDir
	}

	timeoutSec := input.TimeoutSeconds
	if timeoutSec <= 0 || timeoutSec > 300 {
		timeoutSec = 60
	}

	cmdCtx, cancel := context.WithTimeout(ctx, time.Duration(timeoutSec)*time.Second)
	defer cancel()

	cmd := exec.CommandContext(cmdCtx, cmdName, input.Args...)
	cmd.Dir = targetDir
	cmd.Env = scrubbedEnvironment()

	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	cmdErr := cmd.Run()

	combined := stdout.String()
	if stderr.Len() > 0 {
		if combined != "" {
			combined += "\n--- STDERR ---\n"
		}
		combined += stderr.String()
	}

	maxBytes := 50000
	var artifactIDs []string
	summaryOutput := combined

	// If output exceeds MaxOutputBytes, spillover to artifact file
	if len(combined) > maxBytes {
		runID := "run"
		if execCtx != nil && execCtx.RunID != "" {
			runID = execCtx.RunID
		}

		artDir := filepath.Join(workspaceRoot, "artifacts", "test_runs")
		_ = os.MkdirAll(artDir, 0755)
		artFilename := fmt.Sprintf("%s_test_log_%d.txt", runID, time.Now().Unix())
		artPath := filepath.Join(artDir, artFilename)
		_ = os.WriteFile(artPath, []byte(combined), 0644)

		relArt, _ := filepath.Rel(workspaceRoot, artPath)
		artID := fmt.Sprintf("art_%d", time.Now().UnixNano())
		artifactIDs = append(artifactIDs, artID)

		// Excerpt first 2000 chars and last 2000 chars
		head := combined[:2000]
		tail := combined[len(combined)-2000:]
		summaryOutput = fmt.Sprintf("%s\n\n... [TRUNCATED %d BYTES - FULL LOG PERSISTED AT %s (Artifact %s)] ...\n\n%s",
			head, len(combined)-4000, filepath.ToSlash(relArt), artID, tail)
	}

	if cmdErr != nil {
		return fmt.Sprintf("Tests failed with exit code error: %v\nOutput:\n%s", cmdErr, summaryOutput), artifactIDs, nil
	}

	return summaryOutput, artifactIDs, nil
}

// scrubbedEnvironment returns host environment stripped of all sensitive secrets and tokens
func scrubbedEnvironment() []string {
	safeKeys := map[string]bool{
		"PATH":        true,
		"HOME":        true,
		"USER":        true,
		"GOROOT":      true,
		"GOPATH":      true,
		"CARGO_HOME":  true,
		"RUSTUP_HOME": true,
		"SYSTEMROOT":  true,
		"WINDIR":      true,
		"COMSPEC":     true,
		"TEMP":        true,
		"TMP":         true,
		"OS":          true,
		"LANG":        true,
		"LC_ALL":      true,
	}

	var env []string
	for _, e := range os.Environ() {
		parts := strings.SplitN(e, "=", 2)
		if len(parts) == 0 {
			continue
		}
		key := strings.ToUpper(parts[0])

		// Deny any secret/credential patterns
		if strings.Contains(key, "SECRET") || strings.Contains(key, "TOKEN") ||
			strings.Contains(key, "KEY") || strings.Contains(key, "PASSWORD") ||
			strings.Contains(key, "AUTH") || strings.HasPrefix(key, "AWS_") ||
			strings.HasPrefix(key, "GITHUB_") || strings.HasPrefix(key, "OPENAI_") ||
			strings.HasPrefix(key, "ANTHROPIC_") || strings.HasPrefix(key, "GEMINI_") {
			continue
		}

		if safeKeys[key] {
			env = append(env, e)
		}
	}
	return env
}
