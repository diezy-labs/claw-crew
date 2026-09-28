package workflow

import (
	"context"
	"fmt"
	"sync"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/src/task"
)

// memoryRegistry implements Workflow Registry in memory with default built-in SOP templates
type memoryRegistry struct {
	mu        sync.RWMutex
	templates map[string]*WorkflowTemplate
}

// NewRegistry creates a new workflow Registry populated with standard templates
func NewRegistry() Registry {
	r := &memoryRegistry{
		templates: make(map[string]*WorkflowTemplate),
	}
	r.registerDefaults()
	return r
}

func (r *memoryRegistry) registerDefaults() {
	defaults := []*WorkflowTemplate{
		{
			ID:          "sop_feature_dev",
			Name:        "Feature Development SOP",
			Description: "Standard 4-step workflow: requirements analysis, architecture & planning, implementation, and test verification.",
			Category:    "development",
			Version:     "1.0.0",
			Steps: []StepTemplate{
				{
					ID:             "step_requirements",
					Title:          "Requirements Analysis & Architecture",
					Description:    "Analyze requirements, inspect existing architecture, and plan changes.",
					AgentRole:      "researcher",
					PromptTemplate: "Analyze the feature requirements and identify affected files.",
				},
				{
					ID:             "step_implementation",
					Title:          "Core Implementation",
					Description:    "Implement code changes according to architecture plan.",
					AgentRole:      "coder",
					Dependencies:   []string{"step_requirements"},
					PromptTemplate: "Implement feature code with minimal complexity and standard patterns.",
				},
				{
					ID:             "step_review",
					Title:          "Code Review & Diff Inspection",
					Description:    "Review generated diffs for correctness, security, and cleanliness.",
					AgentRole:      "reviewer",
					Dependencies:   []string{"step_implementation"},
					PromptTemplate: "Review changes against requirements and security best practices.",
				},
				{
					ID:             "step_test",
					Title:          "Test & Verification",
					Description:    "Execute unit and integration tests to verify behavior.",
					AgentRole:      "tester",
					Dependencies:   []string{"step_review"},
					PromptTemplate: "Run verification tests and confirm all suites pass.",
				},
			},
		},
		{
			ID:          "sop_bug_fix",
			Name:        "Bug Triage & Resolution SOP",
			Description: "Pinpoint root cause, author regression test, patch code, and verify fix.",
			Category:    "bugfix",
			Version:     "1.0.0",
			Steps: []StepTemplate{
				{
					ID:             "step_reproduce",
					Title:          "Reproduce & Root Cause Analysis",
					Description:    "Reproduce the bug and identify the failing code path.",
					AgentRole:      "researcher",
					PromptTemplate: "Trace execution path and identify defect location.",
				},
				{
					ID:             "step_patch",
					Title:          "Author Fix & Test Case",
					Description:    "Apply minimal fix and add regression test.",
					AgentRole:      "coder",
					Dependencies:   []string{"step_reproduce"},
					PromptTemplate: "Apply the simplest correct fix and add test.",
				},
				{
					ID:             "step_verify",
					Title:          "Regression Verification",
					Description:    "Run complete test suite to ensure no regressions.",
					AgentRole:      "tester",
					Dependencies:   []string{"step_patch"},
					PromptTemplate: "Verify test suite passes completely.",
				},
			},
		},
		{
			ID:          "sop_code_review",
			Name:        "Code Review & Security Audit SOP",
			Description: "Static analysis, security vulnerability review, and summary ledger generation.",
			Category:    "audit",
			Version:     "1.0.0",
			Steps: []StepTemplate{
				{
					ID:             "step_static_analysis",
					Title:          "Static Analysis & Linter Check",
					Description:    "Scan codebase for style and idiom violations.",
					AgentRole:      "reviewer",
					PromptTemplate: "Run linters and identify code smells.",
				},
				{
					ID:             "step_security_audit",
					Title:          "Security & Sandboxing Audit",
					Description:    "Inspect for injection, permission leakage, and path traversal.",
					AgentRole:      "reviewer",
					Dependencies:   []string{"step_static_analysis"},
					PromptTemplate: "Audit external boundaries and sandboxing guarantees.",
				},
				{
					ID:             "step_summary",
					Title:          "Summary Report",
					Description:    "Consolidate review findings into an actionable report.",
					AgentRole:      "researcher",
					Dependencies:   []string{"step_security_audit"},
					PromptTemplate: "Draft comprehensive summary report with risk levels.",
				},
			},
		},
		{
			ID:          "quickstart_default",
			Name:        "General Purpose Quickstart",
			Description: "Single-turn or quick task decomposition, execution, and review.",
			Category:    "quickstart",
			Version:     "1.0.0",
			Steps: []StepTemplate{
				{
					ID:             "step_plan",
					Title:          "Task Decomposition",
					Description:    "Formulate minimal execution plan.",
					AgentRole:      "researcher",
					PromptTemplate: "Break down the task into essential actions.",
				},
				{
					ID:             "step_run",
					Title:          "Execute Plan",
					Description:    "Execute tool actions and generate deliverables.",
					AgentRole:      "coder",
					Dependencies:   []string{"step_plan"},
					PromptTemplate: "Implement required modifications.",
				},
				{
					ID:             "step_conclude",
					Title:          "Outcome Review",
					Description:    "Verify deliverables and summarize status.",
					AgentRole:      "reviewer",
					Dependencies:   []string{"step_run"},
					PromptTemplate: "Verify deliverables match user expectations.",
				},
			},
		},
	}

	for _, tmpl := range defaults {
		_ = r.Register(tmpl)
	}
}

