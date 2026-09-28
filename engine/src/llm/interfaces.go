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

// TokenUsage tracks prompt and completion tokens and estimated cost
type TokenUsage struct {
	PromptTokens     int     `json:"prompt_tokens"`
	CompletionTokens int     `json:"completion_tokens"`
	TotalTokens      int     `json:"total_tokens"`
	EstimatedCostUSD float64 `json:"estimated_cost_usd"`
}

// ChatChunk represents a streaming chunk from an LLM response
type ChatChunk struct {
	ThoughtChunk   string      `json:"thought_chunk,omitempty"`
	ThoughtSummary string      `json:"thought_summary,omitempty"`
	ContentChunk   string      `json:"content_chunk,omitempty"`
	ToolCalls      []ToolCall  `json:"tool_calls,omitempty"`
	Usage          *TokenUsage `json:"usage,omitempty"`
	IsDone         bool        `json:"is_done"`
	Error          error       `json:"-"`
}

// ToolCall represents a structured tool call instruction returned by the LLM
type ToolCall struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	Arguments string `json:"arguments"`
}

// MultiProvider allows dynamic model/provider selection and fallback policies
type MultiProvider interface {
	GetProvider(name string) (Provider, error)
	StreamWithRetry(ctx context.Context, providerName string, req *ChatRequest, chunkCh chan<- *ChatChunk) error
}
