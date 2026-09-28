package tool

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os/exec"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/diezy-labs/claw-crew/engine/core/logger"
)

// JSON-RPC 2.0 messages
type jsonRPCRequest struct {
	JSONRPC string `json:"jsonrpc"`
	ID      int64  `json:"id"`
	Method  string `json:"method"`
	Params  any    `json:"params,omitempty"`
}

type jsonRPCResponse struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      int64           `json:"id"`
	Result  json.RawMessage `json:"result,omitempty"`
	Error   *jsonRPCError   `json:"error,omitempty"`
}

type jsonRPCError struct {
	Code    int    `json:"code"`
	Message string `json:"message"`
	Data    any    `json:"data,omitempty"`
}

// MCPClient defines operations for interacting with an MCP server
type MCPClient interface {
	Connect(ctx context.Context) error
	Close() error
	ListTools(ctx context.Context) ([]*ToolDefinition, error)
	CallTool(ctx context.Context, name string, arguments string) (string, error)
	ServerName() string
}

// MCPServerConfig defines configuration for an external MCP server
type MCPServerConfig struct {
	Name          string            `json:"name"`
	Transport     string            `json:"transport"` // "stdio" or "http" / "sse"
	Command       string            `json:"command,omitempty"`
	Args          []string          `json:"args,omitempty"`
	Env           map[string]string `json:"env,omitempty"`
	URL           string            `json:"url,omitempty"`
	ReadOnlyTools []string          `json:"read_only_tools,omitempty"`
}

// StdioMCPClient connects to an MCP server running as a child process over stdio
type StdioMCPClient struct {
	serverName string
	command    string
	args       []string
	env        map[string]string
	readOnly   map[string]bool

	cmd       *exec.Cmd
	stdin     io.WriteCloser
	stdout    *bufio.Scanner
	mu        sync.Mutex
	reqID     atomic.Int64
	pending   map[int64]chan *jsonRPCResponse
	pendingMu sync.Mutex
	closed    bool
}

// NewStdioMCPClient creates an MCP client over standard IO
func NewStdioMCPClient(cfg MCPServerConfig) *StdioMCPClient {
	roMap := make(map[string]bool)
	for _, t := range cfg.ReadOnlyTools {
		roMap[strings.ToLower(t)] = true
	}

	return &StdioMCPClient{
		serverName: cfg.Name,
		command:    cfg.Command,
		args:       cfg.Args,
		env:        cfg.Env,
		readOnly:   roMap,
		pending:    make(map[int64]chan *jsonRPCResponse),
	}
}

func (c *StdioMCPClient) ServerName() string {
	return c.serverName
}

func (c *StdioMCPClient) Connect(ctx context.Context) error {
	c.mu.Lock()
	defer c.mu.Unlock()

	if c.cmd != nil {
		return nil
	}

	cmd := exec.CommandContext(ctx, c.command, c.args...)
	cmd.Env = scrubbedEnvironment()
	for k, v := range c.env {
		cmd.Env = append(cmd.Env, fmt.Sprintf("%s=%s", k, v))
	}

	stdin, err := cmd.StdinPipe()
	if err != nil {
		return fmt.Errorf("failed to open stdin pipe: %w", err)
	}

	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return fmt.Errorf("failed to open stdout pipe: %w", err)
	}

	if err := cmd.Start(); err != nil {
		return fmt.Errorf("failed to start mcp process %s: %w", c.command, err)
	}

	c.cmd = cmd
	c.stdin = stdin
	c.stdout = bufio.NewScanner(stdout)

	// Background reader loop
	go c.readLoop()

	// Perform initialize handshake
	initReq := jsonRPCRequest{
		JSONRPC: "2.0",
		ID:      c.reqID.Add(1),
		Method:  "initialize",
		Params: map[string]any{
			"protocolVersion": "2024-11-05",
			"capabilities":    map[string]any{},
			"clientInfo": map[string]string{
				"name":    "clawcrew-engine",
				"version": "0.9.0",
			},
		},
	}

	_, err = c.sendRequest(ctx, initReq)
	if err != nil {
		return fmt.Errorf("mcp initialize failed: %w", err)
	}

	return nil
}

