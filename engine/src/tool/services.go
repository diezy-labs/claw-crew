package tool

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/core/logger"
	"github.com/diezy-labs/claw-crew/engine/core/metrics"
	"github.com/diezy-labs/claw-crew/engine/src/run"
	"github.com/prometheus/client_golang/prometheus"
)

// ValidateSandboxPath verifies that targetPath resolves inside workspaceRoot and blocks traversal & symlink escapes (BUG-005)
func ValidateSandboxPath(workspaceRoot, targetPath string) (string, error) {
	if workspaceRoot == "" {
		workspaceRoot = "."
	}

	cleanRoot, err := filepath.Abs(filepath.Clean(workspaceRoot))
	if err != nil {
		return "", fmt.Errorf("invalid workspace root: %w", err)
	}

	realRoot, err := filepath.EvalSymlinks(cleanRoot)
	if err != nil {
		realRoot = cleanRoot
	}

	var combined string
	if filepath.IsAbs(targetPath) {
		combined = filepath.Clean(targetPath)
	} else {
		combined = filepath.Join(cleanRoot, filepath.Clean(targetPath))
	}

	absCombined, err := filepath.Abs(combined)
	if err != nil {
		return "", fmt.Errorf("invalid path: %w", err)
	}

	// Lexical boundary check
	rel, err := filepath.Rel(cleanRoot, absCombined)
	if err != nil || strings.HasPrefix(rel, "..") {
		return "", appErrors.New(appErrors.CodePermissionDenied, fmt.Sprintf("path traversal blocked: %s is outside %s", targetPath, cleanRoot), appErrors.LayerService)
	}

	// Symlink escape resolution (prevent symlink from pointing outside realRoot)
	evalTarget := absCombined
	if _, statErr := os.Lstat(absCombined); statErr == nil {
		if realTarget, symErr := filepath.EvalSymlinks(absCombined); symErr == nil {
			evalTarget = realTarget
		}
	} else {
		parent := filepath.Dir(absCombined)
		if realParent, symErr := filepath.EvalSymlinks(parent); symErr == nil {
			evalTarget = filepath.Join(realParent, filepath.Base(absCombined))
		}
	}

	symRel, symRelErr := filepath.Rel(realRoot, evalTarget)
	if symRelErr != nil || strings.HasPrefix(symRel, "..") {
		return "", appErrors.New(appErrors.CodePermissionDenied, fmt.Sprintf("path traversal blocked: symlink escape detected for %s", targetPath), appErrors.LayerService)
	}

	return absCombined, nil
}

// withWorkspaceRoot injects the resolved workspace root into the tool arguments
// JSON envelope (key "__workspace_root") so the Rust SystemGateway can enforce the
// sandbox boundary on its side (F3-1). If args is empty it starts a fresh object;
// a non-object args payload is an error (native builtins always take a JSON object).
func withWorkspaceRoot(args, workspaceRoot string) (string, error) {
	env := map[string]any{}
	if strings.TrimSpace(args) != "" {
		if err := json.Unmarshal([]byte(args), &env); err != nil {
			return "", appErrors.New(appErrors.CodeInvalidArgument, fmt.Sprintf("gateway routing requires JSON-object arguments: %v", err), appErrors.LayerService)
		}
	}
	env["__workspace_root"] = workspaceRoot
	out, err := json.Marshal(env)
	if err != nil {
		return "", fmt.Errorf("encode gateway arguments: %w", err)
	}
	return string(out), nil
}

// Builtin WriteFileTool
type WriteFileTool struct{}

func (t *WriteFileTool) Name() string        { return "write_file" }
func (t *WriteFileTool) Description() string { return "Writes text to a file inside workspace" }
func (t *WriteFileTool) RiskTier() RiskTier  { return RiskTierWrite }
func (t *WriteFileTool) Definition() *ToolDefinition {
	return &ToolDefinition{
		ID:               t.Name(),
		Version:          "1.0.0",
		DisplayName:      "Write File",
		Description:      t.Description(),
		RiskTier:         RiskTierWrite,
		RiskClass:        RiskClassWriteWorkspace,
		Capabilities:     []string{"workspace.write"},
		RequiresApproval: true,
		TimeoutSeconds:   15,
		MaxOutputBytes:   50000,
		IdempotencyMode:  IdempotencySafe,
		Source:           "native",
		InputSchema:      json.RawMessage(`{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]}`),
	}
}

