package task

import (
	"context"
	"sync/atomic"
	"testing"
	"time"
)

func TestWorkerPool_Execution(t *testing.T) {
	pool := NewWorkerPool(3, 10)
	defer pool.Stop()

	var counter int32
	jobCount := 5

	doneCh := make(chan struct{}, jobCount)

	for i := 0; i < jobCount; i++ {
		err := pool.Submit(context.Background(), Job{
			ID:    "job-test",
			RunID: "run-test",
			Execute: func(ctx context.Context) error {
				atomic.AddInt32(&counter, 1)
				return nil
			},
			OnFinish: func(err error) {
				doneCh <- struct{}{}
			},
		})
		if err != nil {
			t.Fatalf("failed to submit job: %v", err)
		}
	}

	for i := 0; i < jobCount; i++ {
		select {
		case <-doneCh:
		case <-time.After(2 * time.Second):
			t.Fatalf("timed out waiting for jobs to complete")
		}
	}

	if atomic.LoadInt32(&counter) != int32(jobCount) {
		t.Fatalf("expected counter %d, got %d", jobCount, counter)
	}
}
