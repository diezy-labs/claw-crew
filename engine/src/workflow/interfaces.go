package workflow

import "context"

// Registry provides workflow and SOP template storage and discovery
type Registry interface {
	List(ctx context.Context) ([]*WorkflowTemplate, error)
	Get(ctx context.Context, id string) (*WorkflowTemplate, error)
	Register(template *WorkflowTemplate) error
}

// Service provides high-level workflow orchestration and instantiation into DAG tasks
type Service interface {
	ListWorkflows(ctx context.Context) ([]*WorkflowTemplate, error)
	GetWorkflow(ctx context.Context, id string) (*WorkflowTemplate, error)
	InstantiateWorkflow(ctx context.Context, workflowID string, req *InstantiateWorkflowRequest) (*InstantiateWorkflowResponse, error)
}
