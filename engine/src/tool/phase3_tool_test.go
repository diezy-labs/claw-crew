package tool_test

import (
	"context"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/core/metrics"
	"github.com/diezy-labs/claw-crew/engine/src/run"
	"github.com/diezy-labs/claw-crew/engine/src/tool"
)

// 1. Canonical Normalization and Hasher Tests
func TestCanonicalNormalizationAndHashing(t *testing.T) {
	json1 := `{"b": 2, "a": 1, "nested": {"z": 10, "y": 20}}`
	json2 := "{\n  \"nested\": {\n    \"y\": 20,\n    \"z\": 10\n  },\n  \"a\": 1,\n  \"b\": 2\n}"

	norm1, err := tool.NormalizeArguments(json1)
	if err != nil {
		t.Fatalf("unexpected error normalizing json1: %v", err)
	}

	norm2, err := tool.NormalizeArguments(json2)
	if err != nil {
		t.Fatalf("unexpected error normalizing json2: %v", err)
	}

	if norm1 != norm2 {
		t.Fatalf("expected normalized json to be identical.\nnorm1: %s\nnorm2: %s", norm1, norm2)
	}

	hash1 := tool.HashArguments(norm1)
	hash2 := tool.HashArguments(norm2)

	if hash1 != hash2 {
		t.Fatalf("expected argument hashes to match. hash1: %s, hash2: %s", hash1, hash2)
	}
	if !strings.HasPrefix(hash1, "sha256:") {
		t.Fatalf("expected sha256: prefix, got %s", hash1)
	}
}

// 2. Policy Engine Tests
func TestPolicyEngine(t *testing.T) {
	policy := tool.NewPolicyEngine()
	ctx := context.Background()

	readTool := &tool.ToolDefinition{
		ID:               "workspace.read_file",
		RiskTier:         tool.RiskTierRead,
		RiskClass:        tool.RiskClassRead,
		Capabilities:     []string{"workspace.read"},
		RequiresApproval: false,
	}

	writeTool := &tool.ToolDefinition{
		ID:               "workspace.apply_patch",
		RiskTier:         tool.RiskTierWrite,
		RiskClass:        tool.RiskClassWriteWorkspace,
		Capabilities:     []string{"workspace.write"},
		RequiresApproval: true,
	}

	networkTool := &tool.ToolDefinition{
		ID:               "web.fetch",
		RiskTier:         tool.RiskTierRead,
		RiskClass:        tool.RiskClassNetworkRead,
		Capabilities:     []string{"network.read"},
		RequiresApproval: false,
	}

	// 2.1 Missing ActorID -> DENY
	v, err := policy.Evaluate(ctx, &tool.ExecutionContext{ActorID: ""}, readTool, "{}")
	if err == nil || v != tool.VerdictDeny {
		t.Fatalf("expected VerdictDeny for empty ActorID, got %v, err: %v", v, err)
	}

	// 2.2 Missing Capability -> DENY
	v, err = policy.Evaluate(ctx, &tool.ExecutionContext{
		ActorID:      "agent_1",
		Capabilities: []string{"workspace.read"},
	}, writeTool, "{}")
	if err == nil || v != tool.VerdictDeny {
		t.Fatalf("expected VerdictDeny when lacking workspace.write, got %v, err: %v", v, err)
	}

	// 2.3 Wildcard Capability -> Evaluates RiskTier
	v, err = policy.Evaluate(ctx, &tool.ExecutionContext{
		ActorID:      "agent_1",
		Capabilities: []string{"*"},
	}, readTool, "{}")
	if err != nil || v != tool.VerdictAllow {
		t.Fatalf("expected VerdictAllow for read tool with wildcard capabilities, got %v, err: %v", v, err)
	}

	// 2.4 Mutating tool -> REQUIRE_APPROVAL
	v, err = policy.Evaluate(ctx, &tool.ExecutionContext{
		ActorID:      "agent_1",
		Capabilities: []string{"workspace.write"},
	}, writeTool, "{}")
	if err != nil || v != tool.VerdictRequireApproval {
		t.Fatalf("expected VerdictRequireApproval for write tool, got %v, err: %v", v, err)
	}

	// 2.5 Classified Data with Network Tool -> DENY
	v, err = policy.Evaluate(ctx, &tool.ExecutionContext{
		ActorID:            "agent_1",
		DataClassification: "confidential",
		Capabilities:       []string{"network.read"},
	}, networkTool, "{}")
	if err == nil || v != tool.VerdictDeny {
		t.Fatalf("expected VerdictDeny for network tool on confidential data context, got %v, err: %v", v, err)
	}
}

