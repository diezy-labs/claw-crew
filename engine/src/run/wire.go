package run

import "github.com/google/wire"

// Set defines Wire provider set for the run domain
var Set = wire.NewSet(
	NewMemoryStore,
	wire.Bind(new(Store), new(*MemoryStore)),
	NewEventHub,
	wire.Bind(new(EventHub), new(*MemoryEventHub)),
	NewService,
	wire.Bind(new(Service), new(*Service)),
	NewHTTPHandler,
)
