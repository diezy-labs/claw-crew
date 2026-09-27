package app

import (
	"context"
	"fmt"
	"log/slog"
	"net"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/diezy-labs/claw-crew/engine/core/config"
	"github.com/diezy-labs/claw-crew/engine/core/interceptors"
	"github.com/diezy-labs/claw-crew/engine/core/logger"
	"github.com/diezy-labs/claw-crew/engine/core/metrics"
	"github.com/diezy-labs/claw-crew/engine/src/crew"
	"google.golang.org/grpc"
)

// App merepresentasikan root runtime container untuk Go Engine
type App struct {
	Cfg           *config.AppConfig
	GRPCServer    *grpc.Server
	MetricsServer *metrics.Server
	CrewHandler   *crew.GRPCHandler
}

// NewGRPCServer membuat instance grpc.Server dengan interceptors terpasang
func NewGRPCServer() *grpc.Server {
	return grpc.NewServer(
		grpc.UnaryInterceptor(interceptors.UnaryServerInterceptor()),
		grpc.StreamInterceptor(interceptors.StreamServerInterceptor()),
	)
}

// ProvideMetricsServer provider untuk Wire menginisialisasi Metrics Server
func ProvideMetricsServer(cfg *config.AppConfig) *metrics.Server {
	return metrics.NewServer(cfg.MetricsPort)
}

// NewApp membuat instance container App
func NewApp(
	cfg *config.AppConfig,
	grpcServer *grpc.Server,
	metricsServer *metrics.Server,
	crewHandler *crew.GRPCHandler,
) *App {
	// Daftarkan gRPC handlers
	crewHandler.RegisterService(grpcServer)

	return &App{
		Cfg:           cfg,
		GRPCServer:    grpcServer,
		MetricsServer: metricsServer,
		CrewHandler:   crewHandler,
	}
}

// Run menjalankan server gRPC dan Metrics secara konkuren dengan graceful shutdown
func (a *App) Run() error {
	log := logger.Get()

	// 1. Jalankan Prometheus Metrics HTTP Server
	go func() {
		log.Info("memulai Prometheus metrics server", slog.Int("port", a.MetricsServer.Port()))
		if err := a.MetricsServer.Start(); err != nil {
			log.Error("gagal menjalankan metrics server", slog.String("error", err.Error()))
		}
	}()

	// 2. Jalankan gRPC Server
	lis, err := net.Listen("tcp", fmt.Sprintf(":%d", a.Cfg.GRPCPort))
	if err != nil {
		return fmt.Errorf("failed to listen on port %d: %w", a.Cfg.GRPCPort, err)
	}

	go func() {
		log.Info("memulai gRPC Agent Engine server", slog.Int("port", a.Cfg.GRPCPort))
		if err := a.GRPCServer.Serve(lis); err != nil {
			log.Error("gRPC server berhenti dengan error", slog.String("error", err.Error()))
		}
	}()

	// 3. Tangani OS Signal untuk graceful shutdown
	quit := make(chan os.Signal, 1)
	signal.Notify(quit, syscall.SIGINT, syscall.SIGTERM)
	sig := <-quit

	log.Info("menerima sinyal termination, memulai graceful shutdown...", slog.String("signal", sig.String()))

	// Graceful stop gRPC
	stopped := make(chan struct{})
	go func() {
		a.GRPCServer.GracefulStop()
		close(stopped)
	}()

	// Graceful stop HTTP Metrics
	shutdownCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := a.MetricsServer.Stop(shutdownCtx); err != nil {
		log.Warn("metrics server stop warning", slog.String("error", err.Error()))
	}

	select {
	case <-stopped:
		log.Info("gRPC server berhasil dihentikan secara aman.")
	case <-time.After(5 * time.Second):
		log.Warn("graceful stop timed out, menghentikan gRPC server secara paksa.")
		a.GRPCServer.Stop()
	}

	log.Info("Agent Engine shutdown selesai.")
	return nil
}