func (c *StdioMCPClient) readLoop() {
	for c.stdout.Scan() {
		line := c.stdout.Bytes()
		if len(line) == 0 {
			continue
		}

		var resp jsonRPCResponse
		if err := json.Unmarshal(line, &resp); err != nil {
			continue
		}

		c.pendingMu.Lock()
		ch, ok := c.pending[resp.ID]
		if ok {
			delete(c.pending, resp.ID)
		}
		c.pendingMu.Unlock()

		if ok {
			ch <- &resp
		}
	}
}

func (c *StdioMCPClient) sendRequest(ctx context.Context, req jsonRPCRequest) (*jsonRPCResponse, error) {
	reqBytes, err := json.Marshal(req)
	if err != nil {
		return nil, err
	}

	ch := make(chan *jsonRPCResponse, 1)
	c.pendingMu.Lock()
	c.pending[req.ID] = ch
	c.pendingMu.Unlock()

	c.mu.Lock()
	if c.closed || c.stdin == nil {
		c.mu.Unlock()
		return nil, fmt.Errorf("client closed")
	}
	_, err = c.stdin.Write(append(reqBytes, '\n'))
	c.mu.Unlock()

	if err != nil {
		c.pendingMu.Lock()
		delete(c.pending, req.ID)
		c.pendingMu.Unlock()
		return nil, err
	}

	select {
	case <-ctx.Done():
		c.pendingMu.Lock()
		delete(c.pending, req.ID)
		c.pendingMu.Unlock()
		return nil, ctx.Err()
	case resp := <-ch:
		if resp.Error != nil {
			return nil, fmt.Errorf("mcp rpc error %d: %s", resp.Error.Code, resp.Error.Message)
		}
		return resp, nil
	}
}

func (c *StdioMCPClient) ListTools(ctx context.Context) ([]*ToolDefinition, error) {
	callCtx, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()

	req := jsonRPCRequest{
		JSONRPC: "2.0",
		ID:      c.reqID.Add(1),
		Method:  "tools/list",
		Params:  map[string]any{},
	}

	resp, err := c.sendRequest(callCtx, req)
	if err != nil {
		return nil, err
	}

	var listResult struct {
		Tools []struct {
			Name        string          `json:"name"`
			Description string          `json:"description"`
			InputSchema json.RawMessage `json:"inputSchema"`
		} `json:"tools"`
	}

	if err := json.Unmarshal(resp.Result, &listResult); err != nil {
		return nil, fmt.Errorf("failed to parse tools/list result: %w", err)
	}

	var definitions []*ToolDefinition
	for _, t := range listResult.Tools {
		namespacedID := fmt.Sprintf("mcp.%s.%s", c.serverName, t.Name)

		// Untrusted metadata sanitization: prevent injection in tool descriptions
		sanitizedDesc := strings.ReplaceAll(t.Description, "\x00", "")
		if len(sanitizedDesc) > 500 {
			sanitizedDesc = sanitizedDesc[:500] + "..."
		}

		// Security baseline: MCP tools default to RiskTierWrite unless allowlisted
		tier := RiskTierWrite
		class := RiskClassExternalAction
		reqApproval := true

		if c.readOnly[strings.ToLower(t.Name)] || c.readOnly["*"] {
			tier = RiskTierRead
			class = RiskClassRead
			reqApproval = false
		}

		schema := t.InputSchema
		if len(schema) == 0 {
			schema = json.RawMessage(`{"type":"object"}`)
		}

		definitions = append(definitions, &ToolDefinition{
			ID:               namespacedID,
			Version:          "1.0.0",
			DisplayName:      fmt.Sprintf("%s (%s)", t.Name, c.serverName),
			Description:      sanitizedDesc,
			InputSchema:      schema,
			RiskTier:         tier,
			RiskClass:        class,
			Capabilities:     []string{fmt.Sprintf("mcp.%s", c.serverName)},
			RequiresApproval: reqApproval,
			TimeoutSeconds:   30,
			MaxOutputBytes:   50000,
			IdempotencyMode:  IdempotencySafe,
			Source:           fmt.Sprintf("mcp:%s", c.serverName),
		})
	}

	return definitions, nil
}

