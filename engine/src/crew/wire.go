package crew

import "github.com/google/wire"

// Set defines the Wire provider set for the crew module
var Set = wire.NewSet(
	NewRegistry,
	NewService,
	NewGRPCHandler,
)
