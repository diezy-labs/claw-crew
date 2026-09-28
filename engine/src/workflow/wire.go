package workflow

import "github.com/google/wire"

// Set defines Wire provider set for the workflow domain
var Set = wire.NewSet(
	NewRegistry,
	NewService,
	NewHTTPHandler,
)
