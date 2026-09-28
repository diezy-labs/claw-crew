package llm

import (
	"context"
	"fmt"
	"math"
	"sync"
	"time"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/core/logger"
	"github.com/diezy-labs/claw-crew/engine/core/metrics"
)

type multiProvider struct {
	mu        sync.RWMutex
	providers map[string]Provider
	fallback  string
}

// NewMultiProvider creates a multi-provider dispatcher
func NewMultiProvider(fallback string) MultiProvider {
	mp := &multiProvider{
		providers: make(map[string]Provider),
		fallback:  fallback,
	}
	mp.Register("mock", NewMockProvider("mock"))
	return mp
}

func (mp *multiProvider) Register(name string, p Provider) {
	mp.mu.Lock()
	defer mp.mu.Unlock()
	mp.providers[name] = p
}

func (mp *multiProvider) GetProvider(name string) (Provider, error) {
	mp.mu.RLock()
	defer mp.mu.RUnlock()

	if p, ok := mp.providers[name]; ok {
		return p, nil
	}

	// Try fallback
	if mp.fallback != "" {
		if p, ok := mp.providers[mp.fallback]; ok {
			return p, nil
		}
	}

	// Default to mock if available
	if p, ok := mp.providers["mock"]; ok {
		return p, nil
	}

	return nil, appErrors.New(appErrors.CodeNotFound, fmt.Sprintf("provider not found: %s", name), appErrors.LayerService)
}

func (mp *multiProvider) StreamWithRetry(ctx context.Context, providerName string, req *ChatRequest, chunkCh chan<- *ChatChunk) error {
	log := logger.Get()
	p, err := mp.GetProvider(providerName)
	if err != nil {
		return err
	}

	const maxRetries = 3
	var lastErr error

	for attempt := 0; attempt < maxRetries; attempt++ {
		select {
		case <-ctx.Done():
			return ctx.Err()
		default:
		}

		// Intercept chunks to record token usage metrics into Prometheus
		interceptCh := make(chan *ChatChunk, 16)
		errCh := make(chan error, 1)

		go func() {
			defer close(interceptCh)
			errCh <- p.StreamChat(ctx, req, interceptCh)
		}()

		var streamFailed bool
		for chunk := range interceptCh {
			if chunk.Usage != nil {
				// Record metrics
				metrics.LLMTokenUsage.WithLabelValues(p.Name(), req.Model, "prompt").Add(float64(chunk.Usage.PromptTokens))
				metrics.LLMTokenUsage.WithLabelValues(p.Name(), req.Model, "completion").Add(float64(chunk.Usage.CompletionTokens))
			}
			chunkCh <- chunk
		}

		lastErr = <-errCh
		if lastErr == nil {
			return nil
		}

		streamFailed = true
		if streamFailed {
			backoff := time.Duration(math.Pow(2, float64(attempt))) * 50 * time.Millisecond
			log.WarnContext(ctx, "LLM streaming failed, retrying with exponential backoff",
				"provider", p.Name(),
				"attempt", attempt+1,
				"backoff", backoff.String(),
				"error", lastErr.Error(),
			)

			select {
			case <-ctx.Done():
				return ctx.Err()
			case <-time.After(backoff):
			}
		}
	}

	// Try fallback provider if primary exhausted retries
	if mp.fallback != "" && mp.fallback != providerName {
		log.WarnContext(ctx, "primary LLM provider exhausted retries, switching to fallback",
			"primary", providerName,
			"fallback", mp.fallback,
		)
		if fallbackP, err := mp.GetProvider(mp.fallback); err == nil {
			return fallbackP.StreamChat(ctx, req, chunkCh)
		}
	}

	return lastErr
}
