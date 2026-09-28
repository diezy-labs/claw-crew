package llm

import (
	"context"
	"strings"
	"time"
)

// MockProvider provides deterministic offline simulated LLM responses for unit and integration testing
type MockProvider struct {
	name      string
	responses []string
	toolCalls []ToolCall
}

// NewMockProvider constructs a MockProvider
func NewMockProvider(name string) *MockProvider {
	if name == "" {
		name = "mock-provider"
	}
	return &MockProvider{
		name: name,
		responses: []string{
			"Planning next steps...",
			"Executing implementation task...",
			"Task completed successfully.",
		},
	}
}

// SetResponses overrides mock text response steps
func (m *MockProvider) SetResponses(resp []string) {
	m.responses = resp
}

// SetToolCalls sets tool calls to return during generation
func (m *MockProvider) SetToolCalls(calls []ToolCall) {
	m.toolCalls = calls
}

func (m *MockProvider) Name() string {
	return m.name
}

func (m *MockProvider) StreamChat(ctx context.Context, req *ChatRequest, chunkCh chan<- *ChatChunk) error {
	// 1. Emit sanitized thought summary without leaking internal chain-of-thought
	select {
	case <-ctx.Done():
		return ctx.Err()
	case chunkCh <- &ChatChunk{
		ThoughtChunk:   "Analyzing request prompt and decomposing steps...",
		ThoughtSummary: "Decomposing task and planning execution path",
	}:
	}

	// 2. Emit text chunks
	for _, text := range m.responses {
		select {
		case <-ctx.Done():
			return ctx.Err()
		case chunkCh <- &ChatChunk{
			ContentChunk: text + " ",
		}:
		}
		time.Sleep(5 * time.Millisecond)
	}

	// 3. Emit tool calls if any
	if len(m.toolCalls) > 0 {
		select {
		case <-ctx.Done():
			return ctx.Err()
		case chunkCh <- &ChatChunk{
			ToolCalls: m.toolCalls,
		}:
		}
	}

	// 4. Emit completion with token usage
	totalWords := 0
	for _, r := range m.responses {
		totalWords += len(strings.Fields(r))
	}
	promptWords := len(strings.Fields(req.System))
	for _, msg := range req.Messages {
		promptWords += len(strings.Fields(msg.Content))
	}

	usage := &TokenUsage{
		PromptTokens:     promptWords * 2,
		CompletionTokens: totalWords * 2,
		TotalTokens:      (promptWords + totalWords) * 2,
		EstimatedCostUSD: float64((promptWords+totalWords)*2) * 0.000002,
	}

	select {
	case <-ctx.Done():
		return ctx.Err()
	case chunkCh <- &ChatChunk{
		IsDone: true,
		Usage:  usage,
	}:
	}

	return nil
}
