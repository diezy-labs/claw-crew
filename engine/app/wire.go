//go:build wireinject
// +build wireinject

package app

import (
	"github.com/diezy-labs/claw-crew/engine/core/config"
	"github.com/diezy-labs/claw-crew/engine/pkg/client"
	"github.com/diezy-labs/claw-crew/engine/src/artifact"
	"github.com/diezy-labs/claw-crew/engine/src/crew"
	"github.com/diezy-labs/claw-crew/engine/src/fleet"
	"github.com/diezy-labs/claw-crew/engine/src/llm"
	"github.com/diezy-labs/claw-crew/engine/src/memory"
	"github.com/diezy-labs/claw-crew/engine/src/run"
	"github.com/diezy-labs/claw-crew/engine/src/task"
	"github.com/diezy-labs/claw-crew/engine/src/tool"
	"github.com/diezy-labs/claw-crew/engine/src/workflow"
	"github.com/google/wire"
)

// InitializeApp builds the dependency injection graph via Google Wire.
//
// NOTE: fleet's objective proposer (orchestrator → fleet.ObjectiveProposer) is
// wired post-construction in wire_gen.go via fleetService.SetObjectiveProposer,
// because orchestrator imports fleet (dependency inversion breaks the cycle) and
// Wire cannot express a setter call. If you regenerate wire_gen.go, re-add that
// manual step (construct qmorch.NewService(provider, fleetService) and inject it).
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
		fleet.Set,
		NewApp,
	)
	return &App{}, nil
}
