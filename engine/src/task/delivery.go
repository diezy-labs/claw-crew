package task

import (
	"context"
	"encoding/json/v2"
	"fmt"
	"net/http"
	"strings"

	"github.com/diezy-labs/claw-crew/engine/core/metrics"
)

// HTTPHandler handles HTTP endpoints for tasks
type HTTPHandler struct {
	service Service
}

// NewHTTPHandler creates a new HTTPHandler
func NewHTTPHandler(service Service) *HTTPHandler {
	return &HTTPHandler{
		service: service,
	}
}

// RegisterHTTP registers task endpoints on the metrics server
func (h *HTTPHandler) RegisterHTTP(server *metrics.Server) {
	server.RegisterRouteFunc("/api/v1/tasks/", h.handleTasks)
}

func (h *HTTPHandler) handleTasks(w http.ResponseWriter, r *http.Request) {
	path := strings.TrimPrefix(r.URL.Path, "/api/v1/tasks/")
	parts := strings.Split(path, "/")

	if len(parts) == 0 || parts[0] == "" {
		http.NotFound(w, r)
		return
	}

	taskID := parts[0]

	// POST /api/v1/tasks/{task_id}/retry
	if len(parts) > 1 && parts[1] == "retry" {
		if r.Method != http.MethodPost {
			http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
			return
		}

		err := h.service.RetryTask(r.Context(), taskID, func(ctx context.Context, t *Task) error {
			return nil
		})
		if err != nil {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusBadRequest)
			_, _ = w.Write([]byte(fmt.Sprintf(`{"error":{"code":"FAILED","message":%q}}`, err.Error())))
			return
		}

		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(`{"status":"retrying"}`))
		return
	}

	// GET /api/v1/tasks/{task_id}
	if r.Method == http.MethodGet {
		t, err := h.service.GetTask(r.Context(), taskID)
		if err != nil {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"error":{"code":"NOT_FOUND","message":"task not found"}}`))
			return
		}

		w.Header().Set("Content-Type", "application/json")
		data, _ := json.Marshal(t)
		_, _ = w.Write(data)
		return
	}

	http.NotFound(w, r)
}
