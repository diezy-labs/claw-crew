package crew_test

import (
	"fmt"
	"sync"
	"testing"

	"github.com/diezy-labs/claw-crew/engine/src/run"
)

func TestEventOrdering_MonotonicSequence(t *testing.T) {
	hub := run.NewEventHub()
	store := run.NewMemoryStore()
	svc := run.NewService(store, hub)

	runID := "run_ordering_test"

	eventCh, unsub := svc.SubscribeEvents(runID)
	defer unsub()

	const numEvents = 50
	var wg sync.WaitGroup
	wg.Add(numEvents)

	// Publish events concurrently
	for i := 0; i < numEvents; i++ {
		go func(idx int) {
			defer wg.Done()
			svc.PublishEvent(runID, "test.event", map[string]any{"index": idx}, "")
		}(i)
	}

	wg.Wait()

	// Collect events
	received := make([]*run.RunEvent, 0, numEvents)
	for i := 0; i < numEvents; i++ {
		evt := <-eventCh
		received = append(received, evt)
	}

	// Verify all sequences 1..numEvents are present and unique
	seen := make(map[int64]bool)
	for _, evt := range received {
		if evt.Sequence < 1 || evt.Sequence > int64(numEvents) {
			t.Errorf("unexpected sequence number: %d (expected 1..%d)", evt.Sequence, numEvents)
		}
		if seen[evt.Sequence] {
			t.Errorf("duplicate sequence number: %d", evt.Sequence)
		}
		seen[evt.Sequence] = true
	}

	if len(seen) != numEvents {
		t.Errorf("expected %d distinct sequences, got %d", numEvents, len(seen))
	}
}

func TestEventOrdering_SubscribeSinceReplay(t *testing.T) {
	hub := run.NewEventHub()
	store := run.NewMemoryStore()
	svc := run.NewService(store, hub)

	runID := "run_replay_test"

	var published []*run.RunEvent
	for i := 1; i <= 5; i++ {
		evt := svc.PublishEvent(runID, "chunk", map[string]any{"num": i}, "")
		published = append(published, evt)
	}

	// Resume from event #3 (last seen was #3, so should replay #4 and #5)
	lastSeenID := published[2].EventID
	replayCh, unsub := svc.SubscribeEventsSince(runID, lastSeenID)
	defer unsub()

	var replayed []*run.RunEvent
	for len(replayed) < 2 {
		select {
		case evt := <-replayCh:
			replayed = append(replayed, evt)
		default:
			t.Fatalf("channel drained early; got %d events, expected 2", len(replayed))
		}
	}

	if replayed[0].Sequence != 4 || replayed[1].Sequence != 5 {
		t.Errorf("expected replayed events to have sequences [4, 5], got [%d, %d]",
			replayed[0].Sequence, replayed[1].Sequence)
	}

	if replayed[0].EventID != published[3].EventID || replayed[1].EventID != published[4].EventID {
		t.Errorf("replayed event IDs did not match expected subsequent events")
	}
	_ = fmt.Sprintf("replay verified")
}
