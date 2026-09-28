package crew

import (
	"context"
	"fmt"
	"log/slog"
	"sync"
	"time"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/core/logger"
	"github.com/diezy-labs/claw-crew/engine/core/metrics"
	"github.com/diezy-labs/claw-crew/engine/pkg/client"
	"github.com/diezy-labs/claw-crew/engine/src/llm"
)

type service struct {
	mu          sync.RWMutex
	activeTurns map[string]context.CancelFunc
	llmProvider llm.Provider
	dispatcher  llm.ToolDispatcher
	gateway     client.SystemGatewayClient
}

// NewService creates a new crew.Orchestrator instance with LLM and tool gateway injected
func NewService(
	llmProvider llm.Provider,
	dispatcher llm.ToolDispatcher,
	gateway client.SystemGatewayClient,
) Orchestrator {
	return &service{
		activeTurns: make(map[string]context.CancelFunc),
		llmProvider: llmProvider,
		dispatcher:  dispatcher,
		gateway:     gateway,
	}
}

func (s *service) StartTurn(ctx context.Context, req *TurnRequest, eventCh chan<- *TurnEvent) error {
	log := logger.Get()

	if req.SessionID == "" {
		return appErrors.New(appErrors.CodeInvalidArgument, "session_id cannot be empty", appErrors.LayerService)
	}
	if req.Prompt == "" {
		return appErrors.New(appErrors.CodeInvalidArgument, "prompt cannot be empty", appErrors.LayerService)
	}

	turnCtx, cancel := context.WithCancel(ctx)
	s.mu.Lock()
	s.activeTurns[req.SessionID] = cancel
	s.mu.Unlock()

	var subagentWg sync.WaitGroup
	defer func() {
		s.mu.Lock()
		delete(s.activeTurns, req.SessionID)
		s.mu.Unlock()
		cancel()
		subagentWg.Wait() // Ensure all subagents wind down before eventCh can be closed by caller (BUG-001)
	}()

	metrics.ActiveAgents.Inc()
	defer metrics.ActiveAgents.Dec()

	start := time.Now()
	agentID := req.AgentID
	if agentID == "" {
		agentID = "primary_agent"
	}

	defer func() {
		duration := time.Since(start).Seconds()
		metrics.AgentTurnDuration.WithLabelValues(agentID, "completed").Observe(duration)
	}()

	log.InfoContext(ctx, "starting agent turn",
		slog.String("session_id", req.SessionID),
		slog.String("agent_id", agentID),
	)

	// 1. Prepare LLM ChatRequest
	chatReq := &llm.ChatRequest{
		Messages: []llm.Message{
			{Role: "user", Content: req.Prompt},
		},
		Tools: s.dispatcher.GetAvailableTools(),
	}

	chunkCh := make(chan *llm.ChatChunk, 32)
	llmErrCh := make(chan error, 1)

	// 2. Concurrently invoke LLM provider stream
	go func() {
		defer close(chunkCh)
		llmErrCh <- s.llmProvider.StreamChat(turnCtx, chatReq, chunkCh)
	}()

	// 3. Process incoming streaming chunks from LLM
	for chunk := range chunkCh {
		if chunk.Error != nil {
			return appErrors.Wrap(chunk.Error, appErrors.CodeInternal, "error receiving LLM chunk", appErrors.LayerService)
		}

		// Emit Thought chunk if present
		if chunk.ThoughtChunk != "" {
			select {
			case <-turnCtx.Done():
				return appErrors.New(appErrors.CodeTimeout, "turn cancelled or timed out", appErrors.LayerService)
			case eventCh <- &TurnEvent{
				Type:    EventThoughtChunk,
				Content: chunk.ThoughtChunk,
			}:
			}
		}

		// Emit Text chunk if present
		if chunk.ContentChunk != "" {
			select {
			case <-turnCtx.Done():
				return appErrors.New(appErrors.CodeTimeout, "turn cancelled or timed out", appErrors.LayerService)
			case eventCh <- &TurnEvent{
				Type:    EventTextChunk,
				Content: chunk.ContentChunk,
			}:
			}
		}

		// Process Tool Calls if emitted by LLM
		for _, tc := range chunk.ToolCalls {
			// Notify client that tool call started
			select {
			case <-turnCtx.Done():
				return appErrors.New(appErrors.CodeTimeout, "turn cancelled or timed out", appErrors.LayerService)
			case eventCh <- &TurnEvent{
				Type:    EventToolCallStarted,
				Content: fmt.Sprintf("Executing tool '%s' with arguments: %s", tc.Name, tc.Arguments),
			}:
			}

			// Dispatch tool execution
			res, err := s.dispatcher.Dispatch(turnCtx, &tc)
			if err != nil {
				log.WarnContext(ctx, "tool execution failed",
					slog.String("tool", tc.Name),
					slog.String("error", err.Error()),
				)
				select {
				case <-turnCtx.Done():
					return appErrors.New(appErrors.CodeTimeout, "turn cancelled or timed out", appErrors.LayerService)
				case eventCh <- &TurnEvent{
					Type:         EventError,
					Content:      fmt.Sprintf("Tool '%s' error: %s", tc.Name, err.Error()),
					ErrorMessage: err.Error(),
				}:
				}
				continue
			}

			// Handle subagent spawning
			if res.IsSubagentAction {
				select {
				case <-turnCtx.Done():
					return appErrors.New(appErrors.CodeTimeout, "turn cancelled or timed out", appErrors.LayerService)
				case eventCh <- &TurnEvent{
					Type:       EventSubagentSpawned,
					Content:    fmt.Sprintf("Spawned subagent '%s' for delegated task: %s", res.SubagentID, res.SubagentTask),
					SubagentID: res.SubagentID,
				}:
				}

				// Spawn subagent asynchronously with coordination
				subagentWg.Add(1)
				go func(subID, subTask string) {
					defer subagentWg.Done()
					s.runSubagent(turnCtx, subID, subTask, eventCh)
				}(res.SubagentID, res.SubagentTask)
			} else {
				// Tool execution completed successfully
				select {
				case <-turnCtx.Done():
					return appErrors.New(appErrors.CodeTimeout, "turn cancelled or timed out", appErrors.LayerService)
				case eventCh <- &TurnEvent{
					Type:    EventToolCallFinished,
					Content: fmt.Sprintf("Tool '%s' finished: %s", tc.Name, res.Output),
				}:
				}
			}
		}
	}

	// 4. Wait for LLM stream to conclude
	if err := <-llmErrCh; err != nil && err != context.Canceled {
		return appErrors.Wrap(err, appErrors.CodeInternal, "LLM streaming failed", appErrors.LayerService)
	}

	// 5. Wait for any running subagent goroutines to finish
	subagentWg.Wait()

	// 6. Emit Turn Completed event
	select {
	case <-turnCtx.Done():
		return appErrors.New(appErrors.CodeTimeout, "turn cancelled or timed out", appErrors.LayerService)
	case eventCh <- &TurnEvent{
		Type:    EventTurnCompleted,
		Content: "Turn completed successfully.",
	}:
	}

	log.InfoContext(ctx, "agent turn successfully completed",
		slog.String("session_id", req.SessionID),
		slog.String("agent_id", agentID),
	)

	return nil
}

