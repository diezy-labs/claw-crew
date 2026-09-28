package workflow

import (
	"encoding/json"
	"net/http"
	"strings"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/core/metrics"
)

// HTTPHandler handles REST endpoints for workflows and SOPs
type HTTPHandler struct {
	svc Service
}

// NewHTTPHandler constructs a new HTTPHandler
func NewHTTPHandler(svc Service) *HTTPHandler {
	return &HTTPHandler{svc: svc}
}

// RegisterHTTP registers the workflow endpoints onto the metrics/HTTP server multiplexer
func (h *HTTPHandler) RegisterHTTP(srv *metrics.Server) {
	srv.RegisterRouteFunc("/api/v1/workflows", h.handleWorkflows)
	srv.RegisterRouteFunc("/api/v1/workflows/", h.handleWorkflowByID)
}

func (h *HTTPHandler) handleWorkflows(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "Method not allowed", http.StatusMethodNotAllowed)
		return
	}

	workflows, err := h.svc.ListWorkflows(r.Context())
	if err != nil {
		h.writeError(w, err)
		return
	}

	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(workflows)
}

func (h *HTTPHandler) handleWorkflowByID(w http.ResponseWriter, r *http.Request) {
	path := strings.TrimPrefix(r.URL.Path, "/api/v1/workflows/")
	parts := strings.Split(path, "/")
	id := parts[0]
	if id == "" {
		http.Error(w, "workflow ID required", http.StatusBadRequest)
		return
	}

	if len(parts) == 1 {
		// GET /api/v1/workflows/{id}
		if r.Method != http.MethodGet {
			http.Error(w, "Method not allowed", http.StatusMethodNotAllowed)
			return
		}

		wf, err := h.svc.GetWorkflow(r.Context(), id)
		if err != nil {
			h.writeError(w, err)
			return
		}

		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(wf)
		return
	}

	if len(parts) == 2 && parts[1] == "instantiate" {
		// POST /api/v1/workflows/{id}/instantiate
		if r.Method != http.MethodPost {
			http.Error(w, "Method not allowed", http.StatusMethodNotAllowed)
			return
		}

		var req InstantiateWorkflowRequest
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			http.Error(w, "invalid request body", http.StatusBadRequest)
			return
		}

		resp, err := h.svc.InstantiateWorkflow(r.Context(), id, &req)
		if err != nil {
			h.writeError(w, err)
			return
		}

		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusCreated)
		_ = json.NewEncoder(w).Encode(resp)
		return
	}

	http.NotFound(w, r)
}

func (h *HTTPHandler) writeError(w http.ResponseWriter, err error) {
	status := http.StatusInternalServerError
	if appErr, ok := err.(*appErrors.AppError); ok {
		switch appErr.Code {
		case appErrors.CodeNotFound:
			status = http.StatusNotFound
		case appErrors.CodeInvalidArgument:
			status = http.StatusBadRequest
		case appErrors.CodePermissionDenied:
			status = http.StatusForbidden
		default:
			status = http.StatusInternalServerError
		}
	}

	http.Error(w, err.Error(), status)
}
