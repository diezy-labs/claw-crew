package tool

import (
	"context"
	"fmt"
	"strings"
	"sync"
	"time"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/core/id"
	"github.com/diezy-labs/claw-crew/engine/core/logger"
	"github.com/diezy-labs/claw-crew/engine/core/metrics"
	"github.com/diezy-labs/claw-crew/engine/src/run"
)

// MemoryApprovalGate manages cryptographic approval pauses and CAS bindings
type MemoryApprovalGate struct {
	mu          sync.RWMutex
	executions  map[string]*ToolExecution
	approvals   map[string]*ApprovalRequest
	approvalMap map[string]string // execID -> approvalID and approvalID -> execID
	channels    map[string]chan bool
	reasons     map[string]string
	runService  run.Service
}

// NewApprovalGate creates an approval gate
func NewApprovalGate(runService run.Service) ApprovalGate {
	return &MemoryApprovalGate{
		executions:  make(map[string]*ToolExecution),
		approvals:   make(map[string]*ApprovalRequest),
		approvalMap: make(map[string]string),
		channels:    make(map[string]chan bool),
		reasons:     make(map[string]string),
		runService:  runService,
	}
}

func (g *MemoryApprovalGate) RequestApproval(ctx context.Context, runID, toolName, args string, tier RiskTier) (bool, string, error) {
	return g.RequestApprovalWithContext(ctx, &ExecutionContext{RunID: runID, ActorID: "default_actor"}, toolName, args, tier)
}

func (g *MemoryApprovalGate) RequestApprovalWithContext(ctx context.Context, execCtx *ExecutionContext, toolName, args string, tier RiskTier) (bool, string, error) {
	// Canonicalize and hash arguments
	normArgs, _ := NormalizeArguments(args)
	argHash := HashArguments(normArgs)

	summary := fmt.Sprintf("Execute %s (%s)", toolName, tier)
	req := &ApprovalRequest{
		ToolName:                toolName,
		RiskTier:                tier,
		RawArguments:            args,
		NormalizedArgumentsHash: argHash,
		Summary:                 summary,
		ExpiresAt:               time.Now().UTC().Add(10 * time.Minute),
	}
	return g.RequestApprovalWithDetails(ctx, execCtx, req)
}

func (g *MemoryApprovalGate) RequestApprovalWithDetails(ctx context.Context, execCtx *ExecutionContext, req *ApprovalRequest) (bool, string, error) {
	execID := id.NewExecutionID()
	apprID := id.Generate("appr_")
	now := time.Now().UTC()

	runID := ""
	actorID := "default_actor"
	if execCtx != nil {
		runID = execCtx.RunID
		if execCtx.ActorID != "" {
			actorID = execCtx.ActorID
		}
		req.ExecutionContext = *execCtx
	}

	redactedArgs := logger.RedactString(req.RawArguments)

	execution := &ToolExecution{
		ID:             execID,
		ActorID:        actorID,
		RunID:          runID,
		TaskID:         req.ExecutionContext.TaskID,
		ToolName:       req.ToolName,
		Arguments:      redactedArgs,
		RiskTier:       req.RiskTier,
		ApprovalStatus: ApprovalPending,
		CreatedAt:      now,
	}

	req.ApprovalID = apprID
	req.ToolRequestID = execID
	req.Status = ApprovalPending
	if req.ExpiresAt.IsZero() {
		req.ExpiresAt = now.Add(10 * time.Minute)
	}

	decisionCh := make(chan bool, 1)

	g.mu.Lock()
	g.executions[execID] = execution
	g.approvals[apprID] = req
	g.approvalMap[execID] = apprID
	g.approvalMap[apprID] = execID
	g.channels[execID] = decisionCh
	g.channels[apprID] = decisionCh
	g.mu.Unlock()

	// Emit tool.approval_required event
	if g.runService != nil && runID != "" {
		g.runService.PublishEvent(runID, "tool.approval_required", map[string]any{
			"execution_id":              execID,
			"approval_id":               apprID,
			"actor_id":                  actorID,
			"tool_name":                 req.ToolName,
			"risk_tier":                 req.RiskTier,
			"risk_class":                req.RiskClass,
			"summary":                   req.Summary,
			"preview_diff":              req.PreviewDiff,
			"resolved_targets":          req.ResolvedTargets,
			"normalized_arguments_hash": req.NormalizedArgumentsHash,
			"arguments":                 redactedArgs,
			"expires_at":                req.ExpiresAt.Format(time.RFC3339),
		}, "")
	}

	select {
	case <-ctx.Done():
		g.mu.Lock()
		execution.ApprovalStatus = ApprovalTimedOut
		req.Status = ApprovalTimedOut
		delete(g.channels, execID)
		delete(g.channels, apprID)
		g.mu.Unlock()
		metrics.ToolApprovalsTotal.WithLabelValues(req.ToolName, string(ApprovalTimedOut)).Inc()
		return false, "timeout / context cancelled", ctx.Err()

	case approved := <-decisionCh:
		g.mu.RLock()
		reason := g.reasons[execID]
		g.mu.RUnlock()
		return approved, reason, nil
	}
}

