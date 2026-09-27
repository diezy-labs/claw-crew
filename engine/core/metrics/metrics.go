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

	// ActiveAgents menghitung jumlah agen/goroutine yang sedang aktif
	ActiveAgents = prometheus.NewGauge(prometheus.GaugeOpts{
		Namespace: "clawcrew",
		Subsystem: "engine",
		Name:      "active_agents",
		Help:      "Jumlah agen atau sub-agent yang sedang aktif memproses task",
	})

	// AgentTurnDuration mengukur durasi penyelesaian turn
	AgentTurnDuration = prometheus.NewHistogramVec(prometheus.HistogramOpts{
		Namespace: "clawcrew",
		Subsystem: "engine",
		Name:      "agent_turn_duration_seconds",
		Help:      "Histogram durasi eksekusi turn agent dalam detik",
		Buckets:   prometheus.DefBuckets,
	}, []string{"agent_id", "status"})

	// LLMTokenUsage menghitung total token yang dikonsumsi model
	LLMTokenUsage = prometheus.NewCounterVec(prometheus.CounterOpts{
		Namespace: "clawcrew",
		Subsystem: "engine",
		Name:      "llm_token_usage_total",
		Help:      "Total token input dan output yang diproses oleh provider LLM",
	}, []string{"provider", "model", "type"}) // type: prompt | completion

	// GRPCRequestsTotal menghitung total request gRPC yang masuk
	GRPCRequestsTotal = prometheus.NewCounterVec(prometheus.CounterOpts{
		Namespace: "clawcrew",
		Subsystem: "engine",
		Name:      "grpc_requests_total",
		Help:      "Total permintaan gRPC yang diterima engine",
	}, []string{"method", "status"})

	// GRPCRequestDuration mengukur latensi eksekusi endpoint gRPC
	GRPCRequestDuration = prometheus.NewHistogramVec(prometheus.HistogramOpts{
		Namespace: "clawcrew",
		Subsystem: "engine",
		Name:      "grpc_request_duration_seconds",
		Help:      "Durasi pemrosesan permintaan gRPC dalam detik",
		Buckets:   []float64{0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10},
	}, []string{"method"})

	// ErrorsTotal menghitung total error berdasarkan layer dan code
	ErrorsTotal = prometheus.NewCounterVec(prometheus.CounterOpts{
		Namespace: "clawcrew",
		Subsystem: "engine",
		Name:      "errors_total",
		Help:      "Total error terstruktur yang terjadi di setiap layer Clean Architecture",
	}, []string{"layer", "code"})
)

// RegisterMetrics mendaftarkan seluruh metrik Prometheus
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

// Server merepresentasikan HTTP server untuk Prometheus metrics
type Server struct {
	httpServer *http.Server
	port       int
}

// NewServer membuat instance Server metrics
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
		httpServer: &http.Server{
			Addr:              fmt.Sprintf(":%d", port),
			Handler:           mux,
			ReadHeaderTimeout: 5 * time.Second,
		},
	}
}

// Start menjalankan HTTP server metrics secara blocking (jalankan di goroutine)
func (s *Server) Start() error {
	if err := s.httpServer.ListenAndServe(); err != nil && err != http.ErrServerClosed {
		return err
	}
	return nil
}

// Stop menghentikan HTTP server metrics dengan graceful shutdown
func (s *Server) Stop(ctx context.Context) error {
	return s.httpServer.Shutdown(ctx)
}

// Port mengembalikan port yang digunakan server
func (s *Server) Port() int {
	return s.port
}
