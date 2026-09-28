package tool

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
)

// GitStatusTool implements git.status
type GitStatusTool struct{}

func (t *GitStatusTool) Name() string { return "git.status" }
func (t *GitStatusTool) Description() string {
	return "Returns modified files and current branch in the workspace"
}
func (t *GitStatusTool) RiskTier() RiskTier { return RiskTierRead }
func (t *GitStatusTool) Definition() *ToolDefinition {
	return &ToolDefinition{
		ID:               t.Name(),
		Version:          "1.0.0",
		DisplayName:      "Git Workspace Status",
		Description:      t.Description(),
		RiskTier:         RiskTierRead,
		RiskClass:        RiskClassRead,
		Capabilities:     []string{"git.read"},
		RequiresApproval: false,
		TimeoutSeconds:   15,
		MaxOutputBytes:   50000,
		IdempotencyMode:  IdempotencySafe,
		Source:           "native",
		InputSchema:      json.RawMessage(`{"type": "object"}`),
	}
}

func (t *GitStatusTool) Execute(ctx context.Context, args string, workspaceRoot string) (string, error) {
	out, _, err := t.ExecuteWithContext(ctx, nil, args)
	return out, err
}

func (t *GitStatusTool) ExecuteWithContext(ctx context.Context, execCtx *ExecutionContext, args string) (string, []string, error) {
	workspaceRoot := getWorkspaceRoot(execCtx)

	cmd := exec.CommandContext(ctx, "git", "status", "--porcelain", "-b")
	cmd.Dir = workspaceRoot
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return "", nil, appErrors.New(appErrors.CodeToolFailed, fmt.Sprintf("git status failed: %v (stderr: %s)", err, stderr.String()), appErrors.LayerExternal)
	}

	return stdout.String(), nil, nil
}

// GitCreateBranchTool implements git.create_branch
type GitCreateBranchTool struct{}

func (t *GitCreateBranchTool) Name() string        { return "git.create_branch" }
func (t *GitCreateBranchTool) Description() string { return "Creates and switches to a new Git branch" }
func (t *GitCreateBranchTool) RiskTier() RiskTier  { return RiskTierWrite }
func (t *GitCreateBranchTool) Definition() *ToolDefinition {
	return &ToolDefinition{
		ID:               t.Name(),
		Version:          "1.0.0",
		DisplayName:      "Git Create Branch",
		Description:      t.Description(),
		RiskTier:         RiskTierWrite,
		RiskClass:        RiskClassWriteWorkspace,
		Capabilities:     []string{"git.write"},
		RequiresApproval: false,
		TimeoutSeconds:   15,
		MaxOutputBytes:   50000,
		IdempotencyMode:  IdempotencySafe,
		Source:           "native",
		InputSchema: json.RawMessage(`{
			"type": "object",
			"properties": {
				"branch_name": {"type": "string", "description": "Name of branch to create"}
			},
			"required": ["branch_name"]
		}`),
	}
}

var validBranchRegex = regexp.MustCompile(`^[a-zA-Z0-9_\-\.\/]+$`)

func (t *GitCreateBranchTool) Execute(ctx context.Context, args string, workspaceRoot string) (string, error) {
	out, _, err := t.ExecuteWithContext(ctx, nil, args)
	return out, err
}

func (t *GitCreateBranchTool) ExecuteWithContext(ctx context.Context, execCtx *ExecutionContext, args string) (string, []string, error) {
	var input struct {
		BranchName string `json:"branch_name"`
	}
	if err := json.Unmarshal([]byte(args), &input); err != nil {
		return "", nil, appErrors.Wrap(err, appErrors.CodeInvalidArgument, "invalid git.create_branch arguments", appErrors.LayerService)
	}

	branch := strings.TrimSpace(input.BranchName)
	if branch == "" || strings.HasPrefix(branch, "-") || !validBranchRegex.MatchString(branch) {
		return "", nil, appErrors.New(appErrors.CodeInvalidArgument, fmt.Sprintf("invalid branch name: %s", branch), appErrors.LayerService)
	}

	workspaceRoot := getWorkspaceRoot(execCtx)

	cmd := exec.CommandContext(ctx, "git", "checkout", "-b", branch)
	cmd.Dir = workspaceRoot
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return "", nil, appErrors.New(appErrors.CodeToolFailed, fmt.Sprintf("git checkout -b failed: %v (stderr: %s)", err, stderr.String()), appErrors.LayerExternal)
	}

	out := stdout.String()
	if out == "" {
		out = stderr.String()
	}
	return strings.TrimSpace(out), nil, nil
}

