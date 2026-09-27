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
	LLMBaseURL   string        `json:"llm_base_url"`
	LLMAPIKey    string        `json:"llm_api_key"`
	LLMModel     string        `json:"llm_model"`
	LoggerConfig logger.Config `json:"logger"`
}

// LoadConfig loads configurations from CLI flags and environment variables
func LoadConfig() *AppConfig {
	grpcPort := flag.Int("grpc-port", 50051, "gRPC server port for Agent Engine")
	metricsPort := flag.Int("metrics-port", 9090, "HTTP server port for Prometheus metrics")
	systemGRPC := flag.String("system-grpc", "localhost:50052", "gRPC address of Rust SystemGateway")
	llmBaseURL := flag.String("llm-base-url", "", "Custom LLM API Base URL (OpenAI compatible)")
	llmAPIKey := flag.String("llm-api-key", "", "LLM API Key")
	llmModel := flag.String("llm-model", "gpt-4o-mini", "Default LLM model name")
	logPath := flag.String("log-path", "", "Path to log file (default: OS app data)")
	logLevel := flag.String("log-level", "info", "Logging level (debug, info, warn, error)")
	consoleOut := flag.Bool("console-out", true, "Also emit log output to console")

	flag.Parse()

	cfg := &AppConfig{
		GRPCPort:    *grpcPort,
		MetricsPort: *metricsPort,
		SystemGRPC:  *systemGRPC,
		LLMBaseURL:  *llmBaseURL,
		LLMAPIKey:   *llmAPIKey,
		LLMModel:    *llmModel,
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
	if envBaseURL := os.Getenv("CLAWCREW_LLM_BASE_URL"); envBaseURL != "" {
		cfg.LLMBaseURL = envBaseURL
	}
	if envAPIKey := os.Getenv("CLAWCREW_LLM_API_KEY"); envAPIKey != "" {
		cfg.LLMAPIKey = envAPIKey
	}
	if envModel := os.Getenv("CLAWCREW_LLM_MODEL"); envModel != "" {
		cfg.LLMModel = envModel
	}
	if envLog := os.Getenv("CLAWCREW_LOG_PATH"); envLog != "" {
		cfg.LoggerConfig.LogPath = envLog
	}

	return cfg
}
