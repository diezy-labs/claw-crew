package crew_test

import (
	"context"
	"fmt"
	"runtime"
	"sync"
	"testing"
	"time"

	"github.com/diezy-labs/claw-crew/engine/core/config"
	"github.com/diezy-labs/claw-crew/engine/pkg/client"
	"github.com/diezy-labs/claw-crew/engine/src/crew"
	"github.com/diezy-labs/claw-crew/engine/src/llm"
)

func TestMultiAgentConcurrencyStress(t *testing.T) {
	cfg := &config.AppConfig{}
	gateway, err := client.NewSystemGatewayClient("")
	if err != nil {
		t.Fatalf("failed to create gateway client: %v", err)
	}

	provider := llm.NewProvider(cfg, gateway)
	dispatcher := llm.NewToolDispatcher(gateway)
	orchestrator := crew.NewService(provider, dispatcher, gateway)

	const concurrentAgents = 50
	var wg sync.WaitGroup
	errCh := make(chan error, concurrentAgents)

	initialGoroutines := runtime.NumGoroutine()
	start := time.Now()

	for i := 0; i < concurrentAgents; i++ {
		wg.Add(1)
		go func(agentIdx int) {
			defer wg.Done()

			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()

			sessionID := fmt.Sprintf("stress_session_%d", agentIdx)
			agentID := fmt.Sprintf("agent_%d", agentIdx)
			prompt := fmt.Sprintf("Execute parallel agent evaluation #%d with subagent delegation", agentIdx)

			eventCh := make(chan *crew.TurnEvent, 64)
			turnErrCh := make(chan error, 1)

			go func() {
				defer close(eventCh)
				turnErrCh <- orchestrator.StartTurn(ctx, &crew.TurnRequest{
					SessionID: sessionID,
					AgentID:   agentID,
					Prompt:    prompt,
				}, eventCh)
			}()

			var gotCompleted bool
			for event := range eventCh {
				if event.Type == crew.EventTurnCompleted {
					gotCompleted = true
				}
			}

			if err := <-turnErrCh; err != nil {
				errCh <- fmt.Errorf("agent %d turn error: %w", agentIdx, err)
				return
			}

			if !gotCompleted {
				errCh <- fmt.Errorf("agent %d did not receive TurnCompleted event", agentIdx)
			}
		}(i)
	}

	wg.Wait()
	close(errCh)

	for err := range errCh {
		if err != nil {
			t.Errorf("stress test failure: %v", err)
		}
	}

	// Wait briefly for all finished goroutines to wind down
	time.Sleep(50 * time.Millisecond)
	finalGoroutines := runtime.NumGoroutine()

	elapsed := time.Since(start)
	t.Logf("Successfully executed %d concurrent agent turns in %v (goroutines: initial=%d, final=%d)",
		concurrentAgents, elapsed, initialGoroutines, finalGoroutines)

	// Ensure no runaway goroutine leak (allow slight delta for runtime gc/timer workers)
	if finalGoroutines > initialGoroutines+10 {
		t.Errorf("possible goroutine leak: initial=%d, final=%d", initialGoroutines, finalGoroutines)
	}
}
