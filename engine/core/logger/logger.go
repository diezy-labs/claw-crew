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

// Config mendefinisikan opsi konfigurasi logging
type Config struct {
	Level      string `json:"level"`       // "debug", "info", "warn", "error"
	LogPath    string `json:"log_path"`    // Path ke file log, jika kosong gunakan default
	MaxSizeMB  int    `json:"max_size_mb"` // Max ukuran file sebelum rotasi (MB)
	MaxBackups int    `json:"max_backups"` // Jumlah file backup rotasi
	MaxAgeDays int    `json:"max_age_days"`// Lama penyimpanan backup (hari)
	Compress   bool   `json:"compress"`    // Kompresi backup (.gz)
	ConsoleOut bool   `json:"console_out"` // Tampilkan juga di stdout
}

// DefaultLogPath menghasilkan path log standar sesuai OS
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

// Init menginisialisasi logger global tersentralisasi
func Init(cfg Config) (*slog.Logger, error) {
	var initErr error
	once.Do(func() {
		logPath := cfg.LogPath
		if logPath == "" {
			logPath = DefaultLogPath()
		}

		// Pastikan direktori log ada
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

		// Tentukan level log
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

		// Menggunakan JSON handler agar mudah diparsing oleh UI / Dashboard
		handler := slog.NewJSONHandler(writers, opts)
		defaultLogger = slog.New(handler)
		slog.SetDefault(defaultLogger)
	})

	if initErr != nil {
		return nil, initErr
	}
	return defaultLogger, nil
}

// Get mengembalikan logger default
func Get() *slog.Logger {
	if defaultLogger == nil {
		return slog.Default()
	}
	return defaultLogger
}