func (r *memoryRegistry) List(ctx context.Context) ([]*WorkflowTemplate, error) {
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	default:
	}

	r.mu.RLock()
	defer r.mu.RUnlock()

	res := make([]*WorkflowTemplate, 0, len(r.templates))
	for _, t := range r.templates {
		res = append(res, t)
	}
	return res, nil
}

func (r *memoryRegistry) Get(ctx context.Context, id string) (*WorkflowTemplate, error) {
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	default:
	}

	r.mu.RLock()
	defer r.mu.RUnlock()

	t, ok := r.templates[id]
	if !ok {
		return nil, appErrors.New(appErrors.CodeNotFound, fmt.Sprintf("workflow template not found: %s", id), appErrors.LayerService)
	}
	return t, nil
}

func (r *memoryRegistry) Register(template *WorkflowTemplate) error {
	if template == nil || template.ID == "" {
		return appErrors.New(appErrors.CodeInvalidArgument, "workflow template ID cannot be empty", appErrors.LayerService)
	}

	r.mu.Lock()
	defer r.mu.Unlock()

	r.templates[template.ID] = template
	return nil
}

// workflowService orchestrates workflow listing and DAG instantiation
type workflowService struct {
	registry Registry
	taskSvc  task.Service
}

// NewService constructs a workflow service
func NewService(registry Registry, taskSvc task.Service) Service {
	return &workflowService{
		registry: registry,
		taskSvc:  taskSvc,
	}
}

func (s *workflowService) ListWorkflows(ctx context.Context) ([]*WorkflowTemplate, error) {
	return s.registry.List(ctx)
}

func (s *workflowService) GetWorkflow(ctx context.Context, id string) (*WorkflowTemplate, error) {
	return s.registry.Get(ctx, id)
}

func (s *workflowService) InstantiateWorkflow(ctx context.Context, workflowID string, req *InstantiateWorkflowRequest) (*InstantiateWorkflowResponse, error) {
	if req == nil || req.RunID == "" {
		return nil, appErrors.New(appErrors.CodeInvalidArgument, "run_id is required", appErrors.LayerService)
	}

	tmpl, err := s.registry.Get(ctx, workflowID)
	if err != nil {
		return nil, err
	}

	// Map template step IDs to concrete instantiated task IDs
	stepToTaskID := make(map[string]string)
	createdTaskIDs := make([]string, 0, len(tmpl.Steps))

	for _, step := range tmpl.Steps {
		// Resolve concrete dependency task IDs
		var concreteDeps []string
		for _, depStepID := range step.Dependencies {
			if taskID, ok := stepToTaskID[depStepID]; ok {
				concreteDeps = append(concreteDeps, taskID)
			}
		}

		createdTask, err := s.taskSvc.CreateTask(ctx, &task.CreateTaskRequest{
			RunID:        req.RunID,
			Title:        step.Title,
			Description:  step.Description,
			AssignedTo:   step.AgentRole,
			Dependencies: concreteDeps,
		})
		if err != nil {
			return nil, appErrors.Wrap(err, appErrors.CodeInternal, fmt.Sprintf("failed to instantiate workflow step %s", step.ID), appErrors.LayerService)
		}

		stepToTaskID[step.ID] = createdTask.ID
		createdTaskIDs = append(createdTaskIDs, createdTask.ID)
	}

	return &InstantiateWorkflowResponse{
		WorkflowID: workflowID,
		RunID:      req.RunID,
		TaskIDs:    createdTaskIDs,
	}, nil
}
