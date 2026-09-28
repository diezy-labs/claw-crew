package task

import (
	"context"
	"sync"
	"time"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
)

// Job represents a background task execution payload
type Job struct {
	ID       string
	RunID    string
	Execute  func(ctx context.Context) error
	OnFinish func(err error)
}

// WorkerPool coordinates a bounded set of background worker goroutines
type WorkerPool struct {
	maxWorkers int
	queue      chan Job
	wg         sync.WaitGroup
	ctx        context.Context
	cancel     context.CancelFunc
	mu         sync.Mutex
	stopped    bool
}

// NewWorkerPool creates a new bounded worker pool
func NewWorkerPool(maxWorkers int, queueCapacity int) *WorkerPool {
	if maxWorkers <= 0 {
		maxWorkers = 4
	}
	if queueCapacity <= 0 {
		queueCapacity = 100
	}

	ctx, cancel := context.WithCancel(context.Background())
	pool := &WorkerPool{
		maxWorkers: maxWorkers,
		queue:      make(chan Job, queueCapacity),
		ctx:        ctx,
		cancel:     cancel,
	}

	pool.start()
	return pool
}

func (p *WorkerPool) start() {
	for i := 0; i < p.maxWorkers; i++ {
		p.wg.Add(1)
		go p.workerLoop(i)
	}
}

func (p *WorkerPool) workerLoop(workerID int) {
	defer p.wg.Done()

	for {
		select {
		case <-p.ctx.Done():
			return
		case job, ok := <-p.queue:
			if !ok {
				return
			}
			p.executeJob(job)
		}
	}
}

func (p *WorkerPool) executeJob(job Job) {
	if job.Execute == nil {
		return
	}

	jobCtx, cancel := context.WithTimeout(p.ctx, 10*time.Minute)
	defer cancel()

	var err error
	done := make(chan struct{})

	go func() {
		defer close(done)
		err = job.Execute(jobCtx)
	}()

	select {
	case <-jobCtx.Done():
		if job.OnFinish != nil {
			job.OnFinish(jobCtx.Err())
		}
	case <-done:
		if job.OnFinish != nil {
			job.OnFinish(err)
		}
	}
}

// Submit enqueues a background job for worker execution
func (p *WorkerPool) Submit(ctx context.Context, job Job) error {
	p.mu.Lock()
	if p.stopped {
		p.mu.Unlock()
		return appErrors.New(appErrors.CodeUnavailable, "worker pool is stopped", appErrors.LayerService)
	}
	p.mu.Unlock()

	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-p.ctx.Done():
		return appErrors.New(appErrors.CodeUnavailable, "worker pool context cancelled", appErrors.LayerService)
	case p.queue <- job:
		return nil
	default:
		return appErrors.New(appErrors.CodeUnavailable, "worker pool queue full", appErrors.LayerService)
	}
}

// Stop gracefully terminates all worker goroutines
func (p *WorkerPool) Stop() {
	p.mu.Lock()
	if p.stopped {
		p.mu.Unlock()
		return
	}
	p.stopped = true
	p.mu.Unlock()

	p.cancel()
	close(p.queue)
	p.wg.Wait()
}