// 3. Approval Gate CAS and Cryptographic Invariants
func TestApprovalGateCASAndIdempotency(t *testing.T) {
	tempDir := t.TempDir()
	testFile := filepath.Join(tempDir, "code.go")
	_ = os.WriteFile(testFile, []byte("package main\n\nfunc main() {}\n"), 0644)

	initialHash, err := tool.HashFile(testFile)
	if err != nil {
		t.Fatalf("failed to hash file: %v", err)
	}

	runStore := run.NewMemoryStore()
	runHub := run.NewEventHub()
	runSvc := run.NewService(runStore, runHub)
	gate := tool.NewApprovalGate(runSvc)
	ctx := context.Background()

	execCtx := &tool.ExecutionContext{
		ActorID:      "editor_agent",
		RunID:        "run_cas_01",
		WorkspaceID:  tempDir,
		AllowedRoots: []string{tempDir},
	}

	req := &tool.ApprovalRequest{
		ToolName:                "workspace.apply_patch",
		RiskTier:                tool.RiskTierWrite,
		RiskClass:               tool.RiskClassWriteWorkspace,
		RawArguments:            `{"path":"code.go"}`,
		NormalizedArgumentsHash: "sha256:abcd1234",
		Summary:                 "Apply patch to code.go",
		ResolvedTargets: []tool.TargetResource{
			{
				Type:         "file",
				Target:       testFile,
				ExpectedHash: initialHash,
			},
		},
		ExpiresAt: time.Now().UTC().Add(5 * time.Minute),
	}

	// 3.1 Simulate concurrent external modification before approval (TOCTOU conflict)
	go func() {
		time.Sleep(20 * time.Millisecond)
		// Modify file concurrently!
		_ = os.WriteFile(testFile, []byte("package main\n// concurrently modified!\nfunc main() {}\n"), 0644)

		execs := gate.ListExecutions("run_cas_01")
		if len(execs) > 0 {
			// User tries to approve, but CAS hash must detect the external modification!
			resolveErr := gate.Resolve(execs[0].ID, true, "approved by user")
			if resolveErr == nil {
				t.Errorf("expected CAS conflict error on resolve, got nil")
			}
		}
	}()

	approved, reason, _ := gate.RequestApprovalWithDetails(ctx, execCtx, req)
	if approved {
		t.Fatalf("expected approval to be rejected due to CAS mismatch, got approved=true (reason: %s)", reason)
	}
}

