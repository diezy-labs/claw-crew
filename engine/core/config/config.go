package config

import (
	"flag"
	"os"
	"strconv"

	"github.com/diezy-labs/claw-crew/engine/core/logger"
)

// AppConfig holds all configurations for Go Agent Engine daemon
type AppConfig struct {
	GRPCPort     int           `json:"grpc_port"`
	MetricsPort  int           `json:"metrics_port"`
	SystemGRPC   string        `json:"system_grpc"` // gRPC gateway address of Rust core
	LoggerConfig logger.Config `json:"logger"`
}

// LoadConfig loads configurations from CLI flags and environment variables
func LoadConfig() *AppConfig {
	grpcPort := flag.Int("grpc-port", 50051, "gRPC server port for Agent Engine")
	metricsPort := flag.Int("metrics-port", 9090, "HTTP server port for Prometheus metrics")
	systemGRPC := flag.String("system-grpc", "localhost:50052", "gRPC address of Rust SystemGateway")
	logPath := flag.String("log-path", "", "Path to log file (default: OS app data)")
	logLevel := flag.String("log-level", "info", "Logging level (debug, info, warn, error)")
	consoleOut := flag.Bool("console-out", true, "Also emit log output to console")

	flag.Parse()

	cfg := &AppConfig{
		GRPCPort:    *grpcPort,
		MetricsPort: *metricsPort,
		SystemGRPC:  *systemGRPC,
		LoggerConfig: logger.Config{
			Level:      *logLevel,
			LogPath:    *logPath,
			MaxSizeMB:  50,
			MaxBackups: 5,
			MaxAgeDays: 30,
			Compress:   true,
			ConsoleOut: *consoleOut,
		},
	}

	// Environment variable overrides
	if envPort := os.Getenv("CLAWCREW_ENGINE_GRPC_PORT"); envPort != "" {
		if p, err := strconv.Atoi(envPort); err == nil {
			cfg.GRPCPort = p
		}
	}
	if envMetrics := os.Getenv("CLAWCREW_ENGINE_METRICS_PORT"); envMetrics != "" {
		if p, err := strconv.Atoi(envMetrics); err == nil {
			cfg.MetricsPort = p
		}
	}
	if envSystem := os.Getenv("CLAWCREW_RUST_GRPC_ADDR"); envSystem != "" {
		cfg.SystemGRPC = envSystem
	}
	if envLog := os.Getenv("CLAWCREW_LOG_PATH"); envLog != "" {
		cfg.LoggerConfig.LogPath = envLog
	}

	return cfg
}
