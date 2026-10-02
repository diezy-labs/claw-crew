package fleet

import (
	"encoding/json"
	"io"
	"net/http"
	"strings"

	"github.com/diezy-labs/claw-crew/engine/core/metrics"
)

type HTTPHandler struct {
	service Service
}

func NewHTTPHandler(service Service) *HTTPHandler {
	return &HTTPHandler{
		service: service,
	}
}

func (h *HTTPHandler) RegisterHTTP(server *metrics.Server) {
	server.RegisterRouteFunc("/api/fleet/metrics", h.handleMetrics)
	server.RegisterRouteFunc("/api/fleet/deck-bell", h.handleDeckBell)
	server.RegisterRouteFunc("/api/fleet/seed", h.handleSeedData)
	server.RegisterRouteFunc("/api/system/executive-briefing", h.handleBriefing)
	server.RegisterRouteFunc("/api/providers/harbor", h.handleHarborProviders)
	server.RegisterRouteFunc("/api/diagnostics", h.handleDiagnostics)
	server.RegisterRouteFunc("/api/diagnostics/remedy", h.handleRemedy)
	server.RegisterRouteFunc("/api/snapshots", h.handleSnapshots)
	server.RegisterRouteFunc("/api/fleet/policies", h.handlePolicies)
	server.RegisterRouteFunc("/api/collections/", h.handleCollections)
	server.RegisterRouteFunc("/api/chat/quartermaster", h.handleQuartermasterChat)
}

func writeJSON(w http.ResponseWriter, status int, data any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(data)
}

func (h *HTTPHandler) handleMetrics(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	metricsData, err := h.service.GetMetrics(r.Context())
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}
	writeJSON(w, http.StatusOK, metricsData)
}

func (h *HTTPHandler) handleDeckBell(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	msg, err := h.service.RingDeckBell(r.Context())
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"success": true, "message": msg})
}

func (h *HTTPHandler) handleBriefing(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	briefing, err := h.service.GetExecutiveBriefing(r.Context())
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}
	writeJSON(w, http.StatusOK, briefing)
}

func (h *HTTPHandler) handleHarborProviders(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	providers, err := h.service.GetHarborProviders(r.Context())
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}
	writeJSON(w, http.StatusOK, providers)
}

func (h *HTTPHandler) handleDiagnostics(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	diags, err := h.service.GetDiagnostics(r.Context())
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}
	writeJSON(w, http.StatusOK, diags)
}

func (h *HTTPHandler) handleRemedy(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	remedied, err := h.service.ApplyRemedy(r.Context())
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"success": true, "diagnostics": remedied})
}

func (h *HTTPHandler) handleSnapshots(w http.ResponseWriter, r *http.Request) {
	if r.Method == http.MethodGet {
		snaps, err := h.service.GetSnapshots(r.Context())
		if err != nil {
			http.Error(w, err.Error(), http.StatusInternalServerError)
			return
		}
		writeJSON(w, http.StatusOK, snaps)
		return
	}
	if r.Method == http.MethodPost {
		// Frontend (apiClient.createSnapshot) sends {title}; accept {label} too
		// for back-compat. C4: align decoder to the real request contract.
		var req struct {
			Title string `json:"title"`
			Label string `json:"label"`
		}
		_ = json.NewDecoder(r.Body).Decode(&req)
		label := req.Title
		if label == "" {
			label = req.Label
		}
		if label == "" {
			label = "Manual Snapshot via Go Orchestrator"
		}
		snap, err := h.service.CreateSnapshot(r.Context(), label)
		if err != nil {
			http.Error(w, err.Error(), http.StatusInternalServerError)
			return
		}
		writeJSON(w, http.StatusCreated, snap)
		return
	}
	http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
}

func (h *HTTPHandler) handlePolicies(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	policies, err := h.service.GetFleetPolicies(r.Context())
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}
	writeJSON(w, http.StatusOK, policies)
}

func (h *HTTPHandler) handleCollections(w http.ResponseWriter, r *http.Request) {
	collectionName := strings.TrimPrefix(r.URL.Path, "/api/collections/")
	collectionName = strings.TrimSpace(collectionName)

	if collectionName == "" {
		http.NotFound(w, r)
		return
	}

	if r.Method == http.MethodGet {
		data, err := h.service.GetCollection(r.Context(), collectionName)
		if err != nil {
			// If not found yet, return empty list []
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusOK)
			_, _ = w.Write([]byte("[]"))
			return
		}
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write(data)
		return
	}

	if r.Method == http.MethodPost {
		body, err := io.ReadAll(r.Body)
		if err != nil {
			http.Error(w, "failed to read body", http.StatusBadRequest)
			return
		}
		if err := h.service.SaveCollection(r.Context(), collectionName, body); err != nil {
			http.Error(w, err.Error(), http.StatusInternalServerError)
			return
		}
		writeJSON(w, http.StatusOK, map[string]any{"success": true})
		return
	}

	http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
}

func (h *HTTPHandler) handleQuartermasterChat(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	var req struct {
		Message string `json:"message"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		http.Error(w, "invalid request body", http.StatusBadRequest)
		return
	}
	resp, err := h.service.ChatQuartermaster(r.Context(), req.Message)
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}
	writeJSON(w, http.StatusOK, resp)
}

func (h *HTTPHandler) handleSeedData(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}

	seedData, err := h.service.GetSeedData(r.Context())
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}

	writeJSON(w, http.StatusOK, map[string]any{
		"status": "ok",
		"seed":   seedData,
	})
}
