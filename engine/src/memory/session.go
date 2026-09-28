package memory

import (
	"context"
	"sync"
	"time"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
)

type inMemorySessionStore struct {
	mu       sync.RWMutex
	sessions map[string][]*Message
}

// NewSessionMemory creates a new in-memory short-term session memory store
func NewSessionMemory() SessionMemory {
	return &inMemorySessionStore{
		sessions: make(map[string][]*Message),
	}
}

func (s *inMemorySessionStore) Append(ctx context.Context, sessionID string, msg *Message) error {
	if sessionID == "" {
		return appErrors.New(appErrors.CodeInvalidArgument, "session ID cannot be empty", appErrors.LayerRepository)
	}
	if msg == nil {
		return appErrors.New(appErrors.CodeInvalidArgument, "message cannot be nil", appErrors.LayerRepository)
	}

	select {
	case <-ctx.Done():
		return ctx.Err()
	default:
	}

	if msg.CreatedAt.IsZero() {
		msg.CreatedAt = time.Now().UTC()
	}
	if msg.TokenCount <= 0 {
		// Heuristic: ~4 chars per token
		msg.TokenCount = len(msg.Content)/4 + 1
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	s.sessions[sessionID] = append(s.sessions[sessionID], msg)
	return nil
}

func (s *inMemorySessionStore) GetHistory(ctx context.Context, sessionID string) ([]*Message, error) {
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	default:
	}

	s.mu.RLock()
	defer s.mu.RUnlock()

	history, ok := s.sessions[sessionID]
	if !ok {
		return []*Message{}, nil
	}

	result := make([]*Message, len(history))
	copy(result, history)
	return result, nil
}

func (s *inMemorySessionStore) Clear(ctx context.Context, sessionID string) error {
	select {
	case <-ctx.Done():
		return ctx.Err()
	default:
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	delete(s.sessions, sessionID)
	return nil
}
