package tool

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/diezy-labs/claw-crew/engine/src/run"
)

func TestSandboxPathValidation(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "sandbox_test_*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	// Valid inside path
	safe, err := ValidateSandboxPath(tempDir, "file.txt")
	if err != nil {
		t.Fatalf("expected valid path, got err: %v", err)
	}
	if !strings.HasPrefix(safe, filepath.Clean(tempDir)) {
		t.Fatalf("expected path to be inside sandbox")
	}

	// Path traversal attempt: ../../evil.txt
	_, err = ValidateSandboxPath(tempDir, "../../evil.txt")
	if err == nil {
		t.Fatal("expected traversal error, got nil")
	}
}

func TestApprovalGateApproveAndDeny(t *testing.T) {
	runStore := run.NewMemoryStore()
	runHub := run.NewEventHub()
	runSvc := run.NewService(runStore, runHub)

	gate := NewApprovalGate(runSvc)
	ctx := context.Background()

	// 1. Test Approval Granted
	go func() {
		time.Sleep(20 * time.Millisecond)
		execs := gate.ListExecutions("run_test_01")
		if len(execs) > 0 {
			_ = gate.Resolve(execs[0].ID, true, "approved by user")
		}
	}()

	approved, reason, err := gate.RequestApproval(ctx, "run_test_01", "write_file", `{"path":"a.txt"}`, RiskTierWrite)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !approved {
		t.Fatalf("expected approved=true, got reason=%s", reason)
	}

	// 2. Test Approval Denied
	go func() {
		time.Sleep(20 * time.Millisecond)
		execs := gate.ListExecutions("run_test_02")
		if len(execs) > 0 {
			_ = gate.Resolve(execs[0].ID, false, "security risk")
		}
	}()

	approved, reason, err = gate.RequestApproval(ctx, "run_test_02", "execute_command", `{"command":"rm -rf"}`, RiskTierExecute)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if approved {
		t.Fatal("expected approved=false")
	}
	if reason != "security risk" {
		t.Fatalf("expected reason 'security risk', got %s", reason)
	}
}

func TestBuiltinWriteAndReadFile(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "tools_test_*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	registry := NewRegistry()
	ctx := context.Background()

	// 1. Write file
	writer, err := registry.Get("write_file")
	if err != nil {
		t.Fatalf("failed to get write_file: %v", err)
	}

	writeOut, err := writer.Execute(ctx, `{"path":"hello.txt","content":"ClawCrew Phase 2"}`, tempDir)
	if err != nil {
		t.Fatalf("write_file error: %v", err)
	}
	if !strings.Contains(writeOut, "Successfully wrote") {
		t.Fatalf("unexpected write output: %s", writeOut)
	}

	// 2. Read file
	reader, err := registry.Get("read_file")
	if err != nil {
		t.Fatalf("failed to get read_file: %v", err)
	}

	readOut, err := reader.Execute(ctx, `{"path":"hello.txt"}`, tempDir)
	if err != nil {
		t.Fatalf("read_file error: %v", err)
	}
	if readOut != "ClawCrew Phase 2" {
		t.Fatalf("expected 'ClawCrew Phase 2', got '%s'", readOut)
	}

	// 3. Edit file
	editor, err := registry.Get("edit_file")
	if err != nil {
		t.Fatalf("failed to get edit_file: %v", err)
	}

	_, err = editor.Execute(ctx, `{"path":"hello.txt","target":"Phase 2","replacement":"Phase 2 Rocks"}`, tempDir)
	if err != nil {
		t.Fatalf("edit_file error: %v", err)
	}

	readOut2, _ := reader.Execute(ctx, `{"path":"hello.txt"}`, tempDir)
	if readOut2 != "ClawCrew Phase 2 Rocks" {
		t.Fatalf("expected 'ClawCrew Phase 2 Rocks', got '%s'", readOut2)
	}
}
