package run

import (
	"encoding/json/v2"
	"fmt"
	"net/http"
	"strings"

	"github.com/diezy-labs/claw-crew/engine/core/metrics"
)

// HTTPHandler handles HTTP REST and SSE endpoints for Runs
type HTTPHandler struct {
	service Service
}

// NewHTTPHandler creates a new HTTPHandler
func NewHTTPHandler(service Service) *HTTPHandler {
	return &HTTPHandler{
		service: service,
	}
}

// Service exposes the underlying run service (used for startup resume, F1-3).
func (h *HTTPHandler) Service() Service { return h.service }

// RegisterHTTP registers run endpoints on the metrics/HTTP server
func (h *HTTPHandler) RegisterHTTP(server *metrics.Server) {
	server.RegisterRouteFunc("/api/v1/runs", h.handleRunsRoot)
	server.RegisterRouteFunc("/api/v1/runs/", h.handleRunByID)
}

func (h *HTTPHandler) handleRunsRoot(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, `{"error":{"code":"METHOD_NOT_ALLOWED","message":"Method not allowed"}}`, http.StatusMethodNotAllowed)
		return
	}

	reqID := r.Header.Get("X-Request-ID")
	idempotencyKey := r.Header.Get("Idempotency-Key")

	var req CreateRunRequest
	if err := json.UnmarshalRead(r.Body, &req); err != nil {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusBadRequest)
		_, _ = w.Write([]byte(`{"error":{"code":"INVALID_ARGUMENT","message":"invalid JSON body"}}`))
		return
	}

	run, err := h.service.CreateRun(r.Context(), &req, reqID, idempotencyKey)
	if err != nil {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusBadRequest)
		_, _ = w.Write([]byte(fmt.Sprintf(`{"error":{"code":"FAILED","message":%q}}`, err.Error())))
		return
	}

	resp := CreateRunResponse{
		ID:        run.ID,
		CrewID:    run.CrewID,
		Status:    run.Status,
		EventsURL: fmt.Sprintf("/api/v1/runs/%s/events", run.ID),
		CreatedAt: run.CreatedAt,
	}

	w.Header().Set("Content-Type", "application/json")
	if reqID != "" {
		w.Header().Set("X-Request-ID", reqID)
	}
	w.WriteHeader(http.StatusAccepted)
	data, _ := json.Marshal(resp)
	_, _ = w.Write(data)
}

func (h *HTTPHandler) handleRunByID(w http.ResponseWriter, r *http.Request) {
	path := strings.TrimPrefix(r.URL.Path, "/api/v1/runs/")
	parts := strings.Split(path, "/")
	if len(parts) == 0 || parts[0] == "" {
		http.NotFound(w, r)
		return
	}

	runID := parts[0]

	// Check sub-routes: /events or /cancel
	if len(parts) > 1 {
		switch parts[1] {
		case "events":
			h.handleEvents(w, r, runID)
			return
		case "cancel":
			h.handleCancel(w, r, runID)
			return
		default:
			http.NotFound(w, r)
			return
		}
	}

	// GET /api/v1/runs/{run_id}
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}

	run, err := h.service.GetRun(r.Context(), runID)
	if err != nil {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusNotFound)
		_, _ = w.Write([]byte(fmt.Sprintf(`{"error":{"code":"RUN_NOT_FOUND","message":%q}}`, err.Error())))
		return
	}

	w.Header().Set("Content-Type", "application/json")
	data, _ := json.Marshal(run)
	_, _ = w.Write(data)
}

func (h *HTTPHandler) handleEvents(w http.ResponseWriter, r *http.Request, runID string) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}

	// Verify run exists
	if _, err := h.service.GetRun(r.Context(), runID); err != nil {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusNotFound)
		_, _ = w.Write([]byte(`{"error":{"code":"RUN_NOT_FOUND","message":"run not found","layer":"DELIVERY"}}`))
		return
	}

	flusher, ok := w.(http.Flusher)
	if !ok {
		http.Error(w, "streaming unsupported", http.StatusInternalServerError)
		return
	}

	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-cache")
	w.Header().Set("Connection", "keep-alive")
	w.WriteHeader(http.StatusOK)
	flusher.Flush()

	// Parse Last-Event-ID for reconnection deduplication (BUG-006)
	lastEventID := r.Header.Get("Last-Event-ID")
	eventCh, unsubscribe := h.service.SubscribeEventsSince(runID, lastEventID)
	defer unsubscribe()

	ctx := r.Context()
	for {
		select {
		case <-ctx.Done():
			return
		case evt, ok := <-eventCh:
			if !ok {
				return
			}
			data, err := json.Marshal(evt)
			if err == nil {
				fmt.Fprintf(w, "id: %s\nevent: %s\ndata: %s\n\n", evt.EventID, evt.Type, data)
				flusher.Flush()
			}
		}
	}
}

func (h *HTTPHandler) handleCancel(w http.ResponseWriter, r *http.Request, runID string) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}

	if err := h.service.CancelRun(r.Context(), runID); err != nil {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusBadRequest)
		_, _ = w.Write([]byte(fmt.Sprintf(`{"error":{"code":"FAILED_PRECONDITION","message":%q}}`, err.Error())))
		return
	}

	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write([]byte(`{"status":"cancelled"}`))
}
