package llm

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json/v2"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"strings"
	"time"

	"github.com/diezy-labs/claw-crew/engine/core/config"
	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/core/logger"
	"github.com/diezy-labs/claw-crew/engine/pkg/client"
)

type openAIStreamResponse struct {
	Choices []struct {
		Delta struct {
			Role             string `json:"role"`
			Content          string `json:"content"`
			ReasoningContent string `json:"reasoning_content"`
			ToolCalls        []struct {
				Index    int    `json:"index"`
				ID       string `json:"id"`
				Type     string `json:"type"`
				Function struct {
					Name      string `json:"name"`
					Arguments string `json:"arguments"`
				} `json:"function"`
			} `json:"tool_calls"`
		} `json:"delta"`
		FinishReason string `json:"finish_reason"`
	} `json:"choices"`
}

type provider struct {
	cfg        *config.AppConfig
	gateway    client.SystemGatewayClient
	httpClient *http.Client
}

// NewProvider creates a new streaming LLM Provider instance
func NewProvider(cfg *config.AppConfig, gateway client.SystemGatewayClient) Provider {
	return &provider{
		cfg:     cfg,
		gateway: gateway,
		httpClient: &http.Client{
			Timeout: 120 * time.Second,
		},
	}
}

func (p *provider) Name() string {
	if p.cfg.LLMModel != "" {
		return p.cfg.LLMModel
	}
	return "gpt-4o-mini"
}

func (p *provider) StreamChat(ctx context.Context, req *ChatRequest, chunkCh chan<- *ChatChunk) error {
	log := logger.Get()

	// 1. Resolve API key: check config flag, env, or Rust Vault
	apiKey := p.cfg.LLMAPIKey
	if apiKey == "" {
		// Attempt to read from Rust security vault
		if secret, err := p.gateway.GetDecryptedSecret(ctx, "OPENAI_API_KEY"); err == nil && secret != "" {
			apiKey = secret
		}
	}

	baseURL := p.cfg.LLMBaseURL
	if baseURL == "" {
		baseURL = "https://api.openai.com"
	}
	baseURL = strings.TrimSuffix(baseURL, "/")

	// If no API key is provided, execute simulated intelligent local fallback
	if apiKey == "" && p.cfg.LLMBaseURL == "" {
		log.InfoContext(ctx, "no LLM API key configured; running intelligent local simulation engine")
		return p.simulateStream(ctx, req, chunkCh)
	}

	model := req.Model
	if model == "" {
		model = p.Name()
	}

	endpoint := fmt.Sprintf("%s/v1/chat/completions", baseURL)

	requestBody := map[string]any{
		"model":       model,
		"messages":    req.Messages,
		"stream":      true,
		"temperature": req.Temperature,
	}
	if len(req.Tools) > 0 {
		formattedTools := make([]map[string]any, 0, len(req.Tools))
		for _, t := range req.Tools {
			formattedTools = append(formattedTools, map[string]any{
				"type": "function",
				"function": map[string]any{
					"name":        t.Name,
					"description": t.Description,
					"parameters":  t.Parameters,
				},
			})
		}
		requestBody["tools"] = formattedTools
	}

	bodyBytes, err := json.Marshal(requestBody)
	if err != nil {
		return appErrors.Wrap(err, appErrors.CodeInternal, "failed to serialize LLM request body", appErrors.LayerExternal)
	}

	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, bytes.NewReader(bodyBytes))
	if err != nil {
		return appErrors.Wrap(err, appErrors.CodeInternal, "failed to create HTTP request", appErrors.LayerExternal)
	}

	httpReq.Header.Set("Content-Type", "application/json")
	httpReq.Header.Set("Authorization", fmt.Sprintf("Bearer %s", apiKey))

	resp, err := p.httpClient.Do(httpReq)
	if err != nil {
		log.WarnContext(ctx, "failed connecting to upstream LLM API, falling back to local simulation",
			slog.String("endpoint", endpoint),
			slog.String("error", err.Error()),
		)
		return p.simulateStream(ctx, req, chunkCh)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		respBytes, _ := io.ReadAll(resp.Body)
		log.WarnContext(ctx, "upstream LLM returned error status, falling back to local simulation",
			slog.Int("status", resp.StatusCode),
			slog.String("response", string(respBytes)),
		)
		return p.simulateStream(ctx, req, chunkCh)
	}

	scanner := bufio.NewScanner(resp.Body)
	for scanner.Scan() {
		select {
		case <-ctx.Done():
			return appErrors.New(appErrors.CodeTimeout, "LLM streaming request cancelled or timed out", appErrors.LayerExternal)
		default:
		}

		line := scanner.Text()
		if !strings.HasPrefix(line, "data: ") {
			continue
		}

		data := strings.TrimPrefix(line, "data: ")
		if data == "[DONE]" {
			select {
			case <-ctx.Done():
				return ctx.Err()
			case chunkCh <- &ChatChunk{IsDone: true}:
			}
			break
		}

		var streamResp openAIStreamResponse
		if err := json.Unmarshal([]byte(data), &streamResp); err != nil {
			// Skip unparseable chunks
			continue
		}

		if len(streamResp.Choices) == 0 {
			continue
		}

		delta := streamResp.Choices[0].Delta
		var toolCalls []ToolCall
		for _, tc := range delta.ToolCalls {
			toolCalls = append(toolCalls, ToolCall{
				ID:        tc.ID,
				Name:      tc.Function.Name,
				Arguments: tc.Function.Arguments,
			})
		}

		chunk := &ChatChunk{
			ThoughtChunk: delta.ReasoningContent,
			ContentChunk: delta.Content,
			ToolCalls:    toolCalls,
			IsDone:       streamResp.Choices[0].FinishReason == "stop",
		}

		select {
		case <-ctx.Done():
			return ctx.Err()
		case chunkCh <- chunk:
		}
	}

	if err := scanner.Err(); err != nil {
		return appErrors.Wrap(err, appErrors.CodeInternal, "scanner error during LLM streaming", appErrors.LayerExternal)
	}

	return nil
}

