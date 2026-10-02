package fleet

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

// TestGetFleetPolicies_ReadsFromDisk verifies F3: GetFleetPolicies reads
// policies and riskTiers from data files (not hardcoded).
func TestGetFleetPolicies_ReadsFromDisk(t *testing.T) {
	// Create a temporary data directory
	tmpDir, err := os.MkdirTemp("", "fleet-policies-test")
	if err != nil {
		t.Fatalf("Failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Write test data files
	policiesData := `[
		{"id": "pol-test", "name": "Test Policy", "scope": "test", "enforcement": "strict", "description": "Test policy"}
	]`
	if err := os.WriteFile(filepath.Join(tmpDir, "policies.json"), []byte(policiesData), 0644); err != nil {
		t.Fatalf("Failed to write policies.json: %v", err)
	}

	riskTiersData := `[
		{"tier": 99, "name": "Test Tier", "approvalRequired": true, "autoRetry": false, "maxBudgetUSD": 99.99}
	]`
	if err := os.WriteFile(filepath.Join(tmpDir, "riskTiers.json"), []byte(riskTiersData), 0644); err != nil {
		t.Fatalf("Failed to write riskTiers.json: %v", err)
	}

	// Create service with temp data dir
	svc := &fleetService{dataDir: tmpDir, fleets: make(map[string]*Fleet)}

	resp, err := svc.GetFleetPolicies(context.Background())
	if err != nil {
		t.Fatalf("GetFleetPolicies failed: %v", err)
	}

	// Verify response structure
	policies, ok := resp["policies"].([]map[string]any)
	if !ok {
		t.Fatalf("expected policies to be []map[string]any, got %T", resp["policies"])
	}

	riskTiers, ok := resp["riskTiers"].([]map[string]any)
	if !ok {
		t.Fatalf("expected riskTiers to be []map[string]any, got %T", resp["riskTiers"])
	}

	// Verify content matches what we wrote
	if len(policies) != 1 || policies[0]["id"] != "pol-test" {
		t.Errorf("expected policies[0].id='pol-test', got %+v", policies[0])
	}

	if len(riskTiers) != 1 || riskTiers[0]["tier"] != float64(99) {
		t.Errorf("expected riskTiers[0].tier=99, got %+v", riskTiers[0])
	}
}

// TestGetFleetPolicies_ReturnsErrorOnMissingFile verifies error handling.
func TestGetFleetPolicies_ReturnsErrorOnMissingFile(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "fleet-policies-missing-test")
	if err != nil {
		t.Fatalf("Failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	svc := &fleetService{dataDir: tmpDir, fleets: make(map[string]*Fleet)}

	_, err = svc.GetFleetPolicies(context.Background())
	if err == nil {
		t.Error("expected error when files are missing, got nil")
	}
}

// TestGetFleetPolicies_ResponseShape verifies the JSON response shape matches
// the TypeScript expectations (policies[], riskTiers[]).
func TestGetFleetPolicies_ResponseShape(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "fleet-policies-shape-test")
	if err != nil {
		t.Fatalf("Failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	policiesData := `[{"id": "pol-1"}]`
	riskTiersData := `[{"tier": 1}]`
	os.WriteFile(filepath.Join(tmpDir, "policies.json"), []byte(policiesData), 0644)
	os.WriteFile(filepath.Join(tmpDir, "riskTiers.json"), []byte(riskTiersData), 0644)

	svc := &fleetService{dataDir: tmpDir, fleets: make(map[string]*Fleet)}

	resp, err := svc.GetFleetPolicies(context.Background())
	if err != nil {
		t.Fatalf("GetFleetPolicies failed: %v", err)
	}

	// Type assertions to verify exact shape
	_, ok := resp["policies"].([]map[string]any)
	if !ok {
		t.Errorf("response missing policies: []map[string]any")
	}

	_, ok = resp["riskTiers"].([]map[string]any)
	if !ok {
		t.Errorf("response missing riskTiers: []map[string]any")
	}

	// Verify no extra keys
	if len(resp) != 2 {
		t.Errorf("expected 2 keys (policies, riskTiers), got %d keys: %v", len(resp), resp)
	}
}
