package tool

import (
	"encoding/json"
	"net/http"
	"strings"
	"time"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/core/metrics"
)

// HTTPHandler manages tool HTTP endpoints
type HTTPHandler struct {
	service Service
}

// NewHTTPHandler creates a new HTTPHandler
func NewHTTPHandler(service Service) *HTTPHandler {
	return &HTTPHandler{
		service: service,
	}
}

// RegisterHTTP registers endpoints on metrics server
func (h *HTTPHandler) RegisterHTTP(server *metrics.Server) {
	server.RegisterRouteFunc("/api/v1/tools", h.handleTools)
	server.RegisterRouteFunc("/api/v1/approvals/", h.handleApprovals)
	server.RegisterRouteFunc("/api/v1/tool-executions/", h.handleToolExecutions)
	server.RegisterRouteFunc("/api/v1/tool-requests", h.handleToolRequests)
}

// handleTools handles GET /api/v1/tools
func (h *HTTPHandler) handleTools(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}

	workspaceID := r.URL.Query().Get("workspace_id")
	riskTierFilter := strings.ToUpper(r.URL.Query().Get("risk_tier"))

	reg := h.service.GetRegistry()
	var tools []Tool
	if reg != nil {
		tools = reg.ListByScope(workspaceID, nil)
	}

	var definitions []*ToolDefinition
	for _, t := range tools {
		def := t.Definition()
		if def == nil {
			continue
		}
		if riskTierFilter != "" && string(def.RiskTier) != riskTierFilter {
			continue
		}
		definitions = append(definitions, def)
	}

	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusOK)
	_ = json.NewEncoder(w).Encode(map[string]any{
		"tools": definitions,
	})
}

// handleApprovals handles POST /api/v1/approvals/{approval_id}/resolve and GET /api/v1/approvals/{approval_id}
func (h *HTTPHandler) handleApprovals(w http.ResponseWriter, r *http.Request) {
	path := strings.TrimPrefix(r.URL.Path, "/api/v1/approvals/")
	parts := strings.Split(path, "/")

	if len(parts) == 0 || parts[0] == "" {
		http.NotFound(w, r)
		return
	}

	apprID := parts[0]
	gate := h.service.GetApprovalGate()

	if len(parts) > 1 && parts[1] == "resolve" {
		if r.Method != http.MethodPost {
			http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
			return
		}

		var payload struct {
			Approved bool   `json:"approved"`
			Reason   string `json:"reason"`
		}
		_ = json.NewDecoder(r.Body).Decode(&payload)

		if err := gate.Resolve(apprID, payload.Approved, payload.Reason); err != nil {
			writeErrorJSON(w, http.StatusBadRequest, err)
			return
		}

		appr, _ := gate.GetApprovalRequest(apprID)
		execID := ""
		if appr != nil {
			execID = appr.ToolRequestID
		}

		status := "approved"
		if !payload.Approved {
			status = "denied"
		}

		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		_ = json.NewEncoder(w).Encode(map[string]any{
			"approval_id":  apprID,
			"status":       status,
			"resolved_at":  time.Now().UTC().Format(time.RFC3339),
			"execution_id": execID,
		})
		return
	}

	if r.Method == http.MethodGet {
		appr, err := gate.GetApprovalRequest(apprID)
		if err != nil {
			writeErrorJSON(w, http.StatusNotFound, err)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		_ = json.NewEncoder(w).Encode(appr)
		return
	}

	http.NotFound(w, r)
}

// handleToolRequests handles POST /api/v1/tool-requests
func (h *HTTPHandler) handleToolRequests(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}

	var payload struct {
		RunID     string `json:"run_id"`
		TaskID    string `json:"task_id"`
		ToolName  string `json:"tool_name"`
		Arguments string `json:"arguments"`
		Workspace string `json:"workspace"`
	}
	if err := json.NewDecoder(r.Body).Decode(&payload); err != nil {
		writeErrorJSON(w, http.StatusBadRequest, appErrors.Wrap(err, appErrors.CodeInvalidArgument, "invalid request body", appErrors.LayerDelivery))
		return
	}

	execCtx := &ExecutionContext{
		ActorID:      "api_client",
		RunID:        payload.RunID,
		TaskID:       payload.TaskID,
		WorkspaceID:  payload.Workspace,
		AllowedRoots: []string{payload.Workspace},
	}

	output, err := h.service.ExecuteWithContext(r.Context(), execCtx, payload.ToolName, payload.Arguments, payload.Workspace, true)
	if err != nil {
		writeErrorJSON(w, http.StatusBadRequest, err)
		return
	}

	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusOK)
	_ = json.NewEncoder(w).Encode(map[string]any{
		"status":       "approved",
		"output":       output,
		"artifact_ids": []string{},
	})
}

func (h *HTTPHandler) handleToolExecutions(w http.ResponseWriter, r *http.Request) {
	path := strings.TrimPrefix(r.URL.Path, "/api/v1/tool-executions/")
	parts := strings.Split(path, "/")

	if len(parts) == 0 || parts[0] == "" {
		http.NotFound(w, r)
		return
	}

	execID := parts[0]

	// POST /api/v1/tool-executions/{id}/approve
	if len(parts) > 1 && parts[1] == "approve" {
		if r.Method != http.MethodPost {
			http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
			return
		}

		if err := h.service.Approve(execID); err != nil {
			writeErrorJSON(w, http.StatusBadRequest, err)
			return
		}

		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(`{"status":"approved"}`))
		return
	}

	// POST /api/v1/tool-executions/{id}/deny
	if len(parts) > 1 && parts[1] == "deny" {
		if r.Method != http.MethodPost {
			http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
			return
		}

		var payload struct {
			Reason string `json:"reason"`
		}
		_ = json.NewDecoder(r.Body).Decode(&payload)

		if err := h.service.Deny(execID, payload.Reason); err != nil {
			writeErrorJSON(w, http.StatusBadRequest, err)
			return
		}

		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(`{"status":"denied"}`))
		return
	}

	http.NotFound(w, r)
}

func writeErrorJSON(w http.ResponseWriter, status int, err error) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	if appErr, ok := err.(*appErrors.AppError); ok {
		_ = json.NewEncoder(w).Encode(map[string]any{
			"error": map[string]any{
				"code":    string(appErr.Code),
				"message": appErr.Message,
				"layer":   string(appErr.Layer),
			},
		})
		return
	}
	_ = json.NewEncoder(w).Encode(map[string]any{
		"error": map[string]any{
			"code":    "FAILED",
			"message": err.Error(),
		},
	})
}