func (g *MemoryApprovalGate) Resolve(idOrApprovalID string, approved bool, reason string) error {
	g.mu.Lock()
	defer g.mu.Unlock()

	// Find execution and approval
	execID := idOrApprovalID
	apprID := idOrApprovalID

	if mapped, ok := g.approvalMap[idOrApprovalID]; ok {
		if strings.HasPrefix(idOrApprovalID, "appr_") {
			execID = mapped
		} else {
			apprID = mapped
		}
	}

	execution, ok := g.executions[execID]
	if !ok {
		return appErrors.New(appErrors.CodeNotFound, fmt.Sprintf("execution or approval not found: %s", idOrApprovalID), appErrors.LayerService)
	}

	appr, _ := g.approvals[apprID]

	// Check if already expired
	if appr != nil && time.Now().UTC().After(appr.ExpiresAt) {
		execution.ApprovalStatus = ApprovalExpired
		appr.Status = ApprovalExpired
		metrics.ToolApprovalsTotal.WithLabelValues(execution.ToolName, string(ApprovalExpired)).Inc()
		return appErrors.New(appErrors.CodeFailedPrecondition, fmt.Sprintf("approval %s has expired", apprID), appErrors.LayerService)
	}

	// Enforce idempotency: prevent double-approval or resolution races
	if execution.ApprovalStatus != ApprovalPending {
		return appErrors.New(appErrors.CodeFailedPrecondition, fmt.Sprintf("execution %s is already resolved with status %s", execID, execution.ApprovalStatus), appErrors.LayerService)
	}

	// CAS validation if targets specify expected hash
	if approved && appr != nil && len(appr.ResolvedTargets) > 0 {
		for _, target := range appr.ResolvedTargets {
			if target.Type == "file" && target.ExpectedHash != "" {
				currentHash, err := HashFile(target.Target)
				if err == nil && currentHash != target.ExpectedHash {
					execution.ApprovalStatus = ApprovalDenied
					appr.Status = ApprovalDenied
					g.reasons[execID] = fmt.Sprintf("CAS TOCTOU mismatch on %s: expected %s, got %s", target.Target, target.ExpectedHash, currentHash)
					if ch, ok := g.channels[execID]; ok {
						ch <- false
						delete(g.channels, execID)
						delete(g.channels, apprID)
					}
					metrics.ToolApprovalsTotal.WithLabelValues(execution.ToolName, string(ApprovalDenied)).Inc()
					return appErrors.New(appErrors.CodeFailedPrecondition, fmt.Sprintf("CAS hash conflict on %s: file was modified concurrently", target.Target), appErrors.LayerService)
				}
			}
		}
	}

	now := time.Now().UTC()
	execution.ResolvedAt = &now
	if approved {
		execution.ApprovalStatus = ApprovalApproved
		if appr != nil {
			appr.Status = ApprovalApproved
			appr.ResolvedAt = &now
		}
		metrics.ToolApprovalsTotal.WithLabelValues(execution.ToolName, string(ApprovalApproved)).Inc()
	} else {
		execution.ApprovalStatus = ApprovalDenied
		if appr != nil {
			appr.Status = ApprovalDenied
			appr.ResolvedAt = &now
		}
		metrics.ToolApprovalsTotal.WithLabelValues(execution.ToolName, string(ApprovalDenied)).Inc()
	}

	g.reasons[execID] = reason

	if ch, ok := g.channels[execID]; ok {
		ch <- approved
		delete(g.channels, execID)
		delete(g.channels, apprID)
	}

	return nil
}

func (g *MemoryApprovalGate) GetExecution(id string) (*ToolExecution, error) {
	g.mu.RLock()
	defer g.mu.RUnlock()
	e, ok := g.executions[id]
	if !ok {
		return nil, appErrors.New(appErrors.CodeNotFound, fmt.Sprintf("execution not found: %s", id), appErrors.LayerService)
	}
	return e, nil
}

func (g *MemoryApprovalGate) GetApprovalRequest(approvalID string) (*ApprovalRequest, error) {
	g.mu.RLock()
	defer g.mu.RUnlock()
	a, ok := g.approvals[approvalID]
	if !ok {
		return nil, appErrors.New(appErrors.CodeNotFound, fmt.Sprintf("approval not found: %s", approvalID), appErrors.LayerService)
	}
	return a, nil
}

func (g *MemoryApprovalGate) ListExecutions(runID string) []*ToolExecution {
	g.mu.RLock()
	defer g.mu.RUnlock()
	list := make([]*ToolExecution, 0)
	for _, e := range g.executions {
		if e.RunID == runID {
			list = append(list, e)
		}
	}
	return list
}
