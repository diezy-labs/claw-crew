package tool

import (
	"encoding/json"
	"time"
)

// RiskTier categorizes the safety profile of a tool
type RiskTier string

const (
	RiskTierRead    RiskTier = "READ"
	RiskTierWrite   RiskTier = "WRITE"
	RiskTierExecute RiskTier = "EXECUTE"
)

// RiskClass provides granular risk categorization for policy engines
type RiskClass string

const (
	RiskClassRead              RiskClass = "read"
	RiskClassCompute           RiskClass = "compute"
	RiskClassNetworkRead       RiskClass = "network_read"
	RiskClassWriteDraft        RiskClass = "write_draft"
	RiskClassWriteWorkspace    RiskClass = "write_workspace"
	RiskClassExecuteSandboxed  RiskClass = "execute_sandboxed"
	RiskClassExecutePrivileged RiskClass = "execute_privileged"
	RiskClassExternalAction    RiskClass = "external_action"
	RiskClassCredentialAccess  RiskClass = "credential_access"
)

// ApprovalStatus represents the state of a tool approval gate
type ApprovalStatus string

const (
	ApprovalPending  ApprovalStatus = "pending"
	ApprovalApproved ApprovalStatus = "approved"
	ApprovalDenied   ApprovalStatus = "denied"
	ApprovalTimedOut ApprovalStatus = "timed_out"
	ApprovalExpired  ApprovalStatus = "expired"
)

// IdempotencyMode defines how retries are handled across transient failures
type IdempotencyMode string

const (
	IdempotencySafe           IdempotencyMode = "safe"
	IdempotencyKeyRequired    IdempotencyMode = "idempotent_key_required"
	IdempotencyCompareAndSwap IdempotencyMode = "compare_and_swap"
	IdempotencyAtMostOnce     IdempotencyMode = "at_most_once"
	IdempotencyManualRecovery IdempotencyMode = "manual_recovery"
)

// ExecutionContext defines the security, audit, and scoping boundary (BUG-005 compliant)
type ExecutionContext struct {
	ActorID            string       `json:"actor_id"`
	SessionID          string       `json:"session_id,omitempty"`
	WorkspaceID        string       `json:"workspace_id,omitempty"`
	CrewID             string       `json:"crew_id,omitempty"`
	RunID              string       `json:"run_id,omitempty"`
	TaskID             string       `json:"task_id,omitempty"`
	RequestID          string       `json:"request_id,omitempty"`
	ApprovalState      string       `json:"approval_state,omitempty"`
	AllowedRoots       []string     `json:"allowed_roots,omitempty"`
	DataClassification string       `json:"data_classification,omitempty"`
	Capabilities       []string     `json:"capabilities,omitempty"`
	Gateway            SystemGateway `json:"-"` // Optional gRPC gateway for native builtin tool execution
}

// ToolDefinition describes a registered tool and its schema constraints
type ToolDefinition struct {
	ID               string          `json:"id"`
	Version          string          `json:"version"`
	DisplayName      string          `json:"display_name"`
	Description      string          `json:"description"`
	InputSchema      json.RawMessage `json:"input_schema"`
	OutputSchema     json.RawMessage `json:"output_schema,omitempty"`
	RiskTier         RiskTier        `json:"risk_tier"`
	RiskClass        RiskClass       `json:"risk_class"`
	Capabilities     []string        `json:"capabilities"`
	RequiresApproval bool            `json:"requires_approval"`
	TimeoutSeconds   int             `json:"timeout_seconds"`
	MaxOutputBytes   int64           `json:"max_output_bytes"`
	IdempotencyMode  IdempotencyMode `json:"idempotency_mode"`
	Source           string          `json:"source"` // "native" or "mcp:<server_name>"
}

// TargetResource specifies physical assets affected by the tool execution
type TargetResource struct {
	Type         string `json:"type"`          // "file", "network_host", "git_branch"
	Target       string `json:"target"`        // e.g. "engine/src/tool/services.go"
	ExpectedHash string `json:"expected_hash"` // SHA-256 for CAS validation
}

// ApprovalRequest binds user consent to an immutable, cryptographically hashed payload
type ApprovalRequest struct {
	ApprovalID              string           `json:"approval_id"`
	ToolRequestID           string           `json:"tool_request_id"`
	ToolName                string           `json:"tool_name"`
	RiskTier                RiskTier         `json:"risk_tier"`
	RiskClass               RiskClass        `json:"risk_class"`
	ExecutionContext        ExecutionContext `json:"execution_context"`
	NormalizedArgumentsHash string           `json:"normalized_arguments_hash"`
	RawArguments            string           `json:"raw_arguments"`
	ResolvedTargets         []TargetResource `json:"resolved_targets"`
	Summary                 string           `json:"summary"`
	PreviewDiff             string           `json:"preview_diff,omitempty"`
	ExpiresAt               time.Time        `json:"expires_at"`
	Status                  ApprovalStatus   `json:"status"`
	ResolvedAt              *time.Time       `json:"resolved_at,omitempty"`
}

// ToolExecution records an invocation of a tool
type ToolExecution struct {
	ID             string         `json:"id"`
	ActorID        string         `json:"actor_id"`
	RunID          string         `json:"run_id"`
	TaskID         string         `json:"task_id,omitempty"`
	ToolName       string         `json:"tool_name"`
	Arguments      string         `json:"arguments"`
	RiskTier       RiskTier       `json:"risk_tier"`
	ApprovalStatus ApprovalStatus `json:"approval_status"`
	Output         string         `json:"output,omitempty"`
	ArtifactIDs    []string       `json:"artifact_ids,omitempty"`
	ErrorMessage   string         `json:"error_message,omitempty"`
	CreatedAt      time.Time      `json:"created_at"`
	ResolvedAt     *time.Time     `json:"resolved_at,omitempty"`
}

// ApprovalDecisionPayload payload for approve/deny endpoints
type ApprovalDecisionPayload struct {
	Approved bool   `json:"approved"`
	Reason   string `json:"reason,omitempty"`
}

// ToolRequestPayload is the payload for invoking a tool via REST
type ToolRequestPayload struct {
	TaskID    string `json:"task_id,omitempty"`
	ToolName  string `json:"tool_name"`
	Arguments string `json:"arguments"`
}
