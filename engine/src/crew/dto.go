package crew

// TurnRequest represents the input data required to initiate an agent turn
type TurnRequest struct {
	SessionID     string `json:"session_id"`
	AgentID       string `json:"agent_id"`
	Prompt        string `json:"prompt"`
	ContextWindow []byte `json:"context_window"`
}

// TurnEventType indicates the category of event streamed during an active turn
type TurnEventType string

const (
	EventThoughtChunk     TurnEventType = "THOUGHT_CHUNK"
	EventTextChunk        TurnEventType = "TEXT_CHUNK"
	EventToolCallStarted  TurnEventType = "TOOL_CALL_STARTED"
	EventToolCallFinished TurnEventType = "TOOL_CALL_FINISHED"
	EventSubagentSpawned  TurnEventType = "SUBAGENT_SPAWNED"
	EventTurnCompleted    TurnEventType = "TURN_COMPLETED"
	EventError            TurnEventType = "ERROR"
)

// TurnEvent represents a single streamed event payload sent to the caller
type TurnEvent struct {
	Type         TurnEventType `json:"type"`
	Content      string        `json:"content"`
	SubagentID   string        `json:"subagent_id,omitempty"`
	ErrorMessage string        `json:"error_message,omitempty"`
}

// AgentStatus represents the state machine status of an agent
type AgentStatus string

const (
	AgentStatusIdle            AgentStatus = "idle"
	AgentStatusThinking        AgentStatus = "thinking"
	AgentStatusExecutingTool   AgentStatus = "executing_tool"
	AgentStatusWaitingApproval AgentStatus = "waiting_approval"
	AgentStatusCompleted       AgentStatus = "completed"
	AgentStatusError           AgentStatus = "error"
)

// AgentDefinition describes an agent's identity, role, and capabilities
type AgentDefinition struct {
	ID           string      `json:"id"`
	Name         string      `json:"name"`
	Role         string      `json:"role"`
	Status       AgentStatus `json:"status"`
	Capabilities []string    `json:"capabilities"`
}

// CrewDefinition represents a team of specialized agents
type CrewDefinition struct {
	ID          string             `json:"id"`
	Name        string             `json:"name"`
	Description string             `json:"description"`
	AgentCount  int                `json:"agent_count"`
	Agents      []*AgentDefinition `json:"agents"`
}

// ListCrewsResponse payload for GET /api/v1/crews
type ListCrewsResponse struct {
	Items []*CrewDefinition `json:"items"`
}
