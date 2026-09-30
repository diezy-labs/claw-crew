package orchestrator

import (
	"context"
	"encoding/json"
	"fmt"

	"github.com/diezy-labs/claw-crew/engine/src/crew" // which is now Squad DTOs, wait. Did I rename the folder? No, just DTO names inside it.
	"github.com/diezy-labs/claw-crew/engine/src/fleet"
	"github.com/diezy-labs/claw-crew/engine/src/llm"
)

type orchestratorService struct {
	llmProvider  llm.Provider
	fleetService fleet.Service
}

func NewService(llmProvider llm.Provider, fleetService fleet.Service) Service {
	return &orchestratorService{
		llmProvider:  llmProvider,
		fleetService: fleetService,
	}
}

func (s *orchestratorService) ProcessObjective(ctx context.Context, req ObjectiveRequest) (*ObjectiveResponse, error) {
	// Quartermaster Intelligence: Parses global objective
	// For now, we simulate parsing and delegating

	sysPrompt := "You are the Quartermaster of Galleon Fleet. Break down this objective into necessary specialized roles."
	chatReq := &llm.ChatRequest{
		Model:  "quartermaster-model",
		System: sysPrompt,
		Messages: []llm.Message{
			{Role: "user", Content: req.Objective},
		},
	}

	// We use the streaming interface but collect it
	chunkCh := make(chan *llm.ChatChunk)
	errCh := make(chan error, 1)

	go func() {
		errCh <- s.llmProvider.StreamChat(ctx, chatReq, chunkCh)
	}()

	var fullResponse string
	for chunk := range chunkCh {
		fullResponse += chunk.ContentChunk
	}

	if err := <-errCh; err != nil {
		return nil, fmt.Errorf("quartermaster LLM failed: %w", err)
	}

	// Attempt to parse JSON response into SquadDraft
	var draft SquadDraft
	if err := json.Unmarshal([]byte(fullResponse), &draft); err != nil {
		// If LLM didn't return pure JSON, we wrap it in a raw response
		return &ObjectiveResponse{
			Status: "Drafted (Raw): " + fullResponse,
		}, nil
	}

	// Return structured draft response
	return &ObjectiveResponse{
		Status: fmt.Sprintf("Successfully drafted Squad for %s with %d members", draft.MissionName, len(draft.RequiredCrew)),
	}, nil
}

func (s *orchestratorService) CoordinateFleet(ctx context.Context, fleetID string) error {
	_, err := s.fleetService.GetFleet(ctx, fleetID)
	if err != nil {
		return fmt.Errorf("cannot coordinate unknown fleet: %w", err)
	}
	// Logic to check Ship statuses, missions, budgets...
	return nil
}

// SquadDraft represents the JSON structure we expect the Quartermaster LLM to output
type SquadDraft struct {
	MissionName  string            `json:"mission_name"`
	RequiredCrew []crew.CrewMember `json:"required_crew"`
}
