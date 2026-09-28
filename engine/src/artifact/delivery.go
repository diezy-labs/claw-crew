package artifact

import (
	"encoding/json/v2"
	"fmt"
	"net/http"
	"strings"

	"github.com/diezy-labs/claw-crew/engine/core/metrics"
)

// HTTPHandler manages HTTP endpoints for artifacts
type HTTPHandler struct {
	service Service
}

// NewHTTPHandler creates a new HTTPHandler
func NewHTTPHandler(service Service) *HTTPHandler {
	return &HTTPHandler{
		service: service,
	}
}

// RegisterHTTP registers artifact endpoints on metrics server
func (h *HTTPHandler) RegisterHTTP(server *metrics.Server) {
	server.RegisterRouteFunc("/api/v1/artifacts/", h.handleArtifacts)
}

func (h *HTTPHandler) handleArtifacts(w http.ResponseWriter, r *http.Request) {
	path := strings.TrimPrefix(r.URL.Path, "/api/v1/artifacts/")
	parts := strings.Split(path, "/")

	if len(parts) == 0 || parts[0] == "" {
		http.NotFound(w, r)
		return
	}

	artID := parts[0]

	// GET /api/v1/artifacts/{id}/content
	if len(parts) > 1 && parts[1] == "content" {
		if r.Method != http.MethodGet {
			http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
			return
		}

		art, err := h.service.GetArtifact(r.Context(), artID)
		if err != nil {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(fmt.Sprintf(`{"error":{"code":"NOT_FOUND","message":%q}}`, err.Error())))
			return
		}

		w.Header().Set("Content-Type", art.MimeType)
		w.Header().Set("ETag", fmt.Sprintf(`"%s"`, art.Hash))
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(art.Content))
		return
	}

	// GET /api/v1/artifacts/{id}
	if r.Method == http.MethodGet {
		art, err := h.service.GetArtifact(r.Context(), artID)
		if err != nil {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(fmt.Sprintf(`{"error":{"code":"NOT_FOUND","message":%q}}`, err.Error())))
			return
		}

		w.Header().Set("Content-Type", "application/json")
		data, _ := json.Marshal(art)
		_, _ = w.Write(data)
		return
	}

	http.NotFound(w, r)
}
