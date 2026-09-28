package crew

import (
	"context"
	"encoding/json/v2"
	"fmt"
	"net/http"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/core/metrics"
	"github.com/diezy-labs/claw-crew/engine/pkg/pb"
	"github.com/diezy-labs/claw-crew/engine/src/memory"
	"google.golang.org/grpc"
	"strings"
)

// GRPCHandler implements pb.AgentEngineServer and provides HTTP routes
type GRPCHandler struct {
	pb.UnimplementedAgentEngineServer
	orchestrator Orchestrator
	vectorStore  memory.VectorStore
	registry     Registry
}

// NewGRPCHandler constructs a new GRPCHandler instance
func NewGRPCHandler(orchestrator Orchestrator, vectorStore memory.VectorStore, registry Registry) *GRPCHandler {
	return &GRPCHandler{
		orchestrator: orchestrator,
		vectorStore:  vectorStore,
		registry:     registry,
	}
}

// RegisterService registers this handler with the grpc.Server
func (h *GRPCHandler) RegisterService(server *grpc.Server) {
	pb.RegisterAgentEngineServer(server, h)
}

// RegisterHTTP registers HTTP streaming and query endpoints on the metrics server
func (h *GRPCHandler) RegisterHTTP(server *metrics.Server) {
	server.RegisterRouteFunc("/api/turn", h.handleHTTPTurn)
	server.RegisterRouteFunc("/api/query", h.handleHTTPQuery)
	server.RegisterRouteFunc("/api/v1/crews", h.handleCrews)
	server.RegisterRouteFunc("/api/v1/crews/", h.handleCrewByID)
}

func (h *GRPCHandler) handleCrews(w http.ResponseWriter, r *http.Request) {
	if r.Method == http.MethodGet {
		crews, err := h.registry.ListCrews(r.Context())
		if err != nil {
			http.Error(w, err.Error(), http.StatusInternalServerError)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		data, _ := json.Marshal(ListCrewsResponse{Items: crews})
		_, _ = w.Write(data)
		return
	}
	http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
}

func (h *GRPCHandler) handleCrewByID(w http.ResponseWriter, r *http.Request) {
	crewID := strings.TrimPrefix(r.URL.Path, "/api/v1/crews/")
	if crewID == "" {
		http.NotFound(w, r)
		return
	}
	if r.Method == http.MethodGet {
		c, err := h.registry.GetCrew(r.Context(), crewID)
		if err != nil {
			http.Error(w, "crew not found", http.StatusNotFound)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		data, _ := json.Marshal(c)
		_, _ = w.Write(data)
		return
	}
	if r.Method == http.MethodPut {
		var c CrewDefinition
		if err := json.UnmarshalRead(r.Body, &c); err != nil {
			http.Error(w, "invalid request body", http.StatusBadRequest)
			return
		}
		c.ID = crewID
		if err := h.registry.RegisterCrew(r.Context(), &c); err != nil {
			http.Error(w, err.Error(), http.StatusInternalServerError)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		data, _ := json.Marshal(c)
		_, _ = w.Write(data)
		return
	}
	http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
}

func (h *GRPCHandler) handleHTTPTurn(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}

	var req TurnRequest
	if err := json.UnmarshalRead(r.Body, &req); err != nil {
		http.Error(w, "invalid request body", http.StatusBadRequest)
		return
	}

	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-cache")
	w.Header().Set("Connection", "keep-alive")

	flusher, ok := w.(http.Flusher)
	if !ok {
		http.Error(w, "streaming unsupported", http.StatusInternalServerError)
		return
	}

	eventCh := make(chan *TurnEvent, 16)
	errCh := make(chan error, 1)

	ctx := r.Context()
	go func() {
		defer close(eventCh)
		errCh <- h.orchestrator.StartTurn(ctx, &req, eventCh)
	}()

	for event := range eventCh {
		data, err := json.Marshal(event)
		if err == nil {
			fmt.Fprintf(w, "data: %s\n\n", data)
			flusher.Flush()
		}
	}

	if err := <-errCh; err != nil {
		errData, _ := json.Marshal(map[string]string{"error": err.Error()})
		fmt.Fprintf(w, "event: error\ndata: %s\n\n", errData)
		flusher.Flush()
	}
}

func (h *GRPCHandler) handleHTTPQuery(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}

	var req struct {
		Query string `json:"query"`
		TopK  int    `json:"top_k"`
	}
	if err := json.UnmarshalRead(r.Body, &req); err != nil {
		http.Error(w, "invalid request body", http.StatusBadRequest)
		return
	}

	topK := req.TopK
	if topK <= 0 {
		topK = 5
	}

	results, err := h.vectorStore.SearchByText(r.Context(), req.Query, topK)
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}

	w.Header().Set("Content-Type", "application/json")
	data, err := json.Marshal(results)
	if err == nil {
		_, _ = w.Write(data)
	}
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
	if req.GetQuery() == "" {
		return &pb.QueryResponse{
			Matches: []*pb.QueryMatch{},
		}, nil
	}

	topK := int(req.GetTopK())
	if topK <= 0 {
		topK = 5
	}

	results, err := h.vectorStore.SearchByText(ctx, req.GetQuery(), topK)
	if err != nil {
		return nil, appErrors.Wrap(err, appErrors.CodeInternal, "failed to execute vector similarity search", appErrors.LayerDelivery)
	}

	matches := make([]*pb.QueryMatch, 0, len(results))
	for _, r := range results {
		matches = append(matches, &pb.QueryMatch{
			Id:       r.Document.ID,
			Content:  r.Document.Content,
			Score:    r.Score,
			Metadata: r.Document.Metadata,
		})
	}

	return &pb.QueryResponse{
		Matches: matches,
	}, nil
}

// HealthCheck verifies availability of the AgentEngine gRPC service
func (h *GRPCHandler) HealthCheck(ctx context.Context, req *pb.HealthCheckRequest) (*pb.HealthCheckResponse, error) {
	return &pb.HealthCheckResponse{
		Status: pb.HealthCheckResponse_SERVING,
	}, nil
}
