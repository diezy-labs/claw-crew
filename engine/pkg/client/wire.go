package client

import (
	"github.com/diezy-labs/claw-crew/engine/core/config"
	"github.com/google/wire"
)

// ProvideSystemGatewayClient instantiates the SystemGatewayClient from AppConfig
func ProvideSystemGatewayClient(cfg *config.AppConfig) (SystemGatewayClient, error) {
	return NewSystemGatewayClient(cfg.SystemGRPC)
}

// Set defines the Wire provider set for gateway client
var Set = wire.NewSet(
	ProvideSystemGatewayClient,
)
