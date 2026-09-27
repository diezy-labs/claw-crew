package llm

import "github.com/google/wire"

// Set provides Wire dependencies for the LLM domain
var Set = wire.NewSet(
	NewProvider,
	NewToolDispatcher,
)
