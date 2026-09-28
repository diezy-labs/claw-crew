package task

import "github.com/google/wire"

// Set defines Wire provider set for the task domain
var Set = wire.NewSet(
	NewMemoryTaskStore,
	wire.Bind(new(Store), new(*MemoryTaskStore)),
	NewScheduler,
	wire.Bind(new(Scheduler), new(*DAGScheduler)),
	NewService,
	NewHTTPHandler,
)