func (t *WriteFileTool) Execute(ctx context.Context, args string, workspaceRoot string) (string, error) {
	out, _, err := t.ExecuteWithContext(ctx, nil, args)
	return out, err
}

func (t *WriteFileTool) ExecuteWithContext(ctx context.Context, execCtx *ExecutionContext, args string) (string, []string, error) {
	var input struct {
		Path    string `json:"path"`
		Content string `json:"content"`
	}
	if err := json.Unmarshal([]byte(args), &input); err != nil {
		return "", nil, err
	}

	workspaceRoot := getWorkspaceRoot(execCtx)
	safePath, err := ValidateSandboxPath(workspaceRoot, input.Path)
	if err != nil {
		return "", nil, err
	}

	if err := os.MkdirAll(filepath.Dir(safePath), 0755); err != nil {
		return "", nil, err
	}

	if err := os.WriteFile(safePath, []byte(input.Content), 0644); err != nil {
		return "", nil, err
	}
	return fmt.Sprintf("Successfully wrote %d bytes to %s", len(input.Content), input.Path), nil, nil
}

// Builtin EditFileTool
type EditFileTool struct{}

func (t *EditFileTool) Name() string        { return "edit_file" }
func (t *EditFileTool) Description() string { return "Replaces target content inside a file" }
func (t *EditFileTool) RiskTier() RiskTier  { return RiskTierWrite }
func (t *EditFileTool) Definition() *ToolDefinition {
	return &ToolDefinition{
		ID:               t.Name(),
		Version:          "1.0.0",
		DisplayName:      "Edit File",
		Description:      t.Description(),
		RiskTier:         RiskTierWrite,
		RiskClass:        RiskClassWriteWorkspace,
		Capabilities:     []string{"workspace.write"},
		RequiresApproval: true,
		TimeoutSeconds:   15,
		MaxOutputBytes:   50000,
		IdempotencyMode:  IdempotencySafe,
		Source:           "native",
		InputSchema:      json.RawMessage(`{"type":"object","properties":{"path":{"type":"string"},"target":{"type":"string"},"replacement":{"type":"string"}},"required":["path","target","replacement"]}`),
	}
}

func (t *EditFileTool) Execute(ctx context.Context, args string, workspaceRoot string) (string, error) {
	out, _, err := t.ExecuteWithContext(ctx, nil, args)
	return out, err
}

func (t *EditFileTool) ExecuteWithContext(ctx context.Context, execCtx *ExecutionContext, args string) (string, []string, error) {
	var input struct {
		Path        string `json:"path"`
		Target      string `json:"target"`
		Replacement string `json:"replacement"`
	}
	if err := json.Unmarshal([]byte(args), &input); err != nil {
		return "", nil, err
	}

	workspaceRoot := getWorkspaceRoot(execCtx)
	safePath, err := ValidateSandboxPath(workspaceRoot, input.Path)
	if err != nil {
		return "", nil, err
	}

	data, err := os.ReadFile(safePath)
	if err != nil {
		return "", nil, err
	}

	original := string(data)
	if !strings.Contains(original, input.Target) {
		return "", nil, fmt.Errorf("target substring not found in %s", input.Path)
	}

	updated := strings.Replace(original, input.Target, input.Replacement, 1)
	if err := os.WriteFile(safePath, []byte(updated), 0644); err != nil {
		return "", nil, err
	}

	return fmt.Sprintf("Successfully updated %s", input.Path), nil, nil
}

// Builtin ExecuteCommandTool
type ExecuteCommandTool struct{}

func (t *ExecuteCommandTool) Name() string        { return "execute_command" }
func (t *ExecuteCommandTool) Description() string { return "Executes shell commands within workspace" }
func (t *ExecuteCommandTool) RiskTier() RiskTier  { return RiskTierExecute }
func (t *ExecuteCommandTool) Definition() *ToolDefinition {
	return &ToolDefinition{
		ID:               t.Name(),
		Version:          "1.0.0",
		DisplayName:      "Execute Command",
		Description:      t.Description(),
		RiskTier:         RiskTierExecute,
		RiskClass:        RiskClassExecutePrivileged,
		Capabilities:     []string{"command.execute"},
		RequiresApproval: true,
		TimeoutSeconds:   30,
		MaxOutputBytes:   50000,
		IdempotencyMode:  IdempotencyAtMostOnce,
		Source:           "native",
		InputSchema:      json.RawMessage(`{"type":"object","properties":{"command":{"type":"string"}},"required":["command"]}`),
	}
}

