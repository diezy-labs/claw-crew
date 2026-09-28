package tool

import (
	"os"
	"path/filepath"
	"testing"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
)

func TestValidateSandboxPath_Traversal(t *testing.T) {
	tempDir := t.TempDir()
	workspaceRoot := filepath.Join(tempDir, "workspace")
	if err := os.MkdirAll(workspaceRoot, 0755); err != nil {
		t.Fatalf("failed to create workspace: %v", err)
	}

	validFile := filepath.Join(workspaceRoot, "hello.txt")
	if err := os.WriteFile(validFile, []byte("world"), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	// 1. Valid path inside workspace
	res, err := ValidateSandboxPath(workspaceRoot, "hello.txt")
	if err != nil {
		t.Fatalf("expected valid path, got err: %v", err)
	}
	if res != validFile {
		t.Errorf("expected %s, got %s", validFile, res)
	}

	// 2. Traversal ../ outside workspace
	_, err = ValidateSandboxPath(workspaceRoot, "../outside.txt")
	if err == nil {
		t.Fatalf("expected error for ../ traversal, got nil")
	}
	if appErr, ok := err.(*appErrors.AppError); ok {
		if appErr.Code != appErrors.CodePermissionDenied {
			t.Errorf("expected CodePermissionDenied, got %v", appErr.Code)
		}
	}

	// 3. Sneaky relative traversal: sub/../../outside.txt
	_, err = ValidateSandboxPath(workspaceRoot, "sub/../../outside.txt")
	if err == nil {
		t.Fatalf("expected error for sneaky traversal, got nil")
	}

	// 4. Absolute path pointing outside workspace
	outsideFile := filepath.Join(tempDir, "outside.txt")
	_, err = ValidateSandboxPath(workspaceRoot, outsideFile)
	if err == nil {
		t.Fatalf("expected error for absolute path outside workspace, got nil")
	}
}

func TestValidateSandboxPath_SymlinkEscape(t *testing.T) {
	tempDir := t.TempDir()
	workspaceRoot := filepath.Join(tempDir, "workspace")
	outsideDir := filepath.Join(tempDir, "outside")
	if err := os.MkdirAll(workspaceRoot, 0755); err != nil {
		t.Fatalf("failed to create workspace: %v", err)
	}
	if err := os.MkdirAll(outsideDir, 0755); err != nil {
		t.Fatalf("failed to create outside dir: %v", err)
	}

	outsideTarget := filepath.Join(outsideDir, "secret.txt")
	if err := os.WriteFile(outsideTarget, []byte("confidential"), 0644); err != nil {
		t.Fatalf("failed to write outside file: %v", err)
	}

	symlinkPath := filepath.Join(workspaceRoot, "link_to_secret.txt")
	err := os.Symlink(outsideTarget, symlinkPath)
	if err != nil {
		t.Skipf("skipping symlink test (symlinks not permitted or supported): %v", err)
		return
	}

	// Attempt to access via symlink pointing outside
	_, err = ValidateSandboxPath(workspaceRoot, "link_to_secret.txt")
	if err == nil {
		t.Fatalf("expected error for symlink pointing outside workspace, got nil")
	}
	if appErr, ok := err.(*appErrors.AppError); ok {
		if appErr.Code != appErrors.CodePermissionDenied {
			t.Errorf("expected CodePermissionDenied, got %v", appErr.Code)
		}
	}
}
