package logger

import (
	"log/slog"
	"strings"
	"testing"
)

func TestRedactAttr(t *testing.T) {
	tests := []struct {
		key      string
		val      string
		expected string
	}{
		{"api_key", "sk-123456789", "[REDACTED]"},
		{"token", "ghp_abcdef123456", "[REDACTED]"},
		{"user_password", "supersecret", "[REDACTED]"},
		{"authorization", "Bearer xyz", "[REDACTED]"},
		{"normal_field", "regular_value", "regular_value"},
		{"output", "Calling with Bearer 1234567890abcdef...", "Calling with Bearer [REDACTED]"},
		{"command_result", "Key is sk-12345678901234567890 for auth", "Key is sk-...[REDACTED] for auth"},
	}

	for _, tt := range tests {
		attr := slog.String(tt.key, tt.val)
		redacted := RedactAttr(nil, attr)
		if redacted.Value.String() != tt.expected {
			t.Errorf("key %q: expected value %q, got %q", tt.key, tt.expected, redacted.Value.String())
		}
	}
}

func TestRedactString(t *testing.T) {
	raw := "Use Authorization: Bearer my-secret-jwt-token-value and sk-1234567890abcdef12345"
	cleaned := RedactString(raw)
	if !strings.Contains(cleaned, "Bearer [REDACTED]") {
		t.Errorf("expected bearer token to be redacted, got: %s", cleaned)
	}
	if !strings.Contains(cleaned, "sk-...[REDACTED]") {
		t.Errorf("expected sk key to be redacted, got: %s", cleaned)
	}
}
