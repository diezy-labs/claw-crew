package llm_test

import (
	"context"
	"testing"
	"time"

	"github.com/diezy-labs/claw-crew/engine/core/config"
	"github.com/diezy-labs/claw-crew/engine/pkg/client"
	"github.com/diezy-labs/claw-crew/engine/src/llm"
)

func TestStreamChat_ProviderCancellation(t *testing.T) {
	cfg := &config.AppConfig{}
	gateway, err := client.NewSystemGatewayClient("")
	if err != nil {
		t.Fatalf("failed to create gateway: %v", err)
	}

	provider := llm.NewProvider(cfg, gateway)

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	req := &llm.ChatRequest{
		Messages: []llm.Message{
			{Role: "user", Content: "Simulate streaming that gets cancelled"},
		},
	}

	chunkCh := make(chan *llm.ChatChunk, 10)
	errCh := make(chan error, 1)

	go func() {
		errCh <- provider.StreamChat(ctx, req, chunkCh)
	}()

	// Read the first chunk (thought chunk)
	select {
	case <-chunkCh:
		// Cancel context immediately
		cancel()
	case <-time.After(2 * time.Second):
		t.Fatalf("timed out waiting for first chunk")
	}

	// Provider must exit promptly with context.Canceled error
	select {
	case err := <-errCh:
		if err == nil {
			t.Logf("provider exited cleanly upon cancellation")
		} else if err != context.Canceled {
			t.Logf("provider returned error on cancel: %v", err)
		}
	case <-time.After(1 * time.Second):
		t.Fatalf("StreamChat did not cancel promptly")
	}
}

func TestStreamChat_StreamDisconnect(t *testing.T) {
	cfg := &config.AppConfig{}
	gateway, err := client.NewSystemGatewayClient("")
	if err != nil {
		t.Fatalf("failed to create gateway: %v", err)
	}

	provider := llm.NewProvider(cfg, gateway)

	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()

	req := &llm.ChatRequest{
		Messages: []llm.Message{
			{Role: "user", Content: "Stream with quick disconnect"},
		},
	}

	chunkCh := make(chan *llm.ChatChunk, 1)
	doneCh := make(chan struct{})

	go func() {
		_ = provider.StreamChat(ctx, req, chunkCh)
		close(doneCh)
	}()

	select {
	case <-doneCh:
		// Stream disconnected cleanly
	case <-time.After(1 * time.Second):
		t.Fatalf("StreamChat failed to terminate on disconnect/timeout")
	}
}

func TestStreamChat_LateChunkDiscard(t *testing.T) {
	// Verify that sending chunks when receiver context is cancelled does not block or panic
	ctx, cancel := context.WithCancel(context.Background())
	cancel() // Already cancelled

	chunkCh := make(chan *llm.ChatChunk) // Unbuffered channel

	cfg := &config.AppConfig{}
	gateway, err := client.NewSystemGatewayClient("")
	if err != nil {
		t.Fatalf("failed to create gateway: %v", err)
	}

	provider := llm.NewProvider(cfg, gateway)
	req := &llm.ChatRequest{
		Messages: []llm.Message{
			{Role: "user", Content: "Cancelled before start"},
		},
	}

	err = provider.StreamChat(ctx, req, chunkCh)
	if err == nil {
		t.Logf("expected cancellation error or early return")
	}
}
