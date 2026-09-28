package tool

import (
	"context"
)

// Tool represents a runnable tool definition
type Tool interface {
	Name() string
	Description() string
	RiskTier() RiskTier
	Definition() *ToolDefinition
	Execute(ctx context.Context, args string, workspaceRoot string) (string, error)
	ExecuteWithContext(ctx context.Context, execCtx *ExecutionContext, args string) (string, []string, error)
}

// Registry manages registered native and MCP tools
type Registry interface {
	Register(t Tool)
	Get(name string) (Tool, error)
	List() []Tool
	ListByScope(workspaceID string, allowedCapabilities []string) []Tool
}

// PolicyVerdict represents the outcome of policy evaluation
type PolicyVerdict string

const (
	VerdictAllow           PolicyVerdict = "ALLOW"
	VerdictRequireApproval PolicyVerdict = "REQUIRE_APPROVAL"
	VerdictDeny            PolicyVerdict = "DENY"
)

// PolicyEngine evaluates if a tool call is permitted or requires explicit user approval
type PolicyEngine interface {
	Evaluate(ctx context.Context, execCtx *ExecutionContext, toolDef *ToolDefinition, args string) (PolicyVerdict, error)
}

// ApprovalGate coordinates user approval pauses for side-effecting tools
type ApprovalGate interface {
	RequestApproval(ctx context.Context, runID, toolName, args string, tier RiskTier) (bool, string, error)
	RequestApprovalWithContext(ctx context.Context, execCtx *ExecutionContext, toolName, args string, tier RiskTier) (bool, string, error)
	RequestApprovalWithDetails(ctx context.Context, execCtx *ExecutionContext, req *ApprovalRequest) (bool, string, error)
	Resolve(executionID string, approved bool, reason string) error
	GetExecution(id string) (*ToolExecution, error)
	GetApprovalRequest(approvalID string) (*ApprovalRequest, error)
	ListExecutions(runID string) []*ToolExecution
}

// Service coordinates tool execution, sandboxing, and approval gates
type Service interface {
	ExecuteTool(ctx context.Context, runID, toolName, args, workspaceRoot string, requireApproval bool) (string, error)
	ExecuteWithContext(ctx context.Context, execCtx *ExecutionContext, toolName, args, workspaceRoot string, requireApproval bool) (string, error)
	Approve(executionID string) error
	Deny(executionID, reason string) error
	ListExecutions(runID string) []*ToolExecution
	GetRegistry() Registry
	GetPolicyEngine() PolicyEngine
	GetApprovalGate() ApprovalGate
}
