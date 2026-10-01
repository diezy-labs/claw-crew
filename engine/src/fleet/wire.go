package fleet

import "github.com/google/wire"

// Set defines Wire provider set for the fleet domain
var Set = wire.NewSet(
	NewService,
	NewHTTPHandler,
)