func (t *ExecuteCommandTool) Execute(ctx context.Context, args string, workspaceRoot string) (string, error) {
	out, _, err := t.ExecuteWithContext(ctx, nil, args)
	return out, err
}

func (t *ExecuteCommandTool) ExecuteWithContext(ctx context.Context, execCtx *ExecutionContext, args string) (string, []string, error) {
	var input struct {
		Command string `json:"command"`
	}
	if err := json.Unmarshal([]byte(args), &input); err != nil {
		return "", nil, err
	}

	workspaceRoot := getWorkspaceRoot(execCtx)

	var cmd *exec.Cmd
	if strings.Contains(strings.ToLower(os.Getenv("OS")), "windows") {
		cmd = exec.CommandContext(ctx, "cmd", "/c", input.Command)
	} else {
		cmd = exec.CommandContext(ctx, "sh", "-c", input.Command)
	}

	if workspaceRoot != "" {
		cmd.Dir = workspaceRoot
	}

	out, err := cmd.CombinedOutput()
	if err != nil {
		return string(out), nil, fmt.Errorf("command execution error: %w (output: %s)", err, string(out))
	}
	return string(out), nil, nil
}

// Builtin GitDiffTool
type GitDiffTool struct{}

func (t *GitDiffTool) Name() string        { return "git_diff" }
func (t *GitDiffTool) Description() string { return "Returns git diff of modified workspace files" }
func (t *GitDiffTool) RiskTier() RiskTier  { return RiskTierRead }
func (t *GitDiffTool) Definition() *ToolDefinition {
	return &ToolDefinition{
		ID:               t.Name(),
		Version:          "1.0.0",
		DisplayName:      "Git Diff",
		Description:      t.Description(),
		RiskTier:         RiskTierRead,
		RiskClass:        RiskClassRead,
		Capabilities:     []string{"git.read"},
		RequiresApproval: false,
		TimeoutSeconds:   15,
		MaxOutputBytes:   50000,
		IdempotencyMode:  IdempotencySafe,
		Source:           "native",
		InputSchema:      json.RawMessage(`{"type":"object"}`),
	}
}

func (t *GitDiffTool) Execute(ctx context.Context, args string, workspaceRoot string) (string, error) {
	out, _, err := t.ExecuteWithContext(ctx, nil, args)
	return out, err
}

func (t *GitDiffTool) ExecuteWithContext(ctx context.Context, execCtx *ExecutionContext, args string) (string, []string, error) {
	workspaceRoot := getWorkspaceRoot(execCtx)
	cmd := exec.CommandContext(ctx, "git", "diff")
	if workspaceRoot != "" {
		cmd.Dir = workspaceRoot
	}
	out, err := cmd.CombinedOutput()
	if err != nil {
		return "", nil, fmt.Errorf("git diff error: %w", err)
	}
	return string(out), nil, nil
}

// registerBuiltinTools registers all native Phase 3 tools
func registerBuiltinTools(r Registry) {
	r.Register(&ReadFileTool{})
	r.Register(&WriteFileTool{})
	r.Register(&EditFileTool{})
	r.Register(&ExecuteCommandTool{})
	r.Register(&GitDiffTool{})
	r.Register(&ListFilesTool{})
	r.Register(&SearchCodeTool{})
	r.Register(&CreateDraftTool{})
	r.Register(&ApplyPatchTool{})
	r.Register(NewWebFetchTool())
	r.Register(&RunLinterTool{})
	r.Register(&RunTestsTool{})
	r.Register(&GitStatusTool{})
	r.Register(&GitCreateBranchTool{})
	r.Register(&GitCommitTool{})
	r.Register(&DelegateTaskTool{})
	r.Register(&SandboxedCodeRunnerTool{})
	r.Register(&BrowserViewPageTool{})
	r.Register(&BrowserActionTool{})
}

// SystemGateway is the minimal slice of pkg/client.SystemGatewayClient the tool
// service needs to route native builtin execution across the process boundary
// to the Rust SystemGateway (F3-1). Declared locally to avoid a tool->client
// import cycle; *client.systemGatewayClient satisfies it structurally.
type SystemGateway interface {
	ExecuteNativeTool(ctx context.Context, toolName, argumentsJSON string) (string, error)
}

