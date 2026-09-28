package artifact

import "github.com/google/wire"

// Set defines Wire provider set for the artifact domain
var Set = wire.NewSet(
	NewMemoryRepository,
	wire.Bind(new(Repository), new(*MemoryRepository)),
	NewService,
	NewHTTPHandler,
)
