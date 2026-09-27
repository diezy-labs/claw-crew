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
	// 1. Muat konfigurasi dari Flag & Env
	cfg := config.LoadConfig()

	// 2. Inisialisasi centralized logger
	log, err := logger.Init(cfg.LoggerConfig)
	if err != nil {
		fmt.Fprintf(os.Stderr, "FATAL: gagal menginisialisasi logger: %v\n", err)
		os.Exit(1)
	}

	log.Info("ClawCrew Go 1.27 Agent Engine sedang memulai...",
		slog.Int("grpc_port", cfg.GRPCPort),
		slog.Int("metrics_port", cfg.MetricsPort),
		slog.String("log_path", cfg.LoggerConfig.LogPath),
	)

	// 3. Bangun Dependency Graph dengan Google Wire
	appInstance, err := app.InitializeApp(cfg)
	if err != nil {
		log.Error("gagal membangun dependency graph via Wire", slog.String("error", err.Error()))
		os.Exit(1)
	}

	// 4. Jalankan aplikasi (gRPC & Metrics Server)
	if err := appInstance.Run(); err != nil {
		log.Error("aplikasi berhenti karena fatal error", slog.String("error", err.Error()))
		os.Exit(1)
	}
}