// GitCommitTool implements git.commit requiring explicit human approval
type GitCommitTool struct{}

func (t *GitCommitTool) Name() string { return "git.commit" }
func (t *GitCommitTool) Description() string {
	return "Commits staged changes to the repository. Requires human approval"
}
func (t *GitCommitTool) RiskTier() RiskTier { return RiskTierWrite }
func (t *GitCommitTool) Definition() *ToolDefinition {
	return &ToolDefinition{
		ID:               t.Name(),
		Version:          "1.0.0",
		DisplayName:      "Git Commit",
		Description:      t.Description(),
		RiskTier:         RiskTierWrite,
		RiskClass:        RiskClassWriteWorkspace,
		Capabilities:     []string{"git.write"},
		RequiresApproval: true,
		TimeoutSeconds:   20,
		MaxOutputBytes:   50000,
		IdempotencyMode:  IdempotencyAtMostOnce,
		Source:           "native",
		InputSchema: json.RawMessage(`{
			"type": "object",
			"properties": {
				"message": {"type": "string", "description": "Commit message"},
				"files": {"type": "array", "items": {"type": "string"}, "description": "Files to stage before committing (optional, default all modified)"}
			},
			"required": ["message"]
		}`),
	}
}

func (t *GitCommitTool) Execute(ctx context.Context, args string, workspaceRoot string) (string, error) {
	out, _, err := t.ExecuteWithContext(ctx, nil, args)
	return out, err
}

func (t *GitCommitTool) ExecuteWithContext(ctx context.Context, execCtx *ExecutionContext, args string) (string, []string, error) {
	var input struct {
		Message string   `json:"message"`
		Files   []string `json:"files"`
	}
	if err := json.Unmarshal([]byte(args), &input); err != nil {
		return "", nil, appErrors.Wrap(err, appErrors.CodeInvalidArgument, "invalid git.commit arguments", appErrors.LayerService)
	}

	msg := strings.TrimSpace(input.Message)
	if msg == "" {
		return "", nil, appErrors.New(appErrors.CodeInvalidArgument, "commit message cannot be empty", appErrors.LayerService)
	}

	workspaceRoot := getWorkspaceRoot(execCtx)

	// Stage files
	stageArgs := []string{"add"}
	if len(input.Files) > 0 {
		for _, f := range input.Files {
			safeF, err := ValidateSandboxPath(workspaceRoot, f)
			if err != nil {
				return "", nil, err
			}
			rel, _ := filepath.Rel(workspaceRoot, safeF)
			stageArgs = append(stageArgs, rel)
		}
	} else {
		stageArgs = append(stageArgs, "-A")
	}

	addCmd := exec.CommandContext(ctx, "git", stageArgs...)
	addCmd.Dir = workspaceRoot
	if err := addCmd.Run(); err != nil {
		return "", nil, appErrors.New(appErrors.CodeToolFailed, fmt.Sprintf("git add failed: %v", err), appErrors.LayerExternal)
	}

	// Commit with message
	commitCmd := exec.CommandContext(ctx, "git", "commit", "-m", msg)
	commitCmd.Dir = workspaceRoot
	var stdout, stderr bytes.Buffer
	commitCmd.Stdout = &stdout
	commitCmd.Stderr = &stderr

	if err := commitCmd.Run(); err != nil {
		return "", nil, appErrors.New(appErrors.CodeToolFailed, fmt.Sprintf("git commit failed: %v (stderr: %s)", err, stderr.String()), appErrors.LayerExternal)
	}

	return strings.TrimSpace(stdout.String()), nil, nil
}
