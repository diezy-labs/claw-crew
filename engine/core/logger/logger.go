package logger

import (
	"io"
	"log/slog"
	"os"
	"path/filepath"
	"runtime"
	"sync"

	"gopkg.in/natefinch/lumberjack.v2"
)

var (
	defaultLogger *slog.Logger
	once          sync.Once
)

// Config defines logging options
type Config struct {
	Level      string `json:"level"`        // "debug", "info", "warn", "error"
	LogPath    string `json:"log_path"`     // Path to log file; uses default if empty
	MaxSizeMB  int    `json:"max_size_mb"`  // Max file size before rotation in megabytes
	MaxBackups int    `json:"max_backups"`  // Retained backup file count
	MaxAgeDays int    `json:"max_age_days"` // Max days to retain old log files
	Compress   bool   `json:"compress"`     // Compress rotated files with gzip
	ConsoleOut bool   `json:"console_out"`  // Also output logs to stdout
}

// DefaultLogPath returns standard OS-specific log file location
func DefaultLogPath() string {
	if runtime.GOOS == "windows" {
		appData := os.Getenv("APPDATA")
		if appData != "" {
			return filepath.Join(appData, "clawcrew", "logs", "agent.log")
		}
	}

	homeDir, err := os.UserHomeDir()
	if err == nil {
		return filepath.Join(homeDir, ".clawcrew", "logs", "agent.log")
	}

	return filepath.Join(".", "logs", "agent.log")
}

// Init initializes the centralized global logger
func Init(cfg Config) (*slog.Logger, error) {
	var initErr error
	once.Do(func() {
		logPath := cfg.LogPath
		if logPath == "" {
			logPath = DefaultLogPath()
		}

		// Ensure parent directory exists
		dir := filepath.Dir(logPath)
		if err := os.MkdirAll(dir, 0755); err != nil {
			initErr = err
			return
		}

		maxSize := cfg.MaxSizeMB
		if maxSize <= 0 {
			maxSize = 50 // 50MB default
		}
		maxBackups := cfg.MaxBackups
		if maxBackups <= 0 {
			maxBackups = 5
		}
		maxAge := cfg.MaxAgeDays
		if maxAge <= 0 {
			maxAge = 30
		}

		fileWriter := &lumberjack.Logger{
			Filename:   logPath,
			MaxSize:    maxSize,
			MaxBackups: maxBackups,
			MaxAge:     maxAge,
			Compress:   cfg.Compress,
		}

		var writers io.Writer = fileWriter
		if cfg.ConsoleOut {
			writers = io.MultiWriter(os.Stdout, fileWriter)
		}

		// Determine log level
		var level slog.Level
		switch cfg.Level {
		case "debug":
			level = slog.LevelDebug
		case "warn":
			level = slog.LevelWarn
		case "error":
			level = slog.LevelError
		default:
			level = slog.LevelInfo
		}

		opts := &slog.HandlerOptions{
			Level:     level,
			AddSource: true,
		}

		// Use JSON handler for easy parsing by UI and dashboard
		handler := slog.NewJSONHandler(writers, opts)
		defaultLogger = slog.New(handler)
		slog.SetDefault(defaultLogger)
	})

	if initErr != nil {
		return nil, initErr
	}
	return defaultLogger, nil
}

// Get returns the default centralized logger
func Get() *slog.Logger {
	if defaultLogger == nil {
		return slog.Default()
	}
	return defaultLogger
}
