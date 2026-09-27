package crew

import "github.com/google/wire"

// Set mendefinisikan wire provider set untuk modul crew
var Set = wire.NewSet(
	NewService,
	NewGRPCHandler,
)