// toolService coordinates execution
type toolService struct {
	registry   Registry
	gate       ApprovalGate
	policy     PolicyEngine
	runService run.Service
	gateway    SystemGateway // when set, native builtins run in the Rust sandbox, not in-process (F3-1)
}

// NewService creates a tool service
func NewService(registry Registry, gate ApprovalGate, runService run.Service) Service {
	return &toolService{
		registry:   registry,
		gate:       gate,
		policy:     NewPolicyEngine(),
		runService: runService,
	}
}

// WithSystemGateway routes native builtin tool execution through the Rust
// SystemGateway gRPC instead of executing in-process (F3-1). Returns the same
// service for chaining; a nil gateway leaves in-process execution unchanged.
func (s *toolService) WithSystemGateway(gw SystemGateway) Service {
	s.gateway = gw
	return s
}

// NewServiceWithPolicy creates a tool service with explicit PolicyEngine injection
func NewServiceWithPolicy(registry Registry, gate ApprovalGate, policy PolicyEngine, runService run.Service) Service {
	if policy == nil {
		policy = NewPolicyEngine()
	}
	return &toolService{
		registry:   registry,
		gate:       gate,
		policy:     policy,
		runService: runService,
	}
}

func (s *toolService) GetRegistry() Registry         { return s.registry }
func (s *toolService) GetPolicyEngine() PolicyEngine { return s.policy }
func (s *toolService) GetApprovalGate() ApprovalGate { return s.gate }

func (s *toolService) ExecuteTool(ctx context.Context, runID, toolName, args, workspaceRoot string, requireApproval bool) (string, error) {
	execCtx := &ExecutionContext{
		ActorID:      "default_actor",
		RunID:        runID,
		AllowedRoots: []string{workspaceRoot},
	}
	return s.ExecuteWithContext(ctx, execCtx, toolName, args, workspaceRoot, requireApproval)
}

