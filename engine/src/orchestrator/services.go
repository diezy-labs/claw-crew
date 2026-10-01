// Package orchestrator is the embryo of the Quartermaster executive-planner
// (docs/finalize/01 "objective" branch). It turns a Pirate King objective into a
// proposed Squad draft via the LLM — the seed of QuartermasterService.IntakeObjective.
// It is NOT the crew Captain (crew.StartTurn): that is the orchestrator-worker that
// executes an approved objective. Not yet wired into the DI graph pending the
// quartermaster module (M2/F2-3).
package orchestrator

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"

	"github.com/diezy-labs/claw-crew/engine/src/crew"
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
	fullResponse, err := s.draftSquad(ctx, req.Objective)
	if err != nil {
		return nil, err
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

// IntakeObjective is the objective branch of the Quartermaster router: it drafts
// a plan via the LLM and returns a typed FleetOrderProposal that always awaits the
// Pirate King's approval. It NEVER starts a voyage — execution (Captain) happens
// only after the proposal is approved (docs/finalize/01, F2-3 acceptance).
func (s *orchestratorService) IntakeObjective(ctx context.Context, req ObjectiveRequest) (*FleetOrderProposal, error) {
	raw, err := s.draftSquad(ctx, req.Objective)
	if err != nil {
		return nil, err
	}

	proposal := &FleetOrderProposal{
		Objective: req.Objective,
		Status:    ProposalStatusAwaitingApproval,
	}

	// Prefer a structured LLM draft; fall back to a raw summary when the model
	// did not return clean JSON. Either way the proposal stays gated.
	var draft SquadDraft
	if err := json.Unmarshal([]byte(raw), &draft); err == nil && draft.MissionName != "" {
		proposal.MissionName = draft.MissionName
		proposal.Summary = fmt.Sprintf("Drafted Squad %q with %d proposed role(s).", draft.MissionName, len(draft.RequiredCrew))
		for _, m := range draft.RequiredCrew {
			reason := m.Name
			if len(m.Capabilities) > 0 {
				reason = strings.Join(m.Capabilities, ", ")
			}
			proposal.ProposedCrew = append(proposal.ProposedCrew, ProposedCrewRole{Role: m.Role, Reason: reason})
		}
	} else {
		proposal.MissionName = "Unnamed Fleet Order"
		proposal.Summary = strings.TrimSpace(raw)
	}
	return proposal, nil
}

// draftSquad runs one Quartermaster LLM completion for an objective and collects
// the stream. The producer goroutine owns closing chunkCh so the range below
// terminates (fixes the earlier deadlock).
func (s *orchestratorService) draftSquad(ctx context.Context, objective string) (string, error) {
	sysPrompt := "You are the Quartermaster of Galleon Fleet. Break down this objective into necessary specialized roles."
	chatReq := &llm.ChatRequest{
		Model:  "quartermaster-model",
		System: sysPrompt,
		Messages: []llm.Message{
			{Role: "user", Content: objective},
		},
	}

	chunkCh := make(chan *llm.ChatChunk)
	errCh := make(chan error, 1)
	go func() {
		defer close(chunkCh)
		errCh <- s.llmProvider.StreamChat(ctx, chatReq, chunkCh)
	}()

	var sb strings.Builder
	for chunk := range chunkCh {
		sb.WriteString(chunk.ContentChunk)
	}
	if err := <-errCh; err != nil {
		return "", fmt.Errorf("quartermaster LLM failed: %w", err)
	}
	return sb.String(), nil
}

// ProposeFleetOrder satisfies fleet.ObjectiveProposer: it drafts a gated proposal
// and maps it to the fleet-facing view. The dependency is inverted (fleet declares
// the interface) to avoid an import cycle, since this package imports fleet.
func (s *orchestratorService) ProposeFleetOrder(ctx context.Context, objective string) (*fleet.ProposedFleetOrder, error) {
	p, err := s.IntakeObjective(ctx, ObjectiveRequest{Objective: objective})
	if err != nil {
		return nil, err
	}
	return &fleet.ProposedFleetOrder{
		Objective:   p.Objective,
		MissionName: p.MissionName,
		Summary:     p.Summary,
		Status:      p.Status,
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