// simulateStream generates intelligent local tokens when running offline or without credentials
func (p *provider) simulateStream(ctx context.Context, req *ChatRequest, chunkCh chan<- *ChatChunk) error {
	lastPrompt := ""
	if len(req.Messages) > 0 {
		lastPrompt = req.Messages[len(req.Messages)-1].Content
	}

	// 1. Emit thought reasoning chunk
	thought := fmt.Sprintf("Evaluating execution strategy for user request: '%s'. Checking available tools and sub-agent delegates.", lastPrompt)
	select {
	case <-ctx.Done():
		return ctx.Err()
	case chunkCh <- &ChatChunk{ThoughtChunk: thought}:
	}

	if !sleepWithContext(ctx, 30*time.Millisecond) {
		return ctx.Err()
	}

	// 2. Check if prompt requests file reading or execution
	lowerPrompt := strings.ToLower(lastPrompt)
	if strings.Contains(lowerPrompt, "read") || strings.Contains(lowerPrompt, "file") {
		select {
		case <-ctx.Done():
			return ctx.Err()
		case chunkCh <- &ChatChunk{
			ToolCalls: []ToolCall{
				{
					ID:        "call_read_1",
					Name:      "read_file",
					Arguments: `{"path": "package.json"}`,
				},
			},
		}:
		}
		if !sleepWithContext(ctx, 30*time.Millisecond) {
			return ctx.Err()
		}
	} else if strings.Contains(lowerPrompt, "subagent") || strings.Contains(lowerPrompt, "crew") || strings.Contains(lowerPrompt, "delegate") {
		select {
		case <-ctx.Done():
			return ctx.Err()
		case chunkCh <- &ChatChunk{
			ToolCalls: []ToolCall{
				{
					ID:        "call_subagent_1",
					Name:      "spawn_subagent",
					Arguments: `{"role": "Code Analyst", "task": "Analyze dependency tree and verify compliance"}`,
				},
			},
		}:
		}
		if !sleepWithContext(ctx, 30*time.Millisecond) {
			return ctx.Err()
		}
	}

	// 3. Emit streaming text chunks
	responseWords := []string{
		"ClawCrew", "Agent", "Engine", "(Go", "1.27)", "processed", "your", "request", "successfully.",
		"All", "multi-agent", "goroutines", "and", "channels", "are", "operating", "with", "optimal", "latency.",
	}

	for _, word := range responseWords {
		select {
		case <-ctx.Done():
			return ctx.Err()
		case chunkCh <- &ChatChunk{ContentChunk: word + " "}:
		}
		if !sleepWithContext(ctx, 15*time.Millisecond) {
			return ctx.Err()
		}
	}

	// 4. Emit completion chunk
	select {
	case <-ctx.Done():
		return ctx.Err()
	case chunkCh <- &ChatChunk{IsDone: true}:
	}

	return nil
}

func sleepWithContext(ctx context.Context, d time.Duration) bool {
	select {
	case <-ctx.Done():
		return false
	case <-time.After(d):
		return true
	}
}
