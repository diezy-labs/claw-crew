//go:build wireinject
// +build wireinject

package app

import (
	"github.com/diezy-labs/claw-crew/engine/core/config"
	"github.com/diezy-labs/claw-crew/engine/pkg/client"
	"github.com/diezy-labs/claw-crew/engine/src/artifact"
	"github.com/diezy-labs/claw-crew/engine/src/crew"
	"github.com/diezy-labs/claw-crew/engine/src/llm"
	"github.com/diezy-labs/claw-crew/engine/src/memory"
	"github.com/diezy-labs/claw-crew/engine/src/run"
	"github.com/diezy-labs/claw-crew/engine/src/task"
	"github.com/diezy-labs/claw-crew/engine/src/tool"
	"github.com/diezy-labs/claw-crew/engine/src/workflow"
	"github.com/google/wire"
)

// InitializeApp builds the dependency injection graph via Google Wire
func InitializeApp(cfg *config.AppConfig) (*App, error) {
	wire.Build(
		NewGRPCServer,
		ProvideMetricsServer,
		client.Set,
		llm.Set,
		memory.Set,
		crew.Set,
		run.Set,
		task.Set,
		tool.Set,
		artifact.Set,
		workflow.Set,
		NewApp,
	)
	return &App{}, nil
}
