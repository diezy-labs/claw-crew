package memory

import (
	"context"

	"github.com/diezy-labs/claw-crew/engine/src/run"
)

type memoryService struct {
	session    SessionMemory
	vector     VectorStore
	packer     ContextPacker
	runService run.Service
}

// NewService creates a coordinated Memory Service
func NewService(session SessionMemory, vector VectorStore, packer ContextPacker, runService run.Service) Service {
	return &memoryService{
		session:    session,
		vector:     vector,
		packer:     packer,
		runService: runService,
	}
}

func (s *memoryService) Session() SessionMemory {
	return s.session
}

func (s *memoryService) Vector() VectorStore {
	return s.vector
}

func (s *memoryService) Packer() ContextPacker {
	return s.packer
}

func (s *memoryService) RetrieveWithTrace(ctx context.Context, runID string, query string, topK int, maxTokens int) (string, error) {
	results, err := s.vector.SearchByText(ctx, query, topK)
	if err != nil {
		return "", err
	}

	packed, usedTokens := s.packer.Pack(results, maxTokens)

	if s.runService != nil && runID != "" {
		scores := make([]float32, 0, len(results))
		docIDs := make([]string, 0, len(results))
		for _, res := range results {
			if res != nil && res.Document != nil {
				scores = append(scores, res.Score)
				docIDs = append(docIDs, res.Document.ID)
			}
		}

		s.runService.PublishEvent(runID, "memory.retrieved", map[string]interface{}{
			"query":         query,
			"top_k":         topK,
			"results_count": len(results),
			"packed_tokens": usedTokens,
			"scores":        scores,
			"doc_ids":       docIDs,
		}, "")
	}

	return packed, nil
}
