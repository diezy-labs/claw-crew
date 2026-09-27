package memory

import "github.com/google/wire"

// Set defines the Wire provider set for the memory domain
var Set = wire.NewSet(
	NewVectorStore,
)