func (c *StdioMCPClient) CallTool(ctx context.Context, name string, arguments string) (string, error) {
	callCtx, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()

	var argsObj any
	if strings.TrimSpace(arguments) != "" {
		_ = json.Unmarshal([]byte(arguments), &argsObj)
	}
	if argsObj == nil {
		argsObj = map[string]any{}
	}

	req := jsonRPCRequest{
		JSONRPC: "2.0",
		ID:      c.reqID.Add(1),
		Method:  "tools/call",
		Params: map[string]any{
			"name":      name,
			"arguments": argsObj,
		},
	}

	resp, err := c.sendRequest(callCtx, req)
	if err != nil {
		return "", err
	}

	var callResult struct {
		Content []struct {
			Type string `json:"type"`
			Text string `json:"text"`
		} `json:"content"`
		IsError bool `json:"isError"`
	}

	if err := json.Unmarshal(resp.Result, &callResult); err != nil {
		return string(resp.Result), nil
	}

	var outParts []string
	for _, part := range callResult.Content {
		if part.Text != "" {
			outParts = append(outParts, part.Text)
		}
	}

	finalOutput := strings.Join(outParts, "\n")
	// Redact secrets in MCP server output
	redacted := logger.RedactString(finalOutput)

	if callResult.IsError {
		return redacted, fmt.Errorf("mcp tool execution error: %s", redacted)
	}

	return redacted, nil
}

func (c *StdioMCPClient) Close() error {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.closed = true

	if c.stdin != nil {
		_ = c.stdin.Close()
	}
	if c.cmd != nil && c.cmd.Process != nil {
		_ = c.cmd.Process.Kill()
	}
	return nil
}

// HTTPMCPClient connects to remote MCP server over HTTP/SSE
type HTTPMCPClient struct {
	serverName string
	url        string
	readOnly   map[string]bool
	client     *http.Client
	reqID      atomic.Int64
}

// NewHTTPMCPClient creates an HTTP MCP client
func NewHTTPMCPClient(cfg MCPServerConfig) *HTTPMCPClient {
	roMap := make(map[string]bool)
	for _, t := range cfg.ReadOnlyTools {
		roMap[strings.ToLower(t)] = true
	}

	return &HTTPMCPClient{
		serverName: cfg.Name,
		url:        cfg.URL,
		readOnly:   roMap,
		client: &http.Client{
			Timeout: 30 * time.Second,
		},
	}
}

func (h *HTTPMCPClient) ServerName() string {
	return h.serverName
}

func (h *HTTPMCPClient) Connect(ctx context.Context) error {
	return nil
}

func (h *HTTPMCPClient) Close() error {
	return nil
}

