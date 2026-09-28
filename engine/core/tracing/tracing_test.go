package tracing

import (
	"context"
	"testing"
)

func TestTracingSpanLifecycle(t *testing.T) {
	ctx := context.Background()

	// 1. Root span
	rootCtx, rootSpan := StartSpan(ctx, "root_operation")
	if rootSpan.TraceID == "" || rootSpan.SpanID == "" {
		t.Fatalf("expected non-empty trace and span IDs")
	}
	if rootSpan.ParentSpanID != "" {
		t.Errorf("expected root span to have no parent")
	}

	rootSpan.SetAttribute("run_id", "run-trace-1")

	// 2. Child span
	_, childSpan := StartSpan(rootCtx, "child_tool_call")
	if childSpan.TraceID != rootSpan.TraceID {
		t.Errorf("expected child to inherit trace ID %s, got %s", rootSpan.TraceID, childSpan.TraceID)
	}
	if childSpan.ParentSpanID != rootSpan.SpanID {
		t.Errorf("expected child parentSpanID to be %s, got %s", rootSpan.SpanID, childSpan.ParentSpanID)
	}

	childSpan.End()
	rootSpan.End()

	if childSpan.EndTime == nil || rootSpan.EndTime == nil {
		t.Errorf("expected spans to have EndTime populated after End()")
	}
}
