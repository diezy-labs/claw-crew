package crew

import (
	"context"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/pkg/pb"
	"google.golang.org/grpc"
)

// GRPCHandler implements pb.AgentEngineServer
type GRPCHandler struct {
	pb.UnimplementedAgentEngineServer
	orchestrator Orchestrator
}

// NewGRPCHandler constructs a new GRPCHandler instance
func NewGRPCHandler(orchestrator Orchestrator) *GRPCHandler {
	return &GRPCHandler{
		orchestrator: orchestrator,
	}
}

// RegisterService registers this handler with the grpc.Server
func (h *GRPCHandler) RegisterService(server *grpc.Server) {
	pb.RegisterAgentEngineServer(server, h)
}

// StartTurn handles streaming turn requests initiated from the Rust gateway
func (h *GRPCHandler) StartTurn(req *pb.TurnRequest, stream pb.AgentEngine_StartTurnServer) error {
	ctx := stream.Context()

	dtoReq := &TurnRequest{
		SessionID:     req.GetSessionId(),
		AgentID:       req.GetAgentId(),
		Prompt:        req.GetPrompt(),
		ContextWindow: req.GetContextWindow(),
	}

	eventCh := make(chan *TurnEvent, 16)
	errCh := make(chan error, 1)

	go func() {
		defer close(eventCh)
		errCh <- h.orchestrator.StartTurn(ctx, dtoReq, eventCh)
	}()

	for event := range eventCh {
		var protoType pb.TurnResponse_EventType
		switch event.Type {
		case EventThoughtChunk:
			protoType = pb.TurnResponse_THOUGHT_CHUNK
		case EventTextChunk:
			protoType = pb.TurnResponse_TEXT_CHUNK
		case EventToolCallStarted:
			protoType = pb.TurnResponse_TOOL_CALL_STARTED
		case EventToolCallFinished:
			protoType = pb.TurnResponse_TOOL_CALL_FINISHED
		case EventSubagentSpawned:
			protoType = pb.TurnResponse_SUBAGENT_SPAWNED
		case EventTurnCompleted:
			protoType = pb.TurnResponse_TURN_COMPLETED
		default:
			protoType = pb.TurnResponse_ERROR
		}

		resp := &pb.TurnResponse{
			Type:         protoType,
			Content:      event.Content,
			SubagentId:   event.SubagentID,
			ErrorMessage: event.ErrorMessage,
		}

		if err := stream.Send(resp); err != nil {
			return appErrors.Wrap(err, appErrors.CodeInternal, "failed to stream response to client", appErrors.LayerDelivery)
		}
	}

	return <-errCh
}

// QuickQuery executes lightweight stateless queries (e.g. vector search lookups)
func (h *GRPCHandler) QuickQuery(ctx context.Context, req *pb.QueryRequest) (*pb.QueryResponse, error) {
	return &pb.QueryResponse{
		Matches: []*pb.QueryMatch{},
	}, nil
}

// HealthCheck verifies availability of the AgentEngine gRPC service
func (h *GRPCHandler) HealthCheck(ctx context.Context, req *pb.HealthCheckRequest) (*pb.HealthCheckResponse, error) {
	return &pb.HealthCheckResponse{
		Status: pb.HealthCheckResponse_SERVING,
	}, nil
}