// runSubagent handles parallel subagent turn execution
func (s *service) runSubagent(ctx context.Context, subagentID, task string, eventCh chan<- *TurnEvent) {
	metrics.ActiveAgents.Inc()
	defer metrics.ActiveAgents.Dec()

	subChunkCh := make(chan *llm.ChatChunk, 16)
	subReq := &llm.ChatRequest{
		Messages: []llm.Message{
			{Role: "system", Content: fmt.Sprintf("You are sub-agent '%s' tasked with: %s", subagentID, task)},
			{Role: "user", Content: task},
		},
	}

	go func() {
		defer close(subChunkCh)
		_ = s.llmProvider.StreamChat(ctx, subReq, subChunkCh)
	}()

	for chunk := range subChunkCh {
		if chunk.ContentChunk != "" {
			select {
			case <-ctx.Done():
				return
			case eventCh <- &TurnEvent{
				Type:       EventTextChunk,
				Content:    chunk.ContentChunk,
				SubagentID: subagentID,
			}:
			}
		}
	}
}

func (s *service) CancelTurn(ctx context.Context, sessionID string) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	cancel, found := s.activeTurns[sessionID]
	if !found {
		return appErrors.New(appErrors.CodeNotFound, fmt.Sprintf("no active turn found for session %s", sessionID), appErrors.LayerService)
	}

	cancel()
	delete(s.activeTurns, sessionID)
	return nil
}

// MemoryRegistry provides persistent storage for crews and agents
type MemoryRegistry struct {
	mu    sync.RWMutex
	crews map[string]*Squad
}

// NewRegistry creates a new Registry preloaded with standard default crews
func NewRegistry() Registry {
	r := &MemoryRegistry{
		crews: make(map[string]*Squad),
	}

	// Register default Research & Engineering Crew
	defaultCrew := &Squad{
		ID:              "crew_research_dev",
		Name:            "Research & Engineering Crew",
		Description:     "Multi-agent crew for code analysis, planning, implementation, and review.",
		CrewMemberCount: 3,
		Agents: []*CrewMember{
			{
				ID:           "planner",
				Name:         "Planner Agent",
				Role:         "Decompose high-level tasks into DAG task graph.",
				Status:       CrewMemberStatusIdle,
				Capabilities: []string{"planning", "task_breakdown"},
			},
			{
				ID:           "coder",
				Name:         "Code Specialist",
				Role:         "Implements code, refactors, and edits files.",
				Status:       CrewMemberStatusIdle,
				Capabilities: []string{"write_file", "edit_file", "read_file"},
			},
			{
				ID:           "reviewer",
				Name:         "Code Reviewer",
				Role:         "Validates code changes, generates git diffs, runs test suites.",
				Status:       CrewMemberStatusIdle,
				Capabilities: []string{"run_tests", "generate_diff"},
			},
		},
	}

	r.crews[defaultCrew.ID] = defaultCrew
	return r
}

func (r *MemoryRegistry) ListSquads(ctx context.Context) ([]*Squad, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	list := make([]*Squad, 0, len(r.crews))
	for _, c := range r.crews {
		list = append(list, c)
	}
	return list, nil
}

func (r *MemoryRegistry) GetSquad(ctx context.Context, id string) (*Squad, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	c, ok := r.crews[id]
	if !ok {
		return nil, appErrors.New(appErrors.CodeNotFound, fmt.Sprintf("crew not found: %s", id), appErrors.LayerService)
	}
	return c, nil
}

func (r *MemoryRegistry) RegisterSquad(ctx context.Context, crew *Squad) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	crew.CrewMemberCount = len(crew.Agents)
	r.crews[crew.ID] = crew
	return nil
}

func (r *MemoryRegistry) UpdateCrewMemberStatus(ctx context.Context, crewID, agentID string, status CrewMemberStatus) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	c, ok := r.crews[crewID]
	if !ok {
		return appErrors.New(appErrors.CodeNotFound, fmt.Sprintf("crew not found: %s", crewID), appErrors.LayerService)
	}
	for _, a := range c.Agents {
		if a.ID == agentID {
			a.Status = status
			return nil
		}
	}
	return appErrors.New(appErrors.CodeNotFound, fmt.Sprintf("agent %s not found in crew %s", agentID, crewID), appErrors.LayerService)
}
