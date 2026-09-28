package tool

import "github.com/google/wire"

// Set defines Wire provider set for the tool domain
var Set = wire.NewSet(
	NewRegistry,
	NewPolicyEngine,
	NewApprovalGate,
	NewService,
	NewHTTPHandler,
)

// ProviderSet is an alias for Set adhering to tech-spec naming
var ProviderSet = Set
