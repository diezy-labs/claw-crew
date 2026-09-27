package metrics

import (
	"context"
	"fmt"
	"net/http"
	"sync"
	"time"

	"github.com/prometheus/client_golang/prometheus"
	"github.com/prometheus/client_golang/prometheus/promhttp"
)

var (
	once sync.Once

	// ActiveAgents counts the number of agents and sub-agents currently processing tasks
	ActiveAgents = prometheus.NewGauge(prometheus.GaugeOpts{
		Namespace: "clawcrew",
		Subsystem: "engine",
		Name:      "active_agents",
		Help:      "Total number of agents or sub-agents currently processing tasks",
	})

	// AgentTurnDuration tracks agent turn execution duration in seconds
	AgentTurnDuration = prometheus.NewHistogramVec(prometheus.HistogramOpts{
		Namespace: "clawcrew",
		Subsystem: "engine",
		Name:      "agent_turn_duration_seconds",
		Help:      "Histogram of agent turn execution duration in seconds",
		Buckets:   prometheus.DefBuckets,
	}, []string{"agent_id", "status"})

	// LLMTokenUsage counts input and output tokens consumed across LLM providers
	LLMTokenUsage = prometheus.NewCounterVec(prometheus.CounterOpts{
		Namespace: "clawcrew",
		Subsystem: "engine",
		Name:      "llm_token_usage_total",
		Help:      "Total input and output tokens processed by LLM providers",
	}, []string{"provider", "model", "type"}) // type: prompt | completion

	// GRPCRequestsTotal counts total inbound gRPC requests
	GRPCRequestsTotal = prometheus.NewCounterVec(prometheus.CounterOpts{
		Namespace: "clawcrew",
		Subsystem: "engine",
		Name:      "grpc_requests_total",
		Help:      "Total gRPC requests received by the engine",
	}, []string{"method", "status"})

	// GRPCRequestDuration measures gRPC execution latency in seconds
	GRPCRequestDuration = prometheus.NewHistogramVec(prometheus.HistogramOpts{
		Namespace: "clawcrew",
		Subsystem: "engine",
		Name:      "grpc_request_duration_seconds",
		Help:      "Duration of gRPC request processing in seconds",
		Buckets:   []float64{0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10},
	}, []string{"method"})

	// ErrorsTotal counts structured errors categorized by architectural layer and code
	ErrorsTotal = prometheus.NewCounterVec(prometheus.CounterOpts{
		Namespace: "clawcrew",
		Subsystem: "engine",
		Name:      "errors_total",
		Help:      "Total structured errors occurring in each Clean Architecture layer",
	}, []string{"layer", "code"})
)

// RegisterMetrics registers all Prometheus metrics with the default registry
func RegisterMetrics() {
	once.Do(func() {
		prometheus.MustRegister(
			ActiveAgents,
			AgentTurnDuration,
			LLMTokenUsage,
			GRPCRequestsTotal,
			GRPCRequestDuration,
			ErrorsTotal,
		)
	})
}

// Server represents an HTTP server for Prometheus metrics and engine endpoints
type Server struct {
	httpServer *http.Server
	mux        *http.ServeMux
	port       int
}

// NewServer constructs a new Prometheus metrics Server instance
func NewServer(port int) *Server {
	RegisterMetrics()

	mux := http.NewServeMux()
	mux.Handle("/metrics", promhttp.Handler())
	mux.HandleFunc("/healthz", func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("ok"))
	})

	return &Server{
		port: port,
		mux:  mux,
		httpServer: &http.Server{
			Addr:              fmt.Sprintf(":%d", port),
			Handler:           mux,
			ReadHeaderTimeout: 5 * time.Second,
		},
	}
}

// RegisterRoute adds a custom HTTP handler to the server mux
func (s *Server) RegisterRoute(pattern string, handler http.Handler) {
	s.mux.Handle(pattern, handler)
}

// RegisterRouteFunc adds a custom HTTP handler function to the server mux
func (s *Server) RegisterRouteFunc(pattern string, handlerFunc http.HandlerFunc) {
	s.mux.HandleFunc(pattern, handlerFunc)
}

// Start runs the HTTP metrics server (should be executed in a goroutine)
func (s *Server) Start() error {
	if err := s.httpServer.ListenAndServe(); err != nil && err != http.ErrServerClosed {
		return err
	}
	return nil
}

// Stop gracefully shuts down the HTTP metrics server
func (s *Server) Stop(ctx context.Context) error {
	return s.httpServer.Shutdown(ctx)
}

// Port returns the server's listening port
func (s *Server) Port() int {
	return s.port
}
