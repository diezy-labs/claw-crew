package crew

// TurnRequest data transfer object untuk memulai turn agent
type TurnRequest struct {
	SessionID     string `json:"session_id"`
	AgentID       string `json:"agent_id"`
	Prompt        string `json:"prompt"`
	ContextWindow []byte `json:"context_window"`
}

// TurnEventType jenis event yang dikirimkan selama streaming turn
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

// TurnEvent merepresentasikan satu payload event streaming ke client
type TurnEvent struct {
	Type         TurnEventType `json:"type"`
	Content      string        `json:"content"`
	SubagentID   string        `json:"subagent_id,omitempty"`
	ErrorMessage string        `json:"error_message,omitempty"`
}
