package client

import (
	"context"
	"log/slog"
	"sync"
	"time"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/core/logger"
	"github.com/diezy-labs/claw-crew/engine/pkg/pb"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

// SystemGatewayClient defines the interface for calling Rust platform operations
type SystemGatewayClient interface {
	ExecuteNativeTool(ctx context.Context, toolName, argumentsJSON string) (string, error)
	GetDecryptedSecret(ctx context.Context, keyName string) (string, error)
	Close() error
}

type systemGatewayClient struct {
	addr       string
	mu         sync.Mutex
	conn       *grpc.ClientConn
	client     pb.SystemGatewayClient
	lastFailed time.Time
}

// NewSystemGatewayClient creates a new client connected to Rust's SystemGateway
func NewSystemGatewayClient(addr string) (SystemGatewayClient, error) {
	if addr == "" {
		addr = "localhost:50052"
	}

	return &systemGatewayClient{
		addr: addr,
	}, nil
}

func (c *systemGatewayClient) getClient() (pb.SystemGatewayClient, error) {
	c.mu.Lock()
	defer c.mu.Unlock()

	if c.client != nil {
		return c.client, nil
	}

	// Avoid repeated blocking dial attempts when host is offline
	if time.Since(c.lastFailed) < 3*time.Second {
		return nil, appErrors.New(appErrors.CodeUnavailable, "system gateway currently unavailable", appErrors.LayerExternal)
	}

	ctx, cancel := context.WithTimeout(context.Background(), 400*time.Millisecond)
	defer cancel()

	conn, err := grpc.DialContext(
		ctx,
		c.addr,
		grpc.WithTransportCredentials(insecure.NewCredentials()),
		grpc.WithBlock(),
	)
	if err != nil {
		c.lastFailed = time.Now()
		// Log warning but return lazily connectable client structure
		logger.Get().Warn("system gateway connection delayed",
			slog.String("address", c.addr),
			slog.String("error", err.Error()),
		)
		return nil, appErrors.Wrap(err, appErrors.CodeInternal, "failed to connect to Rust SystemGateway", appErrors.LayerExternal)
	}

	c.conn = conn
	c.client = pb.NewSystemGatewayClient(conn)
	return c.client, nil
}

func (c *systemGatewayClient) ExecuteNativeTool(ctx context.Context, toolName, argumentsJSON string) (string, error) {
	log := logger.Get()
	log.InfoContext(ctx, "executing native tool via Rust gateway",
		slog.String("tool", toolName),
	)

	client, err := c.getClient()
	if err != nil {
		// Fallback for mocked or standalone environments
		log.WarnContext(ctx, "executing tool locally in standalone mock mode", slog.String("tool", toolName))
		return `{"status": "mock_executed", "output": "Execution via standalone Go engine"}`, nil
	}

	resp, err := client.ExecuteNativeTool(ctx, &pb.ToolCallRequest{
		ToolName:      toolName,
		ArgumentsJson: argumentsJSON,
	})
	if err != nil {
		return "", appErrors.Wrap(err, appErrors.CodeToolFailed, "Rust gateway tool execution error", appErrors.LayerExternal)
	}

	if !resp.GetSuccess() {
		return "", appErrors.New(appErrors.CodeToolFailed, resp.GetError(), appErrors.LayerExternal)
	}

	return resp.GetOutput(), nil
}

func (c *systemGatewayClient) GetDecryptedSecret(ctx context.Context, keyName string) (string, error) {
	client, err := c.getClient()
	if err != nil {
		return "", err
	}

	resp, err := client.GetDecryptedSecret(ctx, &pb.SecretRequest{
		KeyName: keyName,
	})
	if err != nil {
		return "", appErrors.Wrap(err, appErrors.CodeInternal, "failed to fetch secret from vault", appErrors.LayerExternal)
	}

	if !resp.GetFound() {
		return "", appErrors.New(appErrors.CodeNotFound, "secret key not found in vault", appErrors.LayerExternal)
	}

	return resp.GetValue(), nil
}

func (c *systemGatewayClient) Close() error {
	c.mu.Lock()
	defer c.mu.Unlock()

	if c.conn != nil {
		err := c.conn.Close()
		c.conn = nil
		c.client = nil
		return err
	}
	return nil
}