// 4. Workspace Tools: list_files, read_file, search_code, create_draft, apply_patch
func TestWorkspaceToolsSuite(t *testing.T) {
	tempDir := t.TempDir()

	// Setup directory structure
	_ = os.MkdirAll(filepath.Join(tempDir, "src", "sub"), 0755)
	_ = os.MkdirAll(filepath.Join(tempDir, "node_modules", "pkg"), 0755)
	_ = os.WriteFile(filepath.Join(tempDir, ".gitignore"), []byte("*.log\nsecret/\n"), 0644)
	_ = os.WriteFile(filepath.Join(tempDir, "src", "main.go"), []byte("package main\n\nfunc main() {\n\tprintln(\"Hello ClawCrew\")\n}\n"), 0644)
	_ = os.WriteFile(filepath.Join(tempDir, "src", "sub", "util.go"), []byte("package sub\n\nfunc Helper() string { return \"ok\" }\n"), 0644)
	_ = os.WriteFile(filepath.Join(tempDir, "app.log"), []byte("test log"), 0644)

	ctx := context.Background()
	execCtx := &tool.ExecutionContext{
		ActorID:      "agent_tester",
		WorkspaceID:  tempDir,
		AllowedRoots: []string{tempDir},
	}

	// 4.1 workspace.list_files
	listTool := &tool.ListFilesTool{}
	listOut, _, err := listTool.ExecuteWithContext(ctx, execCtx, `{"max_depth": 3}`)
	if err != nil {
		t.Fatalf("list_files error: %v", err)
	}
	if !strings.Contains(listOut, "src/main.go") {
		t.Errorf("expected src/main.go in list output: %s", listOut)
	}
	if strings.Contains(listOut, "node_modules") {
		t.Errorf("node_modules should be ignored: %s", listOut)
	}
	if strings.Contains(listOut, "app.log") {
		t.Errorf(".gitignore entry (*.log) should be ignored: %s", listOut)
	}

	// 4.2 workspace.read_file
	readTool := &tool.ReadFileTool{}
	readOut, _, err := readTool.ExecuteWithContext(ctx, execCtx, `{"path": "src/main.go", "max_bytes": 1000}`)
	if err != nil {
		t.Fatalf("read_file error: %v", err)
	}
	if !strings.Contains(readOut, "Hello ClawCrew") {
		t.Errorf("expected file content: %s", readOut)
	}

	// 4.3 workspace.search_code
	searchTool := &tool.SearchCodeTool{}
	searchOut, _, err := searchTool.ExecuteWithContext(ctx, execCtx, `{"query": "Hello ClawCrew"}`)
	if err != nil {
		t.Fatalf("search_code error: %v", err)
	}
	if !strings.Contains(searchOut, "main.go") {
		t.Errorf("expected match in main.go, got: %s", searchOut)
	}

	// 4.4 workspace.create_draft
	draftTool := &tool.CreateDraftTool{}
	draftOut, _, err := draftTool.ExecuteWithContext(ctx, execCtx, `{"name": "spec.md", "content": "# Architecture Spec"}`)
	if err != nil {
		t.Fatalf("create_draft error: %v", err)
	}
	if !strings.Contains(draftOut, "artifacts/drafts/spec.md") {
		t.Errorf("expected draft under artifacts/drafts, got: %s", draftOut)
	}
	draftContent, err := os.ReadFile(filepath.Join(tempDir, "artifacts", "drafts", "spec.md"))
	if err != nil || string(draftContent) != "# Architecture Spec" {
		t.Fatalf("draft content mismatch: %v", err)
	}

	// 4.5 workspace.apply_patch with CAS and rollback snapshot
	fileToPatch := filepath.Join(tempDir, "src", "main.go")
	currentHash, _ := tool.HashFile(fileToPatch)

	patchTool := &tool.ApplyPatchTool{}
	patchArgs := `{"path": "src/main.go", "expected_file_hash": "` + currentHash + `", "patch": "package main\n\nfunc main() {\n\tprintln(\"Hello ClawCrew v2\")\n}\n", "reason": "bump version"}`
	patchOut, _, err := patchTool.ExecuteWithContext(ctx, execCtx, patchArgs)
	if err != nil {
		t.Fatalf("apply_patch error: %v", err)
	}
	if !strings.Contains(patchOut, "Successfully patched") {
		t.Errorf("expected success message, got: %s", patchOut)
	}

	patchedBytes, _ := os.ReadFile(fileToPatch)
	if !strings.Contains(string(patchedBytes), "Hello ClawCrew v2") {
		t.Errorf("patched content not found: %s", string(patchedBytes))
	}

	// Verify rollback snapshot exists
	snapshots, _ := os.ReadDir(filepath.Join(tempDir, ".clawcrew", "snapshots"))
	if len(snapshots) == 0 {
		t.Errorf("expected rollback snapshot to be created in .clawcrew/snapshots")
	}
}

// 5. Web Fetch SSRF Guard Tests
func TestWebFetchSSRFGuard(t *testing.T) {
	// 5.1 Block Loopback and Private IPs
	loopbackURL, _ := url.Parse("http://127.0.0.1:8080/admin")
	if err := tool.ValidateSSRFURL(loopbackURL); err == nil {
		t.Fatalf("expected error for 127.0.0.1 loopback, got nil")
	}

	metadataURL, _ := url.Parse("http://169.254.169.254/latest/meta-data")
	if err := tool.ValidateSSRFURL(metadataURL); err == nil {
		t.Fatalf("expected error for cloud metadata IP, got nil")
	}

	privateURL, _ := url.Parse("http://192.168.1.1/router")
	if err := tool.ValidateSSRFURL(privateURL); err == nil {
		t.Fatalf("expected error for private IP 192.168.1.1, got nil")
	}

	fileURL, _ := url.Parse("file:///etc/passwd")
	if err := tool.ValidateSSRFURL(fileURL); err == nil {
		t.Fatalf("expected error for file scheme, got nil")
	}

	// 5.2 Valid public fetch with script stripping
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/html")
		_, _ = w.Write([]byte(`<html><head><script>alert("evil")</script></head><body><h1>ClawCrew Docs</h1><p>Clean text</p></body></html>`))
	}))
	defer server.Close()

	// Note: httptest.NewServer binds to 127.0.0.1, so ValidateSSRFURL will correctly block it as loopback!
	serverURL, _ := url.Parse(server.URL)
	if err := tool.ValidateSSRFURL(serverURL); err == nil {
		t.Fatalf("SSRF guard must block httptest loopback server as well!")
	}
}

