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
	"github.com/diezy-labs/claw-crew/engine/src/artifact"
	"github.com/diezy-labs/claw-crew/engine/src/crew"
	"github.com/diezy-labs/claw-crew/engine/src/run"
	"github.com/diezy-labs/claw-crew/engine/src/fleet"
	"github.com/diezy-labs/claw-crew/engine/src/task"
	"github.com/diezy-labs/claw-crew/engine/src/tool"
	"github.com/diezy-labs/claw-crew/engine/src/workflow"
	"google.golang.org/grpc"
)

// App represents the root runtime container for the Go Agent Engine
type App struct {
	Cfg             *config.AppConfig
	GRPCServer      *grpc.Server
	MetricsServer   *metrics.Server
	CrewHandler     *crew.GRPCHandler
	RunHandler      *run.HTTPHandler
	TaskHandler     *task.HTTPHandler
	ToolHandler     *tool.HTTPHandler
	ArtifactHandler *artifact.HTTPHandler
	WorkflowHandler *workflow.HTTPHandler
	FleetHandler    *fleet.HTTPHandler
}

// NewGRPCServer creates a grpc.Server instance with interceptors configured
func NewGRPCServer() *grpc.Server {
	return grpc.NewServer(
		grpc.UnaryInterceptor(interceptors.UnaryServerInterceptor()),
		grpc.StreamInterceptor(interceptors.StreamServerInterceptor()),
	)
}

// ProvideMetricsServer provider for Wire to instantiate Metrics Server
func ProvideMetricsServer(cfg *config.AppConfig) *metrics.Server {
	return metrics.NewServer(cfg.MetricsPort)
}

// NewApp constructs a new App container instance
func NewApp(
	cfg *config.AppConfig,
	grpcServer *grpc.Server,
	metricsServer *metrics.Server,
	crewHandler *crew.GRPCHandler,
	runHandler *run.HTTPHandler,
	taskHandler *task.HTTPHandler,
	toolHandler *tool.HTTPHandler,
	artifactHandler *artifact.HTTPHandler,
	workflowHandler *workflow.HTTPHandler,
	fleetHandler *fleet.HTTPHandler,
) *App {
	crewHandler.RegisterService(grpcServer)
	crewHandler.RegisterHTTP(metricsServer)
	runHandler.RegisterHTTP(metricsServer)
	taskHandler.RegisterHTTP(metricsServer)
	toolHandler.RegisterHTTP(metricsServer)
	artifactHandler.RegisterHTTP(metricsServer)
	workflowHandler.RegisterHTTP(metricsServer)
	fleetHandler.RegisterHTTP(metricsServer)

	return &App{
		Cfg:             cfg,
		GRPCServer:      grpcServer,
		MetricsServer:   metricsServer,
		CrewHandler:     crewHandler,
		RunHandler:      runHandler,
		TaskHandler:     taskHandler,
		ToolHandler:     toolHandler,
		ArtifactHandler: artifactHandler,
		WorkflowHandler: workflowHandler,
		FleetHandler:    fleetHandler,
	}
}

// Run executes the gRPC and Metrics servers concurrently with graceful shutdown handling
func (a *App) Run() error {
	log := logger.Get()

	// 0. Resume runs interrupted by a prior restart (F1-3). No-op on in-memory store.
	if recovered, err := a.RunHandler.Service().ResumeInterrupted(context.Background()); err != nil {
		log.Warn("resume interrupted runs failed", slog.String("error", err.Error()))
	} else if len(recovered) > 0 {
		log.Info("resumed interrupted runs after restart", slog.Int("count", len(recovered)))
	}

	// 1. Start Prometheus Metrics HTTP Server
	go func() {
		log.Info("starting Prometheus metrics server", slog.Int("port", a.MetricsServer.Port()))
		if err := a.MetricsServer.Start(); err != nil {
			log.Error("failed to start metrics server", slog.String("error", err.Error()))
		}
	}()

	// 2. Start gRPC Server
	lis, err := net.Listen("tcp", fmt.Sprintf(":%d", a.Cfg.GRPCPort))
	if err != nil {
		return fmt.Errorf("failed to listen on port %d: %w", a.Cfg.GRPCPort, err)
	}

	go func() {
		log.Info("starting gRPC Agent Engine server", slog.Int("port", a.Cfg.GRPCPort))
		if err := a.GRPCServer.Serve(lis); err != nil {
			log.Error("gRPC server stopped with error", slog.String("error", err.Error()))
		}
	}()

	// 3. Handle OS termination signals for graceful shutdown
	quit := make(chan os.Signal, 1)
	signal.Notify(quit, syscall.SIGINT, syscall.SIGTERM)
	sig := <-quit

	log.Info("received termination signal, initiating graceful shutdown...", slog.String("signal", sig.String()))

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
		log.Info("gRPC server stopped successfully")
	case <-time.After(5 * time.Second):
		log.Warn("graceful stop timed out, forcing gRPC server termination")
		a.GRPCServer.Stop()
	}

	log.Info("Agent Engine shutdown completed")
	return nil
}
