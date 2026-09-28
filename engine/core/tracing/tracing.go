package tracing

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"log/slog"
	"sync"
	"time"

	"github.com/diezy-labs/claw-crew/engine/core/logger"
)

type contextKey string

const (
	currentSpanKey contextKey = "clawcrew_current_span"
)

// Span represents an OpenTelemetry-compatible tracing span
type Span struct {
	TraceID      string            `json:"trace_id"`
	SpanID       string            `json:"span_id"`
	ParentSpanID string            `json:"parent_span_id,omitempty"`
	Name         string            `json:"name"`
	StartTime    time.Time         `json:"start_time"`
	EndTime      *time.Time        `json:"end_time,omitempty"`
	Attributes   map[string]string `json:"attributes,omitempty"`
	mu           sync.Mutex
}

// GenerateID produces a random hex ID of the requested byte length
func GenerateID(bytes int) string {
	b := make([]byte, bytes)
	_, _ = rand.Read(b)
	return hex.EncodeToString(b)
}

// StartSpan creates a child span linked to parent context or root span
func StartSpan(ctx context.Context, name string) (context.Context, *Span) {
	now := time.Now().UTC()
	var traceID string
	var parentSpanID string

	if parent, ok := ctx.Value(currentSpanKey).(*Span); ok && parent != nil {
		traceID = parent.TraceID
		parentSpanID = parent.SpanID
	} else {
		traceID = GenerateID(16) // 128-bit trace ID
	}

	span := &Span{
		TraceID:      traceID,
		SpanID:       GenerateID(8), // 64-bit span ID
		ParentSpanID: parentSpanID,
		Name:         name,
		StartTime:    now,
		Attributes:   make(map[string]string),
	}

	childCtx := context.WithValue(ctx, currentSpanKey, span)
	return childCtx, span
}

// SetAttribute sets a key-value attribute on the span
func (s *Span) SetAttribute(key, value string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.Attributes[key] = value
}

// End finishes the span and emits a structured trace log entry
func (s *Span) End() {
	s.mu.Lock()
	defer s.mu.Unlock()

	now := time.Now().UTC()
	s.EndTime = &now
	duration := now.Sub(s.StartTime)

	log := logger.Get()
	attrs := []any{
		slog.String("trace_id", s.TraceID),
		slog.String("span_id", s.SpanID),
		slog.String("span_name", s.Name),
		slog.Duration("duration", duration),
	}
	if s.ParentSpanID != "" {
		attrs = append(attrs, slog.String("parent_span_id", s.ParentSpanID))
	}
	for k, v := range s.Attributes {
		attrs = append(attrs, slog.String(k, v))
	}

	log.Debug("trace span finished", attrs...)
}

// SpanFromContext retrieves the current span from context, if any
func SpanFromContext(ctx context.Context) *Span {
	if s, ok := ctx.Value(currentSpanKey).(*Span); ok {
		return s
	}
	return nil
}