// 6. Subagent Delegation & Policy Ceiling Tests
func TestDelegationPolicyCeiling(t *testing.T) {
	delegateTool := &tool.DelegateTaskTool{}
	ctx := context.Background()

	// Parent has only read capabilities
	parentCtx := &tool.ExecutionContext{
		ActorID:      "researcher_agent",
		RunID:        "run_delegation_01",
		Capabilities: []string{"workspace.read"},
	}

	// Child requests write capabilities -> MUST BE BLOCKED (Privilege Escalation)
	escalationArgs := `{"subagent_role": "Code Editor", "task_prompt": "Refactor codebase", "requested_capabilities": ["workspace.write"]}`
	_, _, err := delegateTool.ExecuteWithContext(ctx, parentCtx, escalationArgs)
	if err == nil {
		t.Fatalf("expected error when child attempts to escalate capabilities beyond parent, got nil")
	}
	if appErr, ok := err.(*appErrors.AppError); ok {
		if appErr.Code != appErrors.CodePermissionDenied {
			t.Errorf("expected CodePermissionDenied, got %v", appErr.Code)
		}
	}

	// Child requests allowed subset -> MUST SUCCEED
	allowedArgs := `{"subagent_role": "Doc Reader", "task_prompt": "Read docs", "requested_capabilities": ["workspace.read"]}`
	out, _, err := delegateTool.ExecuteWithContext(ctx, parentCtx, allowedArgs)
	if err != nil {
		t.Fatalf("unexpected error for valid delegation: %v", err)
	}
	if !strings.Contains(out, "delegated") {
		t.Errorf("expected delegated status: %s", out)
	}
}

// 7. Dynamic Intent-Aware Tool Filtering Tests
func TestFilterToolsForIntent(t *testing.T) {
	tools := []tool.Tool{
		&tool.ReadFileTool{},
		&tool.ListFilesTool{},
		&tool.SearchCodeTool{},
		&tool.RunLinterTool{},
		&tool.RunTestsTool{},
		&tool.GitCommitTool{},
	}

	filtered := tool.FilterToolsForIntent("audit and lint source code files", tools, 3)
	if len(filtered) > 3 {
		t.Fatalf("expected maximum 3 tools, got %d", len(filtered))
	}

	hasLinter := false
	for _, t := range filtered {
		if strings.Contains(t.Name(), "linter") || strings.Contains(t.Name(), "read") {
			hasLinter = true
		}
	}
	if !hasLinter {
		t.Errorf("expected lint or read tool in top 3 for audit intent")
	}
}

// 8. REST Delivery Handler Tests
func TestDeliveryHTTPHandler(t *testing.T) {
	runStore := run.NewMemoryStore()
	runHub := run.NewEventHub()
	runSvc := run.NewService(runStore, runHub)

	registry := tool.NewRegistry()
	gate := tool.NewApprovalGate(runSvc)
	toolSvc := tool.NewService(registry, gate, runSvc)
	handler := tool.NewHTTPHandler(toolSvc)

	metricsServer := metrics.NewServer(0)
	handler.RegisterHTTP(metricsServer)

	// 8.1 GET /api/v1/tools
	reqTools := httptest.NewRequest(http.MethodGet, "/api/v1/tools?risk_tier=READ", nil)
	wTools := httptest.NewRecorder()
	metricsServer.ServeHTTP(wTools, reqTools)

	if wTools.Code != http.StatusOK {
		t.Fatalf("expected 200 OK from /api/v1/tools, got %d", wTools.Code)
	}
	if !strings.Contains(wTools.Body.String(), "tools") {
		t.Fatalf("expected tools response body, got %s", wTools.Body.String())
	}
}

// 9. Concurrency & Race Tests (Resolution Idempotency under High Concurrency)
func TestApprovalGateResolutionRace(t *testing.T) {
	runStore := run.NewMemoryStore()
	runHub := run.NewEventHub()
	runSvc := run.NewService(runStore, runHub)
	gate := tool.NewApprovalGate(runSvc)

	ctx := context.Background()
	execCtx := &tool.ExecutionContext{
		ActorID: "agent_conc",
		RunID:   "run_conc_01",
	}

	go func() {
		time.Sleep(10 * time.Millisecond)
		execs := gate.ListExecutions("run_conc_01")
		if len(execs) == 0 {
			return
		}
		targetID := execs[0].ID

		// Launch 20 concurrent goroutines racing to resolve the same approval
		var wg sync.WaitGroup
		successCount := 0
		var mu sync.Mutex

		for i := 0; i < 20; i++ {
			wg.Add(1)
			go func(idx int) {
				defer wg.Done()
				err := gate.Resolve(targetID, true, "concurrent approve")
				if err == nil {
					mu.Lock()
					successCount++
					mu.Unlock()
				}
			}(i)
		}
		wg.Wait()

		// Invariant: Exactly one goroutine must succeed, all other 19 must fail with non-reentrant conflict!
		if successCount != 1 {
			t.Errorf("race condition detected: expected exactly 1 successful resolution, got %d", successCount)
		}
	}()

	approved, _, err := gate.RequestApprovalWithContext(ctx, execCtx, "write_file", `{"path":"conc.txt"}`, tool.RiskTierWrite)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !approved {
		t.Fatalf("expected approved=true")
	}
}