func (s *toolService) ExecuteWithContext(ctx context.Context, execCtx *ExecutionContext, toolName, args, workspaceRoot string, requireApproval bool) (string, error) {
	// 1. Enforce ActorID presence (Deny by default - BUG-005)
	if execCtx == nil || strings.TrimSpace(execCtx.ActorID) == "" {
		return "", appErrors.New(appErrors.CodePermissionDenied, "missing actor_id: tool execution requires an authenticated actor context", appErrors.LayerService)
	}

	// 2. Reject tool execution if parent run is in terminal status (ACT-P0-07)
	if execCtx.RunID != "" && s.runService != nil {
		r, err := s.runService.GetRun(ctx, execCtx.RunID)
		if err == nil && r != nil {
			if r.Status == run.StatusCancelled || r.Status == run.StatusCompleted || r.Status == run.StatusFailed || r.Status == run.StatusCancelling {
				return "", appErrors.New(appErrors.CodeFailedPrecondition, fmt.Sprintf("cannot execute tool %s: run %s is already in terminal state %s", toolName, execCtx.RunID, r.Status), appErrors.LayerService)
			}
		}
	}

	t, err := s.registry.Get(toolName)
	if err != nil {
		return "", err
	}

	toolDef := t.Definition()
	if toolDef == nil {
		toolDef = &ToolDefinition{
			ID:               toolName,
			RiskTier:         t.RiskTier(),
			RequiresApproval: t.RiskTier() == RiskTierWrite || t.RiskTier() == RiskTierExecute,
		}
	}

	// 3. Policy Engine Evaluation (FR-03, Level 2 maturity)
	verdict, polErr := s.policy.Evaluate(ctx, execCtx, toolDef, args)
	if polErr != nil {
		metrics.ToolPolicyDenialsTotal.WithLabelValues(toolName, "policy_evaluation_error").Inc()
		return "", polErr
	}

	if verdict == VerdictDeny {
		metrics.ToolPolicyDenialsTotal.WithLabelValues(toolName, "policy_denied").Inc()
		return "", appErrors.New(appErrors.CodePermissionDenied, fmt.Sprintf("policy engine denied execution of tool %s", toolName), appErrors.LayerService)
	}

	// 4. Approval gate check for side-effecting tiers or when verdict requires approval
	needsApproval := (verdict == VerdictRequireApproval) || (requireApproval && (t.RiskTier() == RiskTierWrite || t.RiskTier() == RiskTierExecute))
	if needsApproval {
		// Build detailed ApprovalRequest
		normArgs, _ := NormalizeArguments(args)
		argHash := HashArguments(normArgs)

		apprReq := &ApprovalRequest{
			ToolName:                toolName,
			RiskTier:                toolDef.RiskTier,
			RiskClass:               toolDef.RiskClass,
			RawArguments:            args,
			NormalizedArgumentsHash: argHash,
			Summary:                 fmt.Sprintf("Execute %s (%s)", toolName, toolDef.RiskTier),
			ExpiresAt:               time.Now().UTC().Add(10 * time.Minute),
		}

		// If mutating workspace file, attach target resource with hash for CAS
		var fileInput struct {
			Path             string `json:"path"`
			ExpectedFileHash string `json:"expected_file_hash"`
		}
		if json.Unmarshal([]byte(args), &fileInput) == nil && fileInput.Path != "" {
			safeP, err := ValidateSandboxPath(workspaceRoot, fileInput.Path)
			if err == nil {
				curHash, _ := HashFile(safeP)
				expectedH := fileInput.ExpectedFileHash
				if expectedH == "" {
					expectedH = curHash
				}
				apprReq.ResolvedTargets = append(apprReq.ResolvedTargets, TargetResource{
					Type:         "file",
					Target:       safeP,
					ExpectedHash: expectedH,
				})
			}
		}

		approved, reason, err := s.gate.RequestApprovalWithDetails(ctx, execCtx, apprReq)
		if err != nil {
			return "", err
		}
		if !approved {
			return "", appErrors.New(appErrors.CodePermissionDenied, fmt.Sprintf("tool execution denied: %s", reason), appErrors.LayerService)
		}
	}

	// Redact secrets in arguments for audit event
	redactedArgs := logger.RedactString(args)

	if s.runService != nil && execCtx.RunID != "" {
		s.runService.PublishEvent(execCtx.RunID, "tool.started", map[string]any{
			"tool":      toolName,
			"actor_id":  execCtx.ActorID,
			"arguments": redactedArgs,
		}, "")
	}

	// Measure execution duration metric
	timer := prometheus.NewTimer(metrics.ToolExecutionDuration.WithLabelValues(toolName, string(t.RiskTier())))
	defer timer.ObserveDuration()

	var output string
	var artifactIDs []string

	// F3-1: when a SystemGateway is wired, native builtin tools execute inside
	// the Rust sandbox (Landlock/Seatbelt) across gRPC :50052 instead of
	// in-process. MCP tools (source "mcp:*") always stay in-process. The
	// workspace root travels in the arguments envelope so Rust enforces the
	// sandbox boundary; the Go-side ValidateSandboxPath pre-check below is
	// defense-in-depth, not the sole gate.
	if s.gateway != nil && toolDef.Source == "native" {
		gwArgs, encErr := withWorkspaceRoot(args, workspaceRoot)
		if encErr != nil {
			return "", encErr
		}
		output, err = s.gateway.ExecuteNativeTool(ctx, toolName, gwArgs)
	} else {
		output, artifactIDs, err = t.ExecuteWithContext(ctx, execCtx, args)
	}
	if err != nil {
		metrics.ToolRequestsTotal.WithLabelValues(toolName, string(t.RiskTier()), "failed").Inc()
		if s.runService != nil && execCtx.RunID != "" {
			s.runService.PublishEvent(execCtx.RunID, "tool.failed", map[string]any{
				"tool":     toolName,
				"actor_id": execCtx.ActorID,
				"error":    err.Error(),
			}, err.Error())
		}
		return "", err
	}

	metrics.ToolRequestsTotal.WithLabelValues(toolName, string(t.RiskTier()), "completed").Inc()

	// Redact secrets in tool output before publishing or returning (BUG-017)
	redactedOutput := logger.RedactString(output)

	if s.runService != nil && execCtx.RunID != "" {
		s.runService.PublishEvent(execCtx.RunID, "tool.completed", map[string]any{
			"tool":         toolName,
			"actor_id":     execCtx.ActorID,
			"output":       redactedOutput,
			"artifact_ids": artifactIDs,
		}, "")
	}

	return output, nil
}

func (s *toolService) Approve(executionID string) error {
	return s.gate.Resolve(executionID, true, "")
}

func (s *toolService) Deny(executionID, reason string) error {
	return s.gate.Resolve(executionID, false, reason)
}

func (s *toolService) ListExecutions(runID string) []*ToolExecution {
	return s.gate.ListExecutions(runID)
}
