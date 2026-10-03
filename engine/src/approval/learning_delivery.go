package approval

import (
	"encoding/json"
	"errors"
	"net/http"
	"strings"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/core/metrics"
	"github.com/diezy-labs/claw-crew/engine/src/memory"
)

// LearningHandler manages HTTP endpoints for learning governance proposals
type LearningHandler struct {
	service *memory.LearningGovernanceService
}

// NewLearningHandler creates a new LearningHandler
func NewLearningHandler(service *memory.LearningGovernanceService) *LearningHandler {
	return &LearningHandler{
		service: service,
	}
}

// RegisterHTTP registers learning governance endpoints on metrics server
func (h *LearningHandler) RegisterHTTP(server *metrics.Server) {
	server.RegisterRouteFunc("/api/v1/learning/proposals", h.handleProposals)
	server.RegisterRouteFunc("/api/v1/learning/proposals/", h.handleProposalByID)
}

// handleProposals handles GET /api/v1/learning/proposals and POST /api/v1/learning/proposals
func (h *LearningHandler) handleProposals(w http.ResponseWriter, r *http.Request) {
	if r.Method == http.MethodGet {
		// GET /api/v1/learning/proposals?status=pending|approved|rejected
		status := strings.ToUpper(r.URL.Query().Get("status"))
		var ps memory.ProposalStatus
		switch status {
		case "PENDING":
			ps = memory.ProposalPending
		case "APPROVED":
			ps = memory.ProposalApproved
		case "REJECTED":
			ps = memory.ProposalRejected
		default:
			ps = ""
		}
		proposals, err := h.service.ListProposals(r.Context(), ps)
		if err != nil {
			h.writeError(w, err)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"proposals": proposals,
		})
		return
	}

	if r.Method == http.MethodPost {
		// POST /api/v1/learning/proposals
		var req CreateProposalRequest
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			http.Error(w, "invalid request body", http.StatusBadRequest)
			return
		}
		if req.Rule == "" {
			http.Error(w, "rule is required", http.StatusBadRequest)
			return
		}
		id, err := h.service.ProposeLearning(r.Context(), req.Rule, req.Scope)
		if err != nil {
			h.writeError(w, err)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusCreated)
		_ = json.NewEncoder(w).Encode(map[string]string{
			"id":         id,
			"status":     "pending",
			"message":    "Proposal created and awaiting approval",
		})
		return
	}

	http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
}

// handleProposalByID handles proposal-specific operations
func (h *LearningHandler) handleProposalByID(w http.ResponseWriter, r *http.Request) {
	path := strings.TrimPrefix(r.URL.Path, "/api/v1/learning/proposals/")
	parts := strings.Split(path, "/")
	if len(parts) == 0 || parts[0] == "" {
		http.NotFound(w, r)
		return
	}

	proposalID := parts[0]

	if len(parts) > 1 && parts[1] == "approve" {
		if r.Method != http.MethodPost {
			http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
			return
		}
		if err := h.service.ApproveLearning(r.Context(), proposalID); err != nil {
			h.writeError(w, err)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]string{
			"id":      proposalID,
			"status":  "approved",
			"message": "Proposal approved and rule written to memory store",
		})
		return
	}

	if len(parts) > 1 && parts[1] == "reject" {
		if r.Method != http.MethodPost {
			http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
			return
		}
		if err := h.service.RejectLearning(r.Context(), proposalID); err != nil {
			h.writeError(w, err)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]string{
			"id":      proposalID,
			"status":  "rejected",
			"message": "Proposal rejected",
		})
		return
	}

	if len(parts) > 1 && parts[1] == "revert" {
		if r.Method != http.MethodPost {
			http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
			return
		}
		if err := h.service.RevertLearning(r.Context(), proposalID); err != nil {
			h.writeError(w, err)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]string{
			"id":      proposalID,
			"status":  "reverted",
			"message": "Approved proposal reverted",
		})
		return
	}

	// GET /api/v1/learning/proposals/{id}
	if r.Method == http.MethodGet {
		prop, err := h.service.GetProposal(r.Context(), proposalID)
		if err != nil {
			h.writeError(w, err)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(prop)
		return
	}

	http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
}

// writeError writes an error response in standard format
func (h *LearningHandler) writeError(w http.ResponseWriter, err error) {
	status := http.StatusInternalServerError
	var appErr *appErrors.AppError
	if errors.As(err, &appErr) {
		switch appErr.Code {
		case appErrors.CodeNotFound:
			status = http.StatusNotFound
		case appErrors.CodeInvalidArgument:
			status = http.StatusBadRequest
		case appErrors.CodePermissionDenied:
			status = http.StatusForbidden
		case appErrors.CodeUnauthorized:
			status = http.StatusUnauthorized
		case appErrors.CodeFailedPrecondition:
			status = http.StatusBadRequest
		default:
			status = http.StatusInternalServerError
		}
	}
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(map[string]any{
		"error": map[string]any{
			"code":    string(appErr.Code),
			"message": appErr.Message,
		},
	})
}

// CreateProposalRequest is the payload for creating a new learning proposal
type CreateProposalRequest struct {
	Rule   string            `json:"rule"`   // The correction/preference text
	Scope  map[string]string `json:"scope"`  // Where it applies (optional, default global)
}
