package llm

import (
	"context"
	"encoding/json/v2"
	"fmt"
	"log/slog"
	"sync"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/core/logger"
	"github.com/diezy-labs/claw-crew/engine/pkg/client"
)

// ToolExecutionResult represents the output of a dispatched tool
type ToolExecutionResult struct {
	ToolCallID       string `json:"tool_call_id"`
	ToolName         string `json:"tool_name"`
	Output           string `json:"output"`
	IsSubagentAction bool   `json:"is_subagent_action"`
	SubagentID       string `json:"subagent_id,omitempty"`
	SubagentTask     string `json:"subagent_task,omitempty"`
	Error            error  `json:"-"`
}

// ToolDispatcher handles tool routing between native OS execution (via Rust) and agent delegation
type ToolDispatcher interface {
	GetAvailableTools() []ToolDefinition
	Dispatch(ctx context.Context, call *ToolCall) (*ToolExecutionResult, error)
	DispatchBatch(ctx context.Context, calls []ToolCall) ([]*ToolExecutionResult, error)
}

type toolDispatcher struct {
	gateway client.SystemGatewayClient
	tools   []ToolDefinition
}

// NewToolDispatcher constructs a tool dispatcher hooked into the SystemGatewayClient
func NewToolDispatcher(gateway client.SystemGatewayClient) ToolDispatcher {
	standardTools := []ToolDefinition{
		{
			Name:        "execute_command",
			Description: "Executes a shell command securely via Rust host gateway",
			Parameters: map[string]any{
				"type": "object",
				"properties": map[string]any{
					"command": map[string]any{
						"type":        "string",
						"description": "Shell command to run",
					},
					"cwd": map[string]any{
						"type":        "string",
						"description": "Current working directory",
					},
				},
				"required": []string{"command"},
			},
		},
		{
			Name:        "read_file",
			Description: "Reads the text contents of a file on the host filesystem",
			Parameters: map[string]any{
				"type": "object",
				"properties": map[string]any{
					"path": map[string]any{
						"type":        "string",
						"description": "Absolute path to file",
					},
				},
				"required": []string{"path"},
			},
		},
		{
			Name:        "write_file",
			Description: "Writes text content to a file on the host filesystem",
			Parameters: map[string]any{
				"type": "object",
				"properties": map[string]any{
					"path": map[string]any{
						"type":        "string",
						"description": "Absolute path to file",
					},
					"content": map[string]any{
						"type":        "string",
						"description": "Content to write",
					},
				},
				"required": []string{"path", "content"},
			},
		},
		{
			Name:        "spawn_subagent",
			Description: "Delegates a specific sub-task to a specialized sub-agent running concurrently",
			Parameters: map[string]any{
				"type": "object",
				"properties": map[string]any{
					"role": map[string]any{
						"type":        "string",
						"description": "Role/title of the sub-agent (e.g. Code Reviewer, Researcher, Tester)",
					},
					"task": map[string]any{
						"type":        "string",
						"description": "Specific task prompt for the sub-agent",
					},
				},
				"required": []string{"role", "task"},
			},
		},
		{
			Name:        "workspace.list_files",
			Description: "Lists files inside the authorized workspace boundary with depth limiting",
			Parameters: map[string]any{
				"type": "object",
				"properties": map[string]any{
					"path": map[string]any{
						"type":        "string",
						"description": "Relative directory path",
					},
					"max_depth": map[string]any{
						"type":        "integer",
						"description": "Maximum directory traversal depth",
					},
				},
			},
		},
		{
			Name:        "workspace.search_code",
			Description: "Searches workspace code files matching query or regex pattern",
			Parameters: map[string]any{
				"type": "object",
				"properties": map[string]any{
					"query": map[string]any{
						"type":        "string",
						"description": "Search query or regex",
					},
					"path": map[string]any{
						"type":        "string",
						"description": "Relative directory path",
					},
				},
				"required": []string{"query"},
			},
		},
		{
			Name:        "web.fetch",
			Description: "Safely fetches web documentation with SSRF and script stripping guards",
			Parameters: map[string]any{
				"type": "object",
				"properties": map[string]any{
					"url": map[string]any{
						"type":        "string",
						"description": "Target HTTP/HTTPS URL",
					},
				},
				"required": []string{"url"},
			},
		},
		{
			Name:        "code.run_linter",
			Description: "Runs a code linter in an isolated, environment-scrubbed workspace process",
			Parameters: map[string]any{
				"type": "object",
				"properties": map[string]any{
					"linter": map[string]any{
						"type":        "string",
						"description": "Linter executable (e.g. golangci-lint, cargo clippy, gofmt)",
					},
					"args": map[string]any{
						"type":        "array",
						"items":       map[string]any{"type": "string"},
						"description": "CLI flags or target packages",
					},
				},
				"required": []string{"linter"},
			},
		},
	}

	return &toolDispatcher{
		gateway: gateway,
		tools:   standardTools,
	}
}

func (d *toolDispatcher) GetAvailableTools() []ToolDefinition {
	return d.tools
}

type subagentArgs struct {
	Role string `json:"role"`
	Task string `json:"task"`
}

func (d *toolDispatcher) Dispatch(ctx context.Context, call *ToolCall) (*ToolExecutionResult, error) {
	log := logger.Get()
	log.InfoContext(ctx, "dispatching tool call",
		slog.String("id", call.ID),
		slog.String("tool", call.Name),
	)

	// Check if this is an internal sub-agent spawn request
	if call.Name == "spawn_subagent" {
		var args subagentArgs
		if err := json.Unmarshal([]byte(call.Arguments), &args); err != nil {
			return nil, appErrors.Wrap(err, appErrors.CodeInvalidArgument, "invalid arguments for spawn_subagent", appErrors.LayerService)
		}

		subagentID := fmt.Sprintf("subagent_%s_%s", args.Role, call.ID)
		return &ToolExecutionResult{
			ToolCallID:       call.ID,
			ToolName:         call.Name,
			Output:           fmt.Sprintf("Delegated task to subagent '%s'", subagentID),
			IsSubagentAction: true,
			SubagentID:       subagentID,
			SubagentTask:     args.Task,
		}, nil
	}

	// Dispatch OS / hardware level tools to the Rust System Gateway
	output, err := d.gateway.ExecuteNativeTool(ctx, call.Name, call.Arguments)
	if err != nil {
		log.ErrorContext(ctx, "failed executing native tool via Rust gateway",
			slog.String("tool", call.Name),
			slog.String("error", err.Error()),
		)
		return &ToolExecutionResult{
			ToolCallID: call.ID,
			ToolName:   call.Name,
			Output:     "",
			Error:      err,
		}, err
	}

	return &ToolExecutionResult{
		ToolCallID: call.ID,
		ToolName:   call.Name,
		Output:     output,
	}, nil
}

func (d *toolDispatcher) DispatchBatch(ctx context.Context, calls []ToolCall) ([]*ToolExecutionResult, error) {
	if len(calls) == 0 {
		return nil, nil
	}

	results := make([]*ToolExecutionResult, len(calls))
	var wg sync.WaitGroup
	errCh := make(chan error, len(calls))

	for i, c := range calls {
		wg.Add(1)
		go func(idx int, call ToolCall) {
			defer wg.Done()
			res, err := d.Dispatch(ctx, &call)
			if err != nil {
				errCh <- err
			}
			results[idx] = res
		}(i, c)
	}

	wg.Wait()
	close(errCh)

	// Return first encountered error if any
	for err := range errCh {
		if err != nil {
			return results, err
		}
	}

	return results, nil
}
