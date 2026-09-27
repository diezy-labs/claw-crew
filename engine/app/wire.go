//go:build wireinject
// +build wireinject

package app

import (
	"github.com/diezy-labs/claw-crew/engine/core/config"
	"github.com/diezy-labs/claw-crew/engine/src/crew"
	"github.com/google/wire"
)

// InitializeApp membangun dependency graph menggunakan Google Wire
func InitializeApp(cfg *config.AppConfig) (*App, error) {
	wire.Build(
		NewGRPCServer,
		ProvideMetricsServer,
		crew.Set,
		NewApp,
	)
	return &App{}, nil
}
