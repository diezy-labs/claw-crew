package workflow

// StepTemplate represents a single task definition within a workflow template
type StepTemplate struct {
	ID             string   `json:"id"`
	Title          string   `json:"title"`
	Description    string   `json:"description"`
	AgentRole      string   `json:"agent_role"`
	Dependencies   []string `json:"dependencies"`
	PromptTemplate string   `json:"prompt_template"`
	TimeoutSeconds int      `json:"timeout_seconds,omitempty"`
}

// WorkflowTemplate represents an SOP or quickstart multi-step workflow definition
type WorkflowTemplate struct {
	ID          string          `json:"id"`
	Name        string          `json:"name"`
	Description string          `json:"description"`
	Category    string          `json:"category"`
	Version     string          `json:"version"`
	Steps       []StepTemplate  `json:"steps"`
}

// InstantiateWorkflowRequest contains arguments to instantiate a workflow for a run
type InstantiateWorkflowRequest struct {
	RunID      string                 `json:"run_id"`
	Parameters map[string]interface{} `json:"parameters,omitempty"`
}

// InstantiateWorkflowResponse contains the created task IDs in execution order
type InstantiateWorkflowResponse struct {
	WorkflowID string   `json:"workflow_id"`
	RunID      string   `json:"run_id"`
	TaskIDs    []string `json:"task_ids"`
}
