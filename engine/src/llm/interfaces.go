package llm

import (
	"context"
)

// Provider defines the communication contract with an upstream AI model
type Provider interface {
	Name() string
	StreamChat(ctx context.Context, req *ChatRequest, chunkCh chan<- *ChatChunk) error
}

// ChatRequest holds prompt, message history, and tool definitions for the LLM
type ChatRequest struct {
	Model       string           `json:"model"`
	System      string           `json:"system,omitempty"`
	Messages    []Message        `json:"messages"`
	Tools       []ToolDefinition `json:"tools,omitempty"`
	Temperature float32          `json:"temperature,omitempty"`
}

// Message represents an individual conversational message
type Message struct {
	Role    string `json:"role"`
	Content string `json:"content"`
}

// ToolDefinition represents a function schema callable by the model
type ToolDefinition struct {
	Name        string `json:"name"`
	Description string `json:"description"`
	Parameters  any    `json:"parameters"`
}

// ChatChunk represents a streaming chunk from an LLM response
type ChatChunk struct {
	ThoughtChunk string     `json:"thought_chunk,omitempty"`
	ContentChunk string     `json:"content_chunk,omitempty"`
	ToolCalls    []ToolCall `json:"tool_calls,omitempty"`
	IsDone       bool       `json:"is_done"`
	Error        error      `json:"-"`
}

// ToolCall represents a structured tool call instruction returned by the LLM
type ToolCall struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	Arguments string `json:"arguments"`
}
