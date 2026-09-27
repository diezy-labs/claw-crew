package llm_test

import (
	"context"
	"strings"
	"testing"
	"time"

	"github.com/diezy-labs/claw-crew/engine/core/config"
	"github.com/diezy-labs/claw-crew/engine/pkg/client"
	"github.com/diezy-labs/claw-crew/engine/src/llm"
)

func TestLLMProviderSimulation(t *testing.T) {
	cfg := &config.AppConfig{}
	gateway, err := client.NewSystemGatewayClient("")
	if err != nil {
		t.Fatalf("failed to create gateway client: %v", err)
	}

	provider := llm.NewProvider(cfg, gateway)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	req := &llm.ChatRequest{
		Messages: []llm.Message{
			{Role: "user", Content: "Please read the config file"},
		},
	}

	chunkCh := make(chan *llm.ChatChunk, 16)
	errCh := make(chan error, 1)

	go func() {
		defer close(chunkCh)
		errCh <- provider.StreamChat(ctx, req, chunkCh)
	}()

	var receivedThought bool
	var receivedTools bool
	var receivedContent bool
	var isDone bool

	for chunk := range chunkCh {
		if chunk.ThoughtChunk != "" {
			receivedThought = true
		}
		if len(chunk.ToolCalls) > 0 {
			receivedTools = true
		}
		if chunk.ContentChunk != "" {
			receivedContent = true
		}
		if chunk.IsDone {
			isDone = true
		}
	}

	if err := <-errCh; err != nil {
		t.Fatalf("unexpected stream error: %v", err)
	}

	if !receivedThought {
		t.Errorf("expected thought chunk to be emitted")
	}
	if !receivedTools {
		t.Errorf("expected tool call to be triggered by file prompt")
	}
	if !receivedContent {
		t.Errorf("expected content chunks")
	}
	if !isDone {
		t.Errorf("expected is_done to be true at completion")
	}
}

func TestToolDispatcher(t *testing.T) {
	gateway, err := client.NewSystemGatewayClient("")
	if err != nil {
		t.Fatalf("failed to create gateway: %v", err)
	}

	dispatcher := llm.NewToolDispatcher(gateway)
	ctx := context.Background()

	// Test subagent spawn tool
	subCall := &llm.ToolCall{
		ID:        "sub_1",
		Name:      "spawn_subagent",
		Arguments: `{"role": "Tester", "task": "Run unit tests"}`,
	}

	res, err := dispatcher.Dispatch(ctx, subCall)
	if err != nil {
		t.Fatalf("failed to dispatch subagent: %v", err)
	}

	if !res.IsSubagentAction {
		t.Errorf("expected IsSubagentAction to be true")
	}
	if !strings.Contains(res.SubagentID, "Tester") {
		t.Errorf("expected subagent ID to contain 'Tester', got '%s'", res.SubagentID)
	}

	// Test OS tool fallback (standalone mock)
	toolCall := &llm.ToolCall{
		ID:        "cmd_1",
		Name:      "execute_command",
		Arguments: `{"command": "echo test"}`,
	}

	toolRes, err := dispatcher.Dispatch(ctx, toolCall)
	if err != nil {
		t.Fatalf("failed to dispatch tool: %v", err)
	}

	if toolRes.Output == "" {
		t.Errorf("expected non-empty output from native tool mock")
	}
}
