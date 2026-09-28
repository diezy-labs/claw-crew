package tool

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"sort"
	"strings"
)

// NormalizeArguments takes raw JSON arguments, parses them, recursively sorts map keys,
// and produces canonical compact JSON string
func NormalizeArguments(rawJSON string) (string, error) {
	trimmed := strings.TrimSpace(rawJSON)
	if trimmed == "" {
		return "{}", nil
	}

	var data any
	if err := json.Unmarshal([]byte(trimmed), &data); err != nil {
		// If not valid JSON, treat as raw string if non-empty
		return "", fmt.Errorf("invalid json arguments: %w", err)
	}

	canonicalData := sortValue(data)
	normalizedBytes, err := json.Marshal(canonicalData)
	if err != nil {
		return "", fmt.Errorf("failed to marshal normalized json: %w", err)
	}

	return string(normalizedBytes), nil
}

// sortValue recursively processes map keys and slices for deterministic serialization
func sortValue(val any) any {
	switch v := val.(type) {
	case map[string]any:
		sortedMap := make(map[string]any, len(v))
		keys := make([]string, 0, len(v))
		for k := range v {
			keys = append(keys, k)
		}
		sort.Strings(keys)
		for _, k := range keys {
			sortedMap[k] = sortValue(v[k])
		}
		return sortedMap
	case []any:
		sortedSlice := make([]any, len(v))
		for i, elem := range v {
			sortedSlice[i] = sortValue(elem)
		}
		return sortedSlice
	default:
		return val
	}
}

// HashArguments returns the SHA-256 hash formatted as "sha256:<hex>" for the normalized arguments
func HashArguments(normalizedJSON string) string {
	sum := sha256.Sum256([]byte(normalizedJSON))
	return "sha256:" + hex.EncodeToString(sum[:])
}

// HashBytes computes the SHA-256 hash of arbitrary bytes formatted as "sha256:<hex>"
func HashBytes(data []byte) string {
	sum := sha256.Sum256(data)
	return "sha256:" + hex.EncodeToString(sum[:])
}

// HashFile calculates the SHA-256 hash of the target file for CAS validation
func HashFile(filePath string) (string, error) {
	data, err := os.ReadFile(filePath)
	if err != nil {
		return "", err
	}
	return HashBytes(data), nil
}
