package llm

import (
	"context"
)

// Provider mendefinisikan antarmuka komunikasi ke Model AI
type Provider interface {
	Name() string
	StreamChat(ctx context.Context, req *ChatRequest, chunkCh chan<- *ChatChunk) error
}

// ChatRequest memuat data prompt dan tools untuk LLM
type ChatRequest struct {
	Model       string         `json:"model"`
	System      string         `json:"system,omitempty"`
	Messages    []Message      `json:"messages"`
	Tools       []ToolDefinition `json:"tools,omitempty"`
	Temperature float32        `json:"temperature,omitempty"`
}

// Message merepresentasikan pesan chat tunggal
type Message struct {
	Role    string `json:"role"`
	Content string `json:"content"`
}

// ToolDefinition merepresentasikan skema tool yang dapat dipanggil LLM
type ToolDefinition struct {
	Name        string `json:"name"`
	Description string `json:"description"`
	Parameters  any    `json:"parameters"`
}

// ChatChunk mewakili satu potongan respon LLM
type ChatChunk struct {
	ThoughtChunk string      `json:"thought_chunk,omitempty"`
	ContentChunk string      `json:"content_chunk,omitempty"`
	ToolCalls    []ToolCall  `json:"tool_calls,omitempty"`
	IsDone       bool        `json:"is_done"`
	Error        error       `json:"-"`
}

// ToolCall merepresentasikan instruksi pemanggilan tool oleh LLM
type ToolCall struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	Arguments string `json:"arguments"`
}
