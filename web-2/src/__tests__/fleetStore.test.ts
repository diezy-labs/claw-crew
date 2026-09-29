import { describe, it, expect, beforeEach } from 'vitest';
import { useFleetStore } from '../store/fleetStore';

describe('FleetStore Zustand State Management', () => {
  beforeEach(() => {
    // Reset defaults if needed
  });

  it('initializes with default Pirate King context and dark theme', () => {
    const state = useFleetStore.getState();
    expect(state.theme).toBe('dark');
    expect(state.activeTab).toBe('quarterdeck');
    expect(state.realmName).toBe("Adiet’s Realm");
    expect(state.fleetName).toBe('Diezy Labs Fleet');
    expect(state.ships.length).toBeGreaterThanOrEqual(3);
    expect(state.crew.length).toBeGreaterThanOrEqual(6);
    expect(state.journalSessions.length).toBeGreaterThanOrEqual(3);
  });

  it('toggles theme between dark and light', () => {
    const { toggleTheme } = useFleetStore.getState();
    toggleTheme();
    expect(useFleetStore.getState().theme).toBe('light');
    toggleTheme();
    expect(useFleetStore.getState().theme).toBe('dark');
  });

  it('creates a new Quest with proper status and map steps', () => {
    const { createQuest } = useFleetStore.getState();
    const prevCount = useFleetStore.getState().quests.length;

    createQuest({
      title: 'Test Integration Suite Run',
      objective: 'Run automated end-to-end verification',
      priority: 'high',
      budgetLimitUSD: 1.50
    });

    const newQuests = useFleetStore.getState().quests;
    expect(newQuests.length).toBe(prevCount + 1);
    expect(newQuests[0].title).toBe('Test Integration Suite Run');
    expect(newQuests[0].status).toBe('ready');
    expect(newQuests[0].mapSteps.length).toBe(3);
  });

  it('runs a quest voyage and updates status to underway', () => {
    const { quests, runQuestVoyage } = useFleetStore.getState();
    const readyQuest = quests.find((q) => q.status === 'ready');
    expect(readyQuest).toBeDefined();

    if (readyQuest) {
      runQuestVoyage(readyQuest.id);
      const updated = useFleetStore.getState().quests.find((q) => q.id === readyQuest.id);
      expect(updated?.status).toBe('underway');
      expect(updated?.activeVoyageProgress).toBe(15);
    }
  });

  it('manages Captain’s Journal sessions, sending messages, and converting to artifacts', () => {
    const { createJournalSession, sendJournalMessage, togglePinJournalSession, convertJournalToArtifact } =
      useFleetStore.getState();
    const initialCount = useFleetStore.getState().journalSessions.length;

    createJournalSession('Exploratory AI Agent Concurrency Notes');
    const updatedSessions = useFleetStore.getState().journalSessions;
    expect(updatedSessions.length).toBe(initialCount + 1);
    const newSession = updatedSessions[0];
    expect(newSession.title).toBe('Exploratory AI Agent Concurrency Notes');

    sendJournalMessage(newSession.id, 'Let us evaluate goroutine channels vs mutex locks.');
    const updatedWithMsg = useFleetStore.getState().journalSessions.find((s) => s.id === newSession.id);
    expect(updatedWithMsg?.messages.length).toBe(2);

    togglePinJournalSession(newSession.id);
    const pinned = useFleetStore.getState().journalSessions.find((s) => s.id === newSession.id);
    expect(pinned?.isPinned).toBe(true);

    const prevArtifactsCount = useFleetStore.getState().artifacts.length;
    convertJournalToArtifact(newSession.id);
    expect(useFleetStore.getState().artifacts.length).toBe(prevArtifactsCount + 1);
  });

  it('handles Captain’s Approval decisions and records in logbook', () => {
    const { approvals, handleApproval } = useFleetStore.getState();
    const pending = approvals.find((a) => a.status === 'pending');
    expect(pending).toBeDefined();

    if (pending) {
      handleApproval(pending.id, 'approved');
      const updated = useFleetStore.getState().approvals.find((a) => a.id === pending.id);
      expect(updated?.status).toBe('approved');

      const log = useFleetStore.getState().logbook[0];
      expect(log.action).toContain('APPROVED');
    }
  });

  it('promotes an Artifact to Treasure', () => {
    const { artifacts, promoteArtifactToTreasure } = useFleetStore.getState();
    const reviewArt = artifacts.find((a) => a.status === 'needs_review');
    expect(reviewArt).toBeDefined();

    if (reviewArt) {
      promoteArtifactToTreasure(reviewArt.id);
      const updated = useFleetStore.getState().artifacts.find((a) => a.id === reviewArt.id);
      expect(updated?.status).toBe('treasure');
    }
  });
});
