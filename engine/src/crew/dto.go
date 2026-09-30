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

// CrewMemberStatus represents the state machine status of an agent
type CrewMemberStatus string

const (
	CrewMemberStatusIdle            CrewMemberStatus = "idle"
	CrewMemberStatusThinking        CrewMemberStatus = "thinking"
	CrewMemberStatusExecutingTool   CrewMemberStatus = "executing_tool"
	CrewMemberStatusWaitingApproval CrewMemberStatus = "waiting_approval"
	CrewMemberStatusCompleted       CrewMemberStatus = "completed"
	CrewMemberStatusError           CrewMemberStatus = "error"
)

// CrewMember describes an agent's identity, role, and capabilities
type CrewMember struct {
	ID           string           `json:"id"`
	Name         string           `json:"name"`
	Role         string           `json:"role"`
	Status       CrewMemberStatus `json:"status"`
	Capabilities []string         `json:"capabilities"`
}

// Squad represents a team of specialized agents
type Squad struct {
	ID              string        `json:"id"`
	ShipID          string        `json:"ship_id"`
	Name            string        `json:"name"`
	Description     string        `json:"description"`
	CrewMemberCount int           `json:"crew_member_count"`
	Agents          []*CrewMember `json:"agents"`
}

// ListSquadsResponse payload for GET /api/v1/crews
type ListSquadsResponse struct {
	Items []*Squad `json:"items"`
}