func (h *HTTPMCPClient) ListTools(ctx context.Context) ([]*ToolDefinition, error) {
	req := jsonRPCRequest{
		JSONRPC: "2.0",
		ID:      h.reqID.Add(1),
		Method:  "tools/list",
		Params:  map[string]any{},
	}

	resp, err := h.sendPost(ctx, req)
	if err != nil {
		return nil, err
	}

	var listResult struct {
		Tools []struct {
			Name        string          `json:"name"`
			Description string          `json:"description"`
			InputSchema json.RawMessage `json:"inputSchema"`
		} `json:"tools"`
	}

	if err := json.Unmarshal(resp.Result, &listResult); err != nil {
		return nil, fmt.Errorf("failed to parse remote tools/list: %w", err)
	}

	var definitions []*ToolDefinition
	for _, t := range listResult.Tools {
		namespacedID := fmt.Sprintf("mcp.%s.%s", h.serverName, t.Name)
		tier := RiskTierWrite
		class := RiskClassExternalAction
		reqApproval := true
		if h.readOnly[strings.ToLower(t.Name)] || h.readOnly["*"] {
			tier = RiskTierRead
			class = RiskClassRead
			reqApproval = false
		}

		definitions = append(definitions, &ToolDefinition{
			ID:               namespacedID,
			Version:          "1.0.0",
			DisplayName:      fmt.Sprintf("%s (%s)", t.Name, h.serverName),
			Description:      t.Description,
			InputSchema:      t.InputSchema,
			RiskTier:         tier,
			RiskClass:        class,
			Capabilities:     []string{fmt.Sprintf("mcp.%s", h.serverName)},
			RequiresApproval: reqApproval,
			TimeoutSeconds:   30,
			MaxOutputBytes:   50000,
			IdempotencyMode:  IdempotencySafe,
			Source:           fmt.Sprintf("mcp:%s", h.serverName),
		})
	}

	return definitions, nil
}

func (h *HTTPMCPClient) CallTool(ctx context.Context, name string, arguments string) (string, error) {
	var argsObj any
	if strings.TrimSpace(arguments) != "" {
		_ = json.Unmarshal([]byte(arguments), &argsObj)
	}
	if argsObj == nil {
		argsObj = map[string]any{}
	}

	req := jsonRPCRequest{
		JSONRPC: "2.0",
		ID:      h.reqID.Add(1),
		Method:  "tools/call",
		Params: map[string]any{
			"name":      name,
			"arguments": argsObj,
		},
	}

	resp, err := h.sendPost(ctx, req)
	if err != nil {
		return "", err
	}

	return logger.RedactString(string(resp.Result)), nil
}

func (h *HTTPMCPClient) sendPost(ctx context.Context, req jsonRPCRequest) (*jsonRPCResponse, error) {
	data, err := json.Marshal(req)
	if err != nil {
		return nil, err
	}

	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, h.url, bytes.NewReader(data))
	if err != nil {
		return nil, err
	}
	httpReq.Header.Set("Content-Type", "application/json")

	resp, err := h.client.Do(httpReq)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, err
	}

	var rpcResp jsonRPCResponse
	if err := json.Unmarshal(body, &rpcResp); err != nil {
		return nil, fmt.Errorf("invalid json-rpc response: %w", err)
	}

	if rpcResp.Error != nil {
		return nil, fmt.Errorf("mcp rpc error %d: %s", rpcResp.Error.Code, rpcResp.Error.Message)
	}

	return &rpcResp, nil
}

// MCPToolAdapter wraps an MCP tool into the native Tool interface
type MCPToolAdapter struct {
	client     MCPClient
	definition *ToolDefinition
	rawName    string
}

// NewMCPToolAdapter wraps an MCP tool definition into a Tool
func NewMCPToolAdapter(client MCPClient, def *ToolDefinition, rawName string) Tool {
	return &MCPToolAdapter{
		client:     client,
		definition: def,
		rawName:    rawName,
	}
}

func (m *MCPToolAdapter) Name() string                { return m.definition.ID }
func (m *MCPToolAdapter) Description() string         { return m.definition.Description }
func (m *MCPToolAdapter) RiskTier() RiskTier          { return m.definition.RiskTier }
func (m *MCPToolAdapter) Definition() *ToolDefinition { return m.definition }
func (m *MCPToolAdapter) Execute(ctx context.Context, args string, workspaceRoot string) (string, error) {
	return m.client.CallTool(ctx, m.rawName, args)
}
func (m *MCPToolAdapter) ExecuteWithContext(ctx context.Context, execCtx *ExecutionContext, args string) (string, []string, error) {
	out, err := m.client.CallTool(ctx, m.rawName, args)
	return out, nil, err
}
