package main

import (
	"fmt"
	"log/slog"
	"os"

	"github.com/diezy-labs/claw-crew/engine/app"
	"github.com/diezy-labs/claw-crew/engine/core/config"
	"github.com/diezy-labs/claw-crew/engine/core/logger"
)

func main() {
	// 1. Load configuration from flags and environment variables
	cfg := config.LoadConfig()

	// 2. Initialize centralized logger
	log, err := logger.Init(cfg.LoggerConfig)
	if err != nil {
		fmt.Fprintf(os.Stderr, "FATAL: failed to initialize logger: %v\n", err)
		os.Exit(1)
	}

	log.Info("ClawCrew Go 1.27 Agent Engine starting...",
		slog.Int("grpc_port", cfg.GRPCPort),
		slog.Int("metrics_port", cfg.MetricsPort),
		slog.String("log_path", cfg.LoggerConfig.LogPath),
	)

	// 3. Build dependency injection graph with Google Wire
	appInstance, err := app.InitializeApp(cfg)
	if err != nil {
		log.Error("failed to construct dependency graph via Wire", slog.String("error", err.Error()))
		os.Exit(1)
	}

	// 4. Run application (gRPC and Metrics server)
	if err := appInstance.Run(); err != nil {
		log.Error("application stopped due to fatal error", slog.String("error", err.Error()))
		os.Exit(1)
	}
}
