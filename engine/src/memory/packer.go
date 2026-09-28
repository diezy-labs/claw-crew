package memory

import (
	"fmt"
	"strings"
)

type boundedContextPacker struct{}

// NewContextPacker creates a new token-bounded ContextPacker
func NewContextPacker() ContextPacker {
	return &boundedContextPacker{}
}

func (p *boundedContextPacker) Pack(results []*SearchResult, maxTokens int) (string, int) {
	if len(results) == 0 || maxTokens <= 0 {
		return "", 0
	}

	var sb strings.Builder
	usedTokens := 0

	for i, res := range results {
		if res == nil || res.Document == nil {
			continue
		}

		docContent := strings.TrimSpace(res.Document.Content)
		if docContent == "" {
			continue
		}

		// Estimate tokens: ~4 chars per token + formatting overhead
		entry := fmt.Sprintf("[Document %d (Score: %.2f)]\n%s\n\n", i+1, res.Score, docContent)
		entryTokens := len(entry)/4 + 1

		if usedTokens+entryTokens > maxTokens {
			remainingTokens := maxTokens - usedTokens
			if remainingTokens > 20 { // only include truncated if reasonably meaningful
				maxChars := remainingTokens * 4
				if maxChars < len(docContent) {
					docContent = docContent[:maxChars] + "..."
				}
				truncatedEntry := fmt.Sprintf("[Document %d (Score: %.2f)]\n%s\n\n", i+1, res.Score, docContent)
				sb.WriteString(truncatedEntry)
				usedTokens += len(truncatedEntry)/4 + 1
			}
			break
		}

		sb.WriteString(entry)
		usedTokens += entryTokens
	}

	return strings.TrimSpace(sb.String()), usedTokens
}
