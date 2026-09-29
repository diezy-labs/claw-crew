import { create } from 'zustand';
import {
  ThemeMode,
  NavigationTab,
  Ship,
  CrewMember,
  Quest,
  Artifact,
  CaptainApproval,
  LogbookEntry,
  TreasuryLedger,
  NotificationItem,
  ChatMessage,
  JournalSession,
  QuestStatus,
  SettingsCategory,
  FleetSettings
} from '../types';

interface FleetState {
  theme: ThemeMode;
  activeTab: NavigationTab;
  isCommandPaletteOpen: boolean;
  realmName: string;
  fleetName: string;
  selectedWorkspace: string;
  selectedProject: string;
  workspaces: string[];
  projects: string[];
  ships: Ship[];
  crew: CrewMember[];
  quests: Quest[];
  artifacts: Artifact[];
  approvals: CaptainApproval[];
  logbook: LogbookEntry[];
  treasuryLedger: TreasuryLedger[];
  notifications: NotificationItem[];
  chatMessages: ChatMessage[];
  journalSessions: JournalSession[];
  selectedJournalSessionId: string | null;
  selectedQuestId: string | null;
  selectedArtifactId: string | null;
  activeSettingsCategory: SettingsCategory;
  settings: FleetSettings;
  isSidebarCollapsed: boolean;
  isMobileSidebarOpen: boolean;
  isAnchorDropped: boolean;
  isFleetPulseOpen: boolean;

  // Actions
  setTheme: (theme: ThemeMode) => void;
  toggleTheme: () => void;
  setActiveTab: (tab: NavigationTab) => void;
  setActiveSettingsCategory: (cat: SettingsCategory) => void;
  setCommandPaletteOpen: (open: boolean) => void;
  toggleSidebarCollapsed: () => void;
  setSidebarCollapsed: (collapsed: boolean) => void;
  setMobileSidebarOpen: (open: boolean) => void;
  toggleAnchor: () => void;
  setFleetPulseOpen: (open: boolean) => void;
  toggleFleetPulse: () => void;
  setSelectedWorkspace: (ws: string) => void;
  setSelectedProject: (proj: string) => void;
  setSelectedQuestId: (id: string | null) => void;
  setSelectedArtifactId: (id: string | null) => void;
  setSelectedJournalSessionId: (id: string | null) => void;
  
  // Settings Actions
  updateSettings: (updater: (prev: FleetSettings) => FleetSettings, logAudit?: string) => void;
  resetSettingsCategory: (category: SettingsCategory) => void;
  
  // Business Actions
  sendQuartermasterMessage: (content: string) => void;
  createQuest: (quest: Partial<Quest>) => void;
  updateQuestStatus: (id: string, status: QuestStatus) => void;
  runQuestVoyage: (id: string) => void;
  handleApproval: (id: string, decision: 'approved' | 'rejected') => void;
  promoteArtifactToTreasure: (id: string) => void;
  saveArtifact: (artifact: Omit<Artifact, 'id' | 'createdAt'>) => void;
  addCrewMember: (crewData: Omit<CrewMember, 'id'>) => void;
  markNotificationRead: (id: string) => void;
  markAllNotificationsRead: () => void;
  addNotification: (notification: Omit<NotificationItem, 'id' | 'createdAt' | 'read'>) => void;
  simulateVoyageTick: () => void;

  // Captain's Journal Actions
  createJournalSession: (title: string, workspaceId?: string) => void;
  sendJournalMessage: (sessionId: string, content: string) => void;
  togglePinJournalSession: (id: string) => void;
  archiveJournalSession: (id: string) => void;
  convertJournalToArtifact: (sessionId: string) => void;
  convertJournalToQuest: (sessionId: string) => void;
}

const initialShips: Ship[] = [
  {
    id: 'ship-dev',
    name: 'Developer Delivery Ship',
    fleetId: 'fleet-diezy',
    tagline: 'Persistent specialist team for delivery quality, repository health, and release readiness.',
    homeScope: 'engineering',
    navigatorName: 'Horizon (Orchestrator)',
    status: 'active',
    activeVoyagesCount: 1,
    monthlySpentUSD: 4.10,
    crewIds: ['crew-repo-analyst', 'crew-eng-planner', 'crew-qa-reviewer'],
    charter: {
      purpose: 'Maintain delivery quality, repository health, and release readiness under read-first policy.',
      acceptedQuestTypes: ['repository_health', 'ci_triage', 'pr_review', 'release_readiness'],
      crewAuthority: 'Read repository and run tests. External write requires Captain’s Approval.',
      prohibitedActions: ['Merge pull request directly', 'Deploy to production without approval', 'Rotate infrastructure secrets'],
      budgetPerVoyageUSD: 2.00,
      monthlyBudgetUSD: 25.00,
      memorySharing: 'ship_scoped'
    }
  },
  {
    id: 'ship-market',
    name: 'Marketing Launch Ship',
    fleetId: 'fleet-diezy',
    tagline: 'Audience research, product launch positioning, and content calendar preparation.',
    homeScope: 'marketing',
    navigatorName: 'Beacon (Navigator)',
    status: 'active',
    activeVoyagesCount: 0,
    monthlySpentUSD: 0.88,
    crewIds: ['crew-market-analyst', 'crew-brand-reviewer'],
    charter: {
      purpose: 'Develop market intelligence and high-converting launch copy with strict human verification.',
      acceptedQuestTypes: ['competitor_research', 'audience_research', 'positioning_strategy', 'content_draft'],
      crewAuthority: 'Draft copy, extract audience signals. External publication requires Captain’s Approval.',
      prohibitedActions: ['Direct social publishing', 'Outbound email blast', 'External media buy'],
      budgetPerVoyageUSD: 2.00,
      monthlyBudgetUSD: 20.00,
      memorySharing: 'project_scoped'
    }
  },
  {
    id: 'ship-research',
    name: 'Research & Decision Ship',
    fleetId: 'fleet-diezy',
    tagline: 'Synthesizing technical architectural decisions, vendor benchmarks, and risk evaluations.',
    homeScope: 'research',
    navigatorName: 'Sextant (Lead)',
    status: 'active',
    activeVoyagesCount: 0,
    monthlySpentUSD: 1.25,
    crewIds: ['crew-tech-evaluator'],
    charter: {
      purpose: 'Provide rigorous evidence-backed decision briefs and comparative trade-off analyses.',
      acceptedQuestTypes: ['vendor_analysis', 'architecture_decision', 'security_audit'],
      crewAuthority: 'Read-only search, documentation parsing, synthetic benchmarking.',
      prohibitedActions: ['Credential generation', 'External payment commits'],
      budgetPerVoyageUSD: 1.50,
      monthlyBudgetUSD: 15.00,
      memorySharing: 'isolated'
    }
  }
];

const initialCrew: CrewMember[] = [
  {
    id: 'crew-repo-analyst',
    name: 'Repository Analyst',
    shipId: 'ship-dev',
    role: 'Static Analysis & Codebase Cartographer',
    purpose: 'Maps repository architecture, dependencies, git history, and technical debt risks.',
    skills: ['AST Parsing', 'Dependency Graphing', 'Commit Chronology', 'Risk Scoring'],
    tools: ['local_filesystem', 'git_log_parser', 'repo_scanner'],
    modelProfile: 'Claude 3.7 Sonnet (Reasoning)',
    authority: 'read_only',
    memoryScope: 'ship',
    status: 'active',
    lastVoyage: '12m ago',
    costLast30Days: 1.84
  },
  {
    id: 'crew-eng-planner',
    name: 'Engineering Planner',
    shipId: 'ship-dev',
    role: 'Work Breakdown & Dependency Strategist',
    purpose: 'Formulates phased implementation plans, test suites, and rollback strategies.',
    skills: ['Architecture Breakdown', 'Milestone Estimation', 'Interface Specification'],
    tools: ['quest_map_builder', 'markdown_architect'],
    modelProfile: 'Gemini 2.5 Pro (Balanced)',
    authority: 'draft_only',
    memoryScope: 'ship',
    status: 'active',
    lastVoyage: '35m ago',
    costLast30Days: 1.32
  },
  {
    id: 'crew-qa-reviewer',
    name: 'QA & Risk Reviewer',
    shipId: 'ship-dev',
    role: 'Safety & Test Verification Specialist',
    purpose: 'Audits edge cases, race conditions, test coverage, and external side-effects.',
    skills: ['Regression Hunting', 'Security Rule Linting', 'Failure Boundary Audit'],
    tools: ['test_runner', 'linter_checker'],
    modelProfile: 'Claude 3.5 Haiku (Fast & Precise)',
    authority: 'gated_write',
    memoryScope: 'ship',
    status: 'active',
    lastVoyage: '5m ago',
    costLast30Days: 0.94
  },
  {
    id: 'crew-market-analyst',
    name: 'Market Researcher',
    shipId: 'ship-market',
    role: 'Competitive Landscape & Audience Analyst',
    purpose: 'Tracks AI agent tools, BYOK user trends, and developer community expectations.',
    skills: ['Signal Extraction', 'Feature Comparison Matrix', 'Audience Persona Mapping'],
    tools: ['web_reader', 'sentiment_analyzer'],
    modelProfile: 'Gemini 2.5 Flash',
    authority: 'read_only',
    memoryScope: 'workspace',
    status: 'active',
    lastVoyage: '3h ago',
    costLast30Days: 0.52
  },
  {
    id: 'crew-brand-reviewer',
    name: 'Brand & Tone Reviewer',
    shipId: 'ship-market',
    role: 'Narrative Alignment & Copy Polish',
    purpose: 'Ensures the Pirate King / Fleet narrative balances calm professional authority with memorable identity.',
    skills: ['Editorial Consistency', 'Zero-Pill Compliance', 'Value Proposition Polish'],
    tools: ['style_guide_checker'],
    modelProfile: 'Claude 3.5 Sonnet',
    authority: 'draft_only',
    memoryScope: 'ship',
    status: 'active',
    lastVoyage: '6h ago',
    costLast30Days: 0.36
  },
  {
    id: 'crew-tech-evaluator',
    name: 'Technical Evaluator',
    shipId: 'ship-research',
    role: 'Benchmarking & Systems Architect',
    purpose: 'Assesses Go engine concurrency vs Rust host sandbox security trade-offs.',
    skills: ['Memory Profile Audit', 'Latency Benchmark', 'Security Tiering'],
    tools: ['bench_profiler', 'matrix_evaluator'],
    modelProfile: 'DeepSeek R1 / Local Ollama',
    authority: 'read_only',
    memoryScope: 'crew',
    status: 'active',
    lastVoyage: '1d ago',
    costLast30Days: 1.25
  }
];

const initialQuests: Quest[] = [
  {
    id: 'quest-release-v14',
    title: 'Prepare Release v1.4 Package',
    objective: 'Compile comprehensive release-readiness brief, verify CI pipeline stability, and draft changelog.',
    workspaceId: 'Product Platform',
    projectId: 'v1.4 Release Readiness',
    priority: 'high',
    status: 'ready',
    suggestedShipId: 'ship-dev',
    assignedShipId: 'ship-dev',
    requiredArtifacts: ['Release Readiness Checklist', 'CI & Test Evidence Brief', 'Changelog Draft'],
    budgetLimitUSD: 2.00,
    estimatedCostUSD: 0.85,
    mapSteps: [
      { stepNumber: 1, title: 'Analyze merged pull requests in feat/enhance-agent-phase', assignedCrewId: 'crew-repo-analyst', status: 'completed', outputArtifactType: 'git-diff-summary' },
      { stepNumber: 2, title: 'Inspect CI integration test timeout runs', assignedCrewId: 'crew-qa-reviewer', status: 'in_progress', outputArtifactType: 'ci-triage' },
      { stepNumber: 3, title: 'Formulate release checklist and migration guide', assignedCrewId: 'crew-eng-planner', status: 'pending', outputArtifactType: 'readiness-checklist' },
      { stepNumber: 4, title: 'Request Captain’s Approval for GitHub Draft Release', assignedCrewId: 'crew-qa-reviewer', status: 'pending' }
    ],
    activeVoyageProgress: 45,
    createdAt: '2026-09-28T09:15:00Z',
    updatedAt: '2026-09-28T10:45:00Z',
    discoveriesCount: 3
  },
  {
    id: 'quest-ci-triage',
    title: 'Triage Integration Test Flakiness',
    objective: 'Investigate recurring timeout in test suite container teardown across recent voyages.',
    workspaceId: 'Product Platform',
    projectId: 'v1.4 Release Readiness',
    priority: 'urgent',
    status: 'underway',
    suggestedShipId: 'ship-dev',
    assignedShipId: 'ship-dev',
    requiredArtifacts: ['CI Triage Report', 'Remediation Proposal'],
    budgetLimitUSD: 1.50,
    estimatedCostUSD: 0.42,
    mapSteps: [
      { stepNumber: 1, title: 'Examine Go engine concurrency traces', assignedCrewId: 'crew-repo-analyst', status: 'completed' },
      { stepNumber: 2, title: 'Isolate race condition in WebSocket teardown', assignedCrewId: 'crew-qa-reviewer', status: 'in_progress' },
      { stepNumber: 3, title: 'Formulate fix patch & draft GitHub issue', assignedCrewId: 'crew-eng-planner', status: 'pending' }
    ],
    activeVoyageProgress: 68,
    createdAt: '2026-09-28T10:00:00Z',
    updatedAt: '2026-09-28T11:15:00Z',
    discoveriesCount: 2
  },
  {
    id: 'quest-github-issue-draft',
    title: 'Create GitHub Issue: WebSocket Teardown Timeout',
    objective: 'Draft issue in diezy-labs/claw-crew highlighting the reproduction steps and proposed fix.',
    workspaceId: 'Product Platform',
    projectId: 'Claw Crew Agent Phase 2',
    priority: 'high',
    status: 'awaiting_captain',
    suggestedShipId: 'ship-dev',
    assignedShipId: 'ship-dev',
    requiredArtifacts: ['GitHub Issue Draft'],
    budgetLimitUSD: 0.50,
    estimatedCostUSD: 0.12,
    mapSteps: [
      { stepNumber: 1, title: 'Draft issue body and stack trace attachments', assignedCrewId: 'crew-qa-reviewer', status: 'completed' },
      { stepNumber: 2, title: 'Captain’s Approval required for GitHub API write', status: 'in_progress' }
    ],
    activeVoyageProgress: 90,
    createdAt: '2026-09-28T11:00:00Z',
    updatedAt: '2026-09-28T11:20:00Z',
    discoveriesCount: 1
  },
  {
    id: 'quest-repo-health',
    title: 'Repository Health & Dependency Audit',
    objective: 'Verify all lockfiles, clean architecture domain boundaries, and Go/Rust safety constraints.',
    workspaceId: 'Product Platform',
    projectId: 'Core Infrastructure',
    priority: 'medium',
    status: 'treasured',
    suggestedShipId: 'ship-dev',
    assignedShipId: 'ship-dev',
    requiredArtifacts: ['Repository Health Brief'],
    budgetLimitUSD: 1.00,
    estimatedCostUSD: 0.38,
    mapSteps: [
      { stepNumber: 1, title: 'Audit engine/src package separation', assignedCrewId: 'crew-repo-analyst', status: 'completed' },
      { stepNumber: 2, title: 'Verify Tauri Landlock sandboxing boundaries', assignedCrewId: 'crew-qa-reviewer', status: 'completed' }
    ],
    activeVoyageProgress: 100,
    createdAt: '2026-09-27T14:00:00Z',
    updatedAt: '2026-09-28T08:30:00Z',
    discoveriesCount: 4
  },
  {
    id: 'quest-byok-treasury',
    title: 'BYOK Multi-Model Routing Strategy',
    objective: 'Evaluate cost savings of routing static summary requests to local Ollama vs Anthropic Claude 3.7.',
    workspaceId: 'Autonomous Agents',
    projectId: 'BYOK Treasury Optimizer',
    priority: 'low',
    status: 'backlog',
    suggestedShipId: 'ship-research',
    requiredArtifacts: ['Model Routing Cost Benchmark'],
    budgetLimitUSD: 1.20,
    estimatedCostUSD: 0.40,
    mapSteps: [
      { stepNumber: 1, title: 'Profile token volume on recurring SOPs', status: 'pending' },
      { stepNumber: 2, title: 'Run comparative accuracy benchmark', status: 'pending' }
    ],
    createdAt: '2026-09-28T07:00:00Z',
    updatedAt: '2026-09-28T07:00:00Z',
    discoveriesCount: 0
  }
];

const initialArtifacts: Artifact[] = [
  {
    id: 'art-repo-health',
    questId: 'quest-repo-health',
    shipId: 'ship-dev',
    producerCrewId: 'crew-repo-analyst',
    title: 'Repository Health Brief: diezy-labs/claw-crew',
    type: 'health-brief',
    summary: 'Branch feat/enhance-agent-phase exhibits clean separation between Go cognitive engine and React/Tauri web UI. Zero circular dependencies detected.',
    content: `# Repository Health Brief

## Executive Summary
Assessment conducted on **diezy-labs/claw-crew** across branch \`feat/enhance-agent-phase\`.

### Key Findings:
- **Clean Architecture:** \`engine/src/\` respects clean boundaries. Domain logic (fleet, ship, quest, map, artifact) does not leak transport concerns.
- **Security Sandboxing:** Tauri host integration correctly wraps local file reads with cryptographic tool receipts.
- **Frontend Architecture:** Web application uses modular React + Tailwind CSS with high-performance Zustand state synchronization.

## Recommendations
1. Establish ActionDigest verification prior to executing any write-level tool.
2. Standardize all telemetry to use tabular-nums formatting.`,
    discoveries: [
      { id: 'disc-1', type: 'opportunity', title: 'Clean Architecture intact', detail: 'Bounded contexts in engine/src/ can easily host the new Fleet and Ship aggregates without schema rewrites.', evidenceSource: 'engine/src/orchestrator/' },
      { id: 'disc-2', type: 'risk', title: 'CI timeout in teardown phase', detail: 'WebSocket teardown occasionally hangs for 12 seconds in headless test harness.', evidenceSource: '.github/workflows/ci.yml' }
    ],
    evidenceCount: 14,
    voyageCostUSD: 0.38,
    status: 'treasure',
    createdAt: '2026-09-28T08:30:00Z'
  },
  {
    id: 'art-ci-triage',
    questId: 'quest-ci-triage',
    shipId: 'ship-dev',
    producerCrewId: 'crew-qa-reviewer',
    title: 'CI Triage: Integration Suite Teardown Hang',
    type: 'ci-triage',
    summary: 'Root cause identified: goroutine leak in connection listener when client disconnects without sending EOF frame.',
    content: `# CI Triage Report #104

## Symptom
Integration tests fail intermittently with \`context deadline exceeded (10m)\` during container cleanup.

## Investigation Path
- Examined Go runtime stack dump from GitHub Actions runner.
- Goroutine 482 was parked in \`net.(*netFD).Read\` waiting for socket close.
- Proposed patch: Add explicit \`SetReadDeadline\` before closing listener socket.`,
    discoveries: [
      { id: 'disc-3', type: 'risk', title: 'Release Blocker Detected', detail: 'Blocks automated merge validation on release PRs until deadline timeout patch lands.', evidenceSource: 'pkg/transport/ws_listener.go:142' }
    ],
    evidenceCount: 8,
    voyageCostUSD: 0.42,
    status: 'needs_review',
    createdAt: '2026-09-28T11:15:00Z'
  },
  {
    id: 'art-release-v14',
    questId: 'quest-release-v14',
    shipId: 'ship-dev',
    producerCrewId: 'crew-eng-planner',
    title: 'Release Readiness Checklist v1.4',
    type: 'readiness-checklist',
    summary: 'Preliminary readiness verification for v1.4 release package. 13 of 14 gates passed; 1 pending CI fix.',
    content: `# Release Readiness Checklist v1.4

## Quality Gates
- [x] TypeScript strict type checks pass without emit
- [x] Unit test suite passes with 100% component coverage
- [x] Dark/light theme WCAG AA contrast verified
- [x] Zustand state synchronization across 7 views validated
- [ ] Integration test runner timeout fix verified (In progress)

## Go / No-Go Status
**CONDITIONAL GO** — Pending verified merge of CI listener patch.`,
    discoveries: [
      { id: 'disc-4', type: 'recommendation', title: 'Deploy with gated write approval', detail: 'Keep Captain’s Approval strictly active for all GitHub Actions writes during launch.', evidenceSource: 'Fleet Code Policy v1.2' }
    ],
    evidenceCount: 19,
    voyageCostUSD: 0.62,
    status: 'needs_review',
    createdAt: '2026-09-28T10:45:00Z'
  }
];

const initialApprovals: CaptainApproval[] = [
  {
    id: 'appr-github-issue',
    questId: 'quest-github-issue-draft',
    shipId: 'ship-dev',
    crewId: 'crew-qa-reviewer',
    title: 'Create Draft GitHub Issue for CI Teardown Timeout',
    actionType: 'github_issue_create',
    targetResource: 'github.com/diezy-labs/claw-crew/issues',
    draftSummary: 'Title: [Bug] WebSocket connection listener goroutine leak causes CI runner timeout\nLabels: bug, ci/cd, high-priority\nAssignee: Developer Delivery Ship',
    justification: 'Repeated across 3 consecutive CI runs. Documenting the stack trace and fix prevents other contributors from chasing phantom failures.',
    effect: 'Creates a public/repo issue draft. Does NOT merge code, deploy software, or modify branch protection rules.',
    costUSD: 0.00,
    status: 'pending',
    createdAt: '2026-09-28T11:20:00Z'
  },
  {
    id: 'appr-publish-copy',
    questId: 'quest-release-v14',
    shipId: 'ship-market',
    crewId: 'crew-brand-reviewer',
    title: 'Approve Release Announcement Editorial Copy',
    actionType: 'publish_content',
    targetResource: 'docs/releases/v1.4.md',
    draftSummary: 'Headline: "Your Fleet. Your Rules." — Galleon v1.4 Delivers Persistent Multi-Ship Collaboration and BYOK Cost Control.',
    justification: 'Coordinates marketing communications with the scheduled release milestone.',
    effect: 'Stages release markdown file for inclusion in official documentation.',
    costUSD: 0.00,
    status: 'pending',
    createdAt: '2026-09-28T09:40:00Z'
  }
];

const initialLogbook: LogbookEntry[] = [
  {
    id: 'log-1',
    timestamp: '11:20:14',
    actorType: 'crew',
    actorName: 'QA & Risk Reviewer',
    action: 'Requested Captain’s Approval: Create GitHub issue for CI timeout',
    entityType: 'approval',
    entityId: 'appr-github-issue',
    correlationId: 'voyage-ci-884',
    severity: 'warning'
  },
  {
    id: 'log-2',
    timestamp: '11:15:02',
    actorType: 'crew',
    actorName: 'QA & Risk Reviewer',
    action: 'Published Artifact: CI Triage: Integration Suite Teardown Hang',
    entityType: 'artifact',
    entityId: 'art-ci-triage',
    correlationId: 'voyage-ci-884',
    severity: 'info'
  },
  {
    id: 'log-3',
    timestamp: '10:45:22',
    actorType: 'navigator',
    actorName: 'Horizon (Orchestrator)',
    action: 'Advanced Map Step 2 of Prepare Release v1.4 Package',
    entityType: 'quest',
    entityId: 'quest-release-v14',
    correlationId: 'voyage-rel-012',
    severity: 'info'
  },
  {
    id: 'log-4',
    timestamp: '08:30:10',
    actorType: 'owner',
    actorName: 'Pirate King (You)',
    action: 'Validated Artifact and claimed Treasure: Repository Health Brief',
    entityType: 'artifact',
    entityId: 'art-repo-health',
    correlationId: 'voyage-audit-001',
    severity: 'success'
  },
  {
    id: 'log-5',
    timestamp: '08:00:00',
    actorType: 'quartermaster',
    actorName: 'Quartermaster Executive',
    action: 'Compiled Morning Executive Briefing: 2 active Ships, 1 release blocker',
    entityType: 'ship',
    entityId: 'fleet-diezy',
    correlationId: 'qm-briefing-daily',
    severity: 'info'
  }
];

const initialTreasuryLedger: TreasuryLedger[] = [
  { id: 't-1', date: '2026-09-28', shipId: 'ship-dev', questTitle: 'Prepare Release v1.4 Package', provider: 'Anthropic', model: 'claude-3-7-sonnet', tokensUsed: 42180, costUSD: 0.62 },
  { id: 't-2', date: '2026-09-28', shipId: 'ship-dev', questTitle: 'Triage Integration Test Flakiness', provider: 'Anthropic', model: 'claude-3-5-haiku', tokensUsed: 88400, costUSD: 0.42 },
  { id: 't-3', date: '2026-09-28', shipId: 'ship-dev', questTitle: 'Repository Health Audit', provider: 'Google Gemini', model: 'gemini-2.5-pro', tokensUsed: 124000, costUSD: 0.38 },
  { id: 't-4', date: '2026-09-27', shipId: 'ship-market', questTitle: 'Audience Signal Mapping', provider: 'Google Gemini', model: 'gemini-2.5-flash', tokensUsed: 210000, costUSD: 0.52 },
  { id: 't-5', date: '2026-09-27', shipId: 'ship-research', questTitle: 'Local Ollama vs Cloud Benchmark', provider: 'Ollama (Local)', model: 'deepseek-r1:14b', tokensUsed: 340000, costUSD: 0.00 }
];

const initialNotifications: NotificationItem[] = [
  {
    id: 'notif-1',
    title: 'Captain’s Approval Required',
    description: 'Developer Delivery Ship requested authorization to create a draft GitHub issue for CI timeout.',
    type: 'approval',
    read: false,
    createdAt: '11:20 AM',
    actionLinkTab: 'approvals'
  },
  {
    id: 'notif-2',
    title: 'Voyage Underway: CI Triage',
    description: 'QA & Risk Reviewer reached 68% progress on isolating the socket teardown bug.',
    type: 'quest',
    read: false,
    createdAt: '11:15 AM',
    actionLinkTab: 'mission-board'
  },
  {
    id: 'notif-3',
    title: 'Treasure Claimed',
    description: 'Repository Health Brief was promoted to validated Treasure by Owner.',
    type: 'artifact',
    read: true,
    createdAt: '08:30 AM',
    actionLinkTab: 'artifacts'
  },
  {
    id: 'notif-4',
    title: 'Treasury Budget Health',
    description: 'Weekly BYOK spend is $4.10 / $25.00 (16.4%). Projected to stay safely within monthly cap.',
    type: 'treasury',
    read: true,
    createdAt: '08:00 AM',
    actionLinkTab: 'treasury'
  }
];

const initialJournalSessions: JournalSession[] = [
  {
    id: 'session-1',
    title: 'Product Direction & Multi-Ship Capacity',
    updatedAt: 'Today, 10:45 AM',
    workspaceId: 'Diezy Labs',
    lastNote: 'Evaluate whether Fleet capacity should be one Ship or five on Community vs Pro...',
    isPinned: true,
    isArchived: false,
    isTemporary: false,
    savedArtifactCount: 1,
    questDraftCount: 1,
    messages: [
      { id: 'jm-1', sender: 'owner', content: 'Compare these two model providers for CI triage and repository health.', timestamp: '10:40 AM' },
      { id: 'jm-2', sender: 'quartermaster', content: 'For repository AST parsing, local Ollama deepseek-r1 provides zero token cost with high syntax fidelity. For CI triage and complex race condition debugging, Claude 3.7 Sonnet reasoning remains superior.', timestamp: '10:42 AM' },
      { id: 'jm-3', sender: 'owner', content: 'Agreed. Let’s keep local Ollama for the Repository Analyst, and Claude for QA Reviewer.', timestamp: '10:45 AM' }
    ]
  },
  {
    id: 'session-2',
    title: 'CI Socket Teardown Race Condition Notes',
    updatedAt: 'Yesterday',
    workspaceId: 'Product Platform',
    lastNote: 'Investigate SetReadDeadline before socket close in ws_listener.go',
    isPinned: false,
    isArchived: false,
    isTemporary: false,
    savedArtifactCount: 1,
    questDraftCount: 1,
    messages: [
      { id: 'jm-4', sender: 'owner', content: 'Why does the integration test runner hang during container cleanup?', timestamp: 'Yesterday' },
      { id: 'jm-5', sender: 'quartermaster', content: 'Goroutine dump shows socket listener blocked on read because client disconnects without sending EOF. Proposing SetReadDeadline timeout patch.', timestamp: 'Yesterday' }
    ]
  },
  {
    id: 'session-3',
    title: 'Architecture Ideas & Sandboxing Bounds',
    updatedAt: 'This week',
    workspaceId: 'Core Infrastructure',
    lastNote: 'Verify Landlock syscall boundaries on macOS/Linux...',
    isPinned: false,
    isArchived: false,
    isTemporary: false,
    savedArtifactCount: 0,
    questDraftCount: 0,
    messages: [
      { id: 'jm-6', sender: 'owner', content: 'Are our cryptographic tool receipts validated before write tools execute?', timestamp: '2 days ago' },
      { id: 'jm-7', sender: 'quartermaster', content: 'Yes, ActionDigest generates deterministic SHA-256 verification of tool name, resource path, and credential scope before any write tool is dispatched.', timestamp: '2 days ago' }
    ]
  }
];

const initialChatMessages: ChatMessage[] = [
  {
    id: 'msg-1',
    sender: 'quartermaster',
    content: `Good morning, Pirate King. 

All 3 Ships are operational. Horizon on the Developer Ship flagged a release blocker: a recurring CI timeout during integration test socket teardown.

I recommend reviewing the pending Captain’s Approval to file the issue, or allowing the Developer Ship to formulate the fix patch.

What should your Fleet accomplish next?`,
    timestamp: '11:22 AM',
    suggestedActions: [
      { label: 'Review Release Blocker Approval', actionType: 'open_tab', payload: 'approvals' },
      { label: 'Inspect v1.4 Mission Board', actionType: 'open_tab', payload: 'mission-board' },
      { label: 'Run Repository Health Quest', actionType: 'create_quest', payload: { title: 'Run Immediate Health Quest' } }
    ]
  }
];

const defaultSettings: FleetSettings = {
  profile: {
    displayName: 'Adiet Alimudin',
    narrativeTitle: 'Pirate King',
    preferredAddress: 'Pirate King',
    communicationStyle: 'Clear and concise',
    defaultLanguage: 'Bahasa Indonesia',
    workingHours: '09:00–18:00 Asia/Jakarta',
    weeklyBriefing: 'Monday 09:00'
  },
  appearance: {
    theme: 'dark',
    density: 'comfortable',
    terminology: 'adventure',
    showFunctionalSubtitles: true,
    showAdvancedTechnicalNames: false,
    displayLanguage: 'English / Bahasa Indonesia',
    outputLanguage: 'Bahasa Indonesia'
  },
  notifications: {
    captainApprovalRequested: true,
    strategicDecisionRequested: true,
    questBlocked: true,
    questCompleted: true,
    everyVoyageProgress: false,
    scheduledQuestFailed: true,
    budgetSoftLimitReached: true,
    providerAttention: true,
    shipHealthWarning: true,
    desktopNotifications: true,
    soundEnabled: false,
    quietHoursMode: 'working_hours'
  },
  journal: {
    defaultChatTarget: 'Quartermaster',
    defaultPrivacy: 'Private to Pirate King',
    autoNameSessions: true,
    learningMode: 'ask',
    retentionActive: 'Keep until I delete them',
    retentionTemporary: 'Delete when closed',
    autoArchiveDays: 30,
    includeArchivedInSearch: true
  },
  defaults: {
    defaultFleet: 'Diezy Labs Fleet',
    defaultWorkspace: 'Diezy Labs',
    defaultProject: 'Product Platform',
    defaultPriority: 'medium',
    defaultPlanningMode: 'guided',
    defaultRouting: 'Quartermaster recommends',
    defaultModelProfile: 'Balanced Engineering Model',
    defaultLearningMode: 'Propose and ask'
  },
  privacy: {
    dataDirectory: '~/Library/Application Support/FleetAI',
    shareAnonymousDiagnostics: false,
    shareCrashReports: false,
    artifactRetention: 'Keep until deleted',
    logbookRetention: 'Keep locally for 90 days',
    runTraceRetention: 'Keep locally for 30 days',
    redactKnownSecrets: true,
    confirmExportingSensitive: true
  },
  runtime: {
    launchAtSignIn: true,
    startRuntimeWithApp: true,
    keepRuntimeActiveOnClose: true,
    closeAction: 'minimize_tray',
    openAtStartup: 'Quarterdeck',
    localAddress: '127.0.0.1:8080'
  },
  updates: {
    channel: 'stable',
    autoCheck: true,
    autoDownload: true,
    lastChecked: 'Today, 01:42'
  },
  accessibility: {
    textSize: '100%',
    increaseContrast: false,
    reduceMotion: false,
    alwaysShowFocus: true,
    useKeyboardShortcuts: true,
    confirmDestructiveActions: true,
    explainTechnicalErrors: true
  },
  developer: {
    developerMode: false,
    localApiEnabled: true,
    includeDebugMetadata: false,
    showInternalEntityIds: false,
    verboseLocalLogs: false
  },
  experimental: {
    multiShipPreview: false,
    tempAgentPromotion: false,
    advancedMapStudio: false,
    localModelAutoRouting: false,
    crossShipQuestPreview: false,
    newQuartermasterBriefing: false
  }
};

export const useFleetStore = create<FleetState>((set, get) => ({
  theme: 'dark',
  activeTab: 'quarterdeck',
  isCommandPaletteOpen: false,
  realmName: "Adiet’s Realm",
  fleetName: 'Diezy Labs Fleet',
  selectedWorkspace: 'Product Platform',
  selectedProject: 'v1.4 Release Readiness',
  workspaces: ['Product Platform', 'Autonomous Agents', 'Core Infrastructure'],
  projects: ['v1.4 Release Readiness', 'Claw Crew Agent Phase 2', 'BYOK Treasury Optimizer'],
  ships: initialShips,
  crew: initialCrew,
  quests: initialQuests,
  artifacts: initialArtifacts,
  approvals: initialApprovals,
  logbook: initialLogbook,
  treasuryLedger: initialTreasuryLedger,
  notifications: initialNotifications,
  chatMessages: initialChatMessages,
  journalSessions: initialJournalSessions,
  selectedJournalSessionId: 'session-1',
  selectedQuestId: null,
  selectedArtifactId: null,
  activeSettingsCategory: 'profile',
  settings: defaultSettings,
  isSidebarCollapsed: false,
  isMobileSidebarOpen: false,
  isAnchorDropped: false,
  isFleetPulseOpen: false,

  setActiveSettingsCategory: (cat) => set({ activeSettingsCategory: cat }),
  toggleSidebarCollapsed: () => set((state) => ({ isSidebarCollapsed: !state.isSidebarCollapsed })),
  setSidebarCollapsed: (collapsed) => set({ isSidebarCollapsed: collapsed }),
  setMobileSidebarOpen: (open) => set({ isMobileSidebarOpen: open }),
  setFleetPulseOpen: (open) => set({ isFleetPulseOpen: open }),
  toggleFleetPulse: () => set((state) => ({ isFleetPulseOpen: !state.isFleetPulseOpen })),
  toggleAnchor: () => {
    const isDropped = !get().isAnchorDropped;
    set((state) => ({
      isAnchorDropped: isDropped,
      notifications: [
        {
          id: 'notif-' + Date.now(),
          title: isDropped ? 'Anchor Dropped: All Autonomous Voyages Paused' : 'Anchor Weighed: Voyages Resumed',
          description: isDropped
            ? 'Emergency pause activated. All autonomous background agent loops are safely halted.'
            : 'Fleet operations resumed. Autonomous agent voyages and SOP executions are active.',
          type: 'health',
          read: false,
          createdAt: 'Just now',
          actionLinkTab: 'crows-nest'
        },
        ...state.notifications
      ],
      logbook: [
        {
          id: 'log-' + Date.now(),
          timestamp: new Date().toLocaleTimeString(),
          actorType: 'owner',
          actorName: 'Pirate King (You)',
          action: isDropped ? 'EMERGENCY HALT: Dropped Anchor (Paused all agent voyages)' : 'RESUME: Weighed Anchor (Resumed agent voyages)',
          entityType: 'ship',
          entityId: 'fleet-all',
          correlationId: 'anchor-' + Date.now(),
          severity: isDropped ? 'alert' : 'info'
        },
        ...state.logbook
      ]
    }));
  },

  updateSettings: (updater, logAudit) => {
    set((state) => {
      const nextSettings = updater(state.settings);
      const nextLogbook = logAudit
        ? [
            {
              id: 'log-' + Date.now(),
              timestamp: new Date().toLocaleTimeString(),
              actorType: 'owner' as const,
              actorName: 'Pirate King (You)',
              action: `Settings updated: ${logAudit}`,
              entityType: 'policy' as const,
              entityId: 'settings-pref',
              correlationId: 'set-' + Date.now(),
              severity: 'info' as const
            },
            ...state.logbook
          ]
        : state.logbook;

      return {
        settings: nextSettings,
        logbook: nextLogbook
      };
    });
  },

  resetSettingsCategory: (category) => {
    set((state) => {
      if (category in defaultSettings) {
        const catKey = category as keyof FleetSettings;
        return {
          settings: {
            ...state.settings,
            [catKey]: defaultSettings[catKey]
          }
        };
      }
      return state;
    });
  },

  setTheme: (theme) => {
    document.documentElement.classList.toggle('dark', theme === 'dark');
    set({ theme });
  },

  toggleTheme: () => {
    const next = get().theme === 'dark' ? 'light' : 'dark';
    document.documentElement.classList.toggle('dark', next === 'dark');
    set({ theme: next });
  },

  setActiveTab: (tab) => set({ activeTab: tab }),

  setCommandPaletteOpen: (open) => set({ isCommandPaletteOpen: open }),

  setSelectedWorkspace: (ws) => set({ selectedWorkspace: ws }),

  setSelectedProject: (proj) => set({ selectedProject: proj }),

  setSelectedQuestId: (id) => set({ selectedQuestId: id }),

  setSelectedArtifactId: (id) => set({ selectedArtifactId: id }),

  setSelectedJournalSessionId: (id) => set({ selectedJournalSessionId: id }),

  sendQuartermasterMessage: (content) => {
    const userMsgId = 'usr-' + Date.now();
    const nowStr = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    
    const userMsg: ChatMessage = {
      id: userMsgId,
      sender: 'owner',
      content,
      timestamp: nowStr
    };

    set((state) => ({
      chatMessages: [...state.chatMessages, userMsg]
    }));

    // Quartermaster intelligent response simulation
    setTimeout(() => {
      const qmMsgId = 'qm-' + Date.now();
      const lower = content.toLowerCase();
      let reply = '';
      let actions = [];
      let generatedArtifact: Partial<Artifact> | undefined;

      if (lower.includes('release') || lower.includes('v1.4')) {
        reply = `Understood, Pirate King. I have inspected the Developer Delivery Ship’s logs. We have 3 artifacts ready, and 1 Captain's Approval pending for the CI teardown issue. I can set sail on the final readiness checklist immediately.`;
        actions = [
          { label: 'Set Sail on Release Checklist', actionType: 'create_quest' as const },
          { label: 'Review Captain’s Approval', actionType: 'open_tab' as const, payload: 'approvals' }
        ];
      } else if (lower.includes('ci') || lower.includes('bug') || lower.includes('issue')) {
        reply = `QA & Risk Reviewer has already traced the WebSocket goroutine hang. The fix involves passing an explicit timeout before closing the socket. I have prepared a draft briefing you can promote to an Artifact or approve for submission to GitHub.`;
        generatedArtifact = {
          title: 'Immediate CI Remediation Strategy',
          type: 'ci-triage',
          summary: 'Apply SetReadDeadline in pkg/transport/ws_listener.go to terminate pending read loops cleanly on client abort.'
        };
        actions = [
          { label: 'Save as Artifact', actionType: 'save_artifact' as const, payload: generatedArtifact },
          { label: 'Open Captain’s Approval', actionType: 'open_tab' as const, payload: 'approvals' }
        ];
      } else if (lower.includes('squad') || lower.includes('crew') || lower.includes('team')) {
        reply = `Your Fleet currently has 3 Ships with 6 active Crew Specialists. The Developer Delivery Ship has 3 of 5 berths filled. Would you like to use "Make Me a Squad" to recruit additional specialists?`;
        actions = [
          { label: 'Open Crew Management', actionType: 'open_tab' as const, payload: 'crew' },
          { label: 'Inspect Ships', actionType: 'open_tab' as const, payload: 'ships' }
        ];
      } else {
        reply = `Received. I have recorded your guidance in Fleet operational memory. I will coordinate with Horizon and Beacon to route tasks accordingly. You can inspect the live status on the Mission Board at any moment.`;
        actions = [
          { label: 'Open Mission Board', actionType: 'open_tab' as const, payload: 'mission-board' },
          { label: 'View Recent Artifacts', actionType: 'open_tab' as const, payload: 'artifacts' }
        ];
      }

      const qmMsg: ChatMessage = {
        id: qmMsgId,
        sender: 'quartermaster',
        content: reply,
        timestamp: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
        suggestedActions: actions,
        generatedArtifactPreview: generatedArtifact
      };

      set((state) => ({
        chatMessages: [...state.chatMessages, qmMsg]
      }));
    }, 600);
  },

  createQuest: (questData) => {
    const id = 'quest-' + Date.now();
    const newQuest: Quest = {
      id,
      title: questData.title || 'New Fleet Quest',
      objective: questData.objective || 'Accomplish assigned operational outcome.',
      workspaceId: questData.workspaceId || get().selectedWorkspace,
      projectId: questData.projectId || get().selectedProject,
      priority: questData.priority || 'medium',
      status: 'ready',
      suggestedShipId: questData.suggestedShipId || 'ship-dev',
      assignedShipId: questData.assignedShipId || 'ship-dev',
      requiredArtifacts: questData.requiredArtifacts || ['Operational Deliverable Brief'],
      budgetLimitUSD: questData.budgetLimitUSD || 2.00,
      estimatedCostUSD: questData.estimatedCostUSD || 0.45,
      mapSteps: [
        { stepNumber: 1, title: 'Analyze objective and codebase context', status: 'pending' },
        { stepNumber: 2, title: 'Execute specialist tasks with scoped tools', status: 'pending' },
        { stepNumber: 3, title: 'Synthesize verified Artifact and Discoveries', status: 'pending' }
      ],
      activeVoyageProgress: 0,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      discoveriesCount: 0
    };

    set((state) => ({
      quests: [newQuest, ...state.quests],
      notifications: [
        {
          id: 'notif-' + Date.now(),
          title: 'Quest Created',
          description: `"${newQuest.title}" was routed to ${newQuest.suggestedShipId === 'ship-dev' ? 'Developer Delivery Ship' : 'Specialist Ship'}.`,
          type: 'quest',
          read: false,
          createdAt: 'Just now',
          actionLinkTab: 'mission-board'
        },
        ...state.notifications
      ],
      logbook: [
        {
          id: 'log-' + Date.now(),
          timestamp: new Date().toLocaleTimeString(),
          actorType: 'quartermaster',
          actorName: 'Quartermaster Executive',
          action: `Created and routed Quest: "${newQuest.title}"`,
          entityType: 'quest',
          entityId: id,
          correlationId: 'voyage-' + Math.random().toString(36).substring(7),
          severity: 'info'
        },
        ...state.logbook
      ]
    }));
  },

  updateQuestStatus: (id, status) => {
    set((state) => ({
      quests: state.quests.map((q) => (q.id === id ? { ...q, status, updatedAt: new Date().toISOString() } : q))
    }));
  },

  runQuestVoyage: (id) => {
    set((state) => ({
      quests: state.quests.map((q) =>
        q.id === id ? { ...q, status: 'underway', activeVoyageProgress: 15, updatedAt: new Date().toISOString() } : q
      ),
      notifications: [
        {
          id: 'notif-' + Date.now(),
          title: 'Voyage Set Sail',
          description: 'Specialist Crew began execution of assigned Map steps.',
          type: 'quest',
          read: false,
          createdAt: 'Just now',
          actionLinkTab: 'mission-board'
        },
        ...state.notifications
      ]
    }));
  },

  handleApproval: (id, decision) => {
    const appr = get().approvals.find((a) => a.id === id);
    if (!appr) return;

    set((state) => ({
      approvals: state.approvals.map((a) => (a.id === id ? { ...a, status: decision } : a)),
      notifications: [
        {
          id: 'notif-' + Date.now(),
          title: decision === 'approved' ? 'Action Approved by Captain' : 'Action Rejected by Captain',
          description: `"${appr.title}" has been ${decision}. Logbook updated with signature.`,
          type: 'approval',
          read: false,
          createdAt: 'Just now',
          actionLinkTab: 'approvals'
        },
        ...state.notifications
      ],
      logbook: [
        {
          id: 'log-' + Date.now(),
          timestamp: new Date().toLocaleTimeString(),
          actorType: 'owner',
          actorName: 'Pirate King (You)',
          action: `${decision.toUpperCase()}: ${appr.title} (${appr.targetResource})`,
          entityType: 'approval',
          entityId: id,
          correlationId: 'appr-' + id,
          severity: decision === 'approved' ? 'success' : 'warning'
        },
        ...state.logbook
      ]
    }));
  },

  promoteArtifactToTreasure: (id) => {
    const art = get().artifacts.find((a) => a.id === id);
    if (!art) return;

    set((state) => ({
      artifacts: state.artifacts.map((a) => (a.id === id ? { ...a, status: 'treasure' } : a)),
      notifications: [
        {
          id: 'notif-' + Date.now(),
          title: 'Treasure Claimed!',
          description: `"${art.title}" was verified and claimed as high-value organizational Treasure.`,
          type: 'artifact',
          read: false,
          createdAt: 'Just now',
          actionLinkTab: 'artifacts'
        },
        ...state.notifications
      ],
      logbook: [
        {
          id: 'log-' + Date.now(),
          timestamp: new Date().toLocaleTimeString(),
          actorType: 'owner',
          actorName: 'Pirate King (You)',
          action: `Promoted Artifact to Treasure: ${art.title}`,
          entityType: 'artifact',
          entityId: id,
          correlationId: 'treasure-' + id,
          severity: 'success'
        },
        ...state.logbook
      ]
    }));
  },

  saveArtifact: (artifactData) => {
    const id = 'art-' + Date.now();
    const newArtifact: Artifact = {
      ...artifactData,
      id,
      createdAt: new Date().toISOString()
    };

    set((state) => ({
      artifacts: [newArtifact, ...state.artifacts],
      notifications: [
        {
          id: 'notif-' + Date.now(),
          title: 'Artifact Saved',
          description: `"${newArtifact.title}" is now available in the Artifacts Gallery.`,
          type: 'artifact',
          read: false,
          createdAt: 'Just now',
          actionLinkTab: 'artifacts'
        },
        ...state.notifications
      ]
    }));
  },

  addCrewMember: (crewData) => {
    const id = 'crew-' + Date.now();
    const newMember: CrewMember = {
      ...crewData,
      id
    };

    set((state) => ({
      crew: [...state.crew, newMember],
      ships: state.ships.map((s) => (s.id === newMember.shipId ? { ...s, crewIds: [...s.crewIds, id] } : s)),
      notifications: [
        {
          id: 'notif-' + Date.now(),
          title: 'Crew Berth Assigned',
          description: `${newMember.name} joined ${state.ships.find((s) => s.id === newMember.shipId)?.name || 'the Fleet'}.`,
          type: 'quest',
          read: false,
          createdAt: 'Just now',
          actionLinkTab: 'crew'
        },
        ...state.notifications
      ]
    }));
  },

  markNotificationRead: (id) => {
    set((state) => ({
      notifications: state.notifications.map((n) => (n.id === id ? { ...n, read: true } : n))
    }));
  },

  markAllNotificationsRead: () => {
    set((state) => ({
      notifications: state.notifications.map((n) => ({ ...n, read: true }))
    }));
  },

  addNotification: (notifData) => {
    const newNotif: NotificationItem = {
      ...notifData,
      id: 'notif-' + Date.now(),
      createdAt: 'Just now',
      read: false
    };

    set((state) => ({
      notifications: [newNotif, ...state.notifications]
    }));
  },

  simulateVoyageTick: () => {
    if (get().isAnchorDropped) {
      return; // All autonomous runs halted safely
    }

    set((state) => {
      let updated = false;
      const quests = state.quests.map((q) => {
        if (q.status === 'underway' && q.activeVoyageProgress !== undefined) {
          const nextProg = q.activeVoyageProgress + 12;
          if (nextProg >= 100) {
            updated = true;
            return {
              ...q,
              status: 'review' as QuestStatus,
              activeVoyageProgress: 100,
              updatedAt: new Date().toISOString()
            };
          }
          return { ...q, activeVoyageProgress: nextProg, updatedAt: new Date().toISOString() };
        }
        return q;
      });

      if (!updated) return { quests };

      return {
        quests,
        notifications: [
          {
            id: 'notif-' + Date.now(),
            title: 'Voyage Completed Map Steps',
            description: 'A Voyage reached 100% and produced a reviewable Artifact.',
            type: 'artifact',
            read: false,
            createdAt: 'Just now',
            actionLinkTab: 'artifacts'
          },
          ...state.notifications
        ]
      };
    });
  },

  createJournalSession: (title, workspaceId) => {
    const id = 'session-' + Date.now();
    const newSession: JournalSession = {
      id,
      title: title || 'New Captain’s Journal Entry',
      updatedAt: 'Just now',
      workspaceId: workspaceId || get().selectedWorkspace,
      lastNote: 'Session opened with Quartermaster...',
      isPinned: false,
      isArchived: false,
      isTemporary: false,
      savedArtifactCount: 0,
      questDraftCount: 0,
      messages: [
        {
          id: 'msg-' + Date.now(),
          sender: 'quartermaster',
          content: `Welcome to this private Captain’s Journal entry. This is your personal workspace for exploratory thought, drafts, and ongoing thinking with me. It is not recorded in the official fleet Logbook. What are you considering?`,
          timestamp: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
        }
      ]
    };

    set((state) => ({
      journalSessions: [newSession, ...state.journalSessions],
      selectedJournalSessionId: id
    }));
  },

  sendJournalMessage: (sessionId, content) => {
    const userMsgId = 'jmsg-' + Date.now();
    const nowStr = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

    const userMsg: ChatMessage = {
      id: userMsgId,
      sender: 'owner',
      content,
      timestamp: nowStr
    };

    set((state) => ({
      journalSessions: state.journalSessions.map((s) =>
        s.id === sessionId
          ? {
              ...s,
              lastNote: content.slice(0, 80) + '...',
              updatedAt: 'Just now',
              messages: [...s.messages, userMsg]
            }
          : s
      )
    }));

    // Quartermaster simulated response inside journal
    setTimeout(() => {
      const qmMsg: ChatMessage = {
        id: 'jqm-' + Date.now(),
        sender: 'quartermaster',
        content: `I have noted this in your private journal. We can keep this exploratory, promote this concept into a reviewable Artifact, or draft a Quest to assign to one of your Ships.`,
        timestamp: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
      };

      set((state) => ({
        journalSessions: state.journalSessions.map((s) =>
          s.id === sessionId ? { ...s, messages: [...s.messages, qmMsg] } : s
        )
      }));
    }, 600);
  },

  togglePinJournalSession: (id) => {
    set((state) => ({
      journalSessions: state.journalSessions.map((s) =>
        s.id === id ? { ...s, isPinned: !s.isPinned } : s
      )
    }));
  },

  archiveJournalSession: (id) => {
    set((state) => ({
      journalSessions: state.journalSessions.map((s) =>
        s.id === id ? { ...s, isArchived: !s.isArchived } : s
      )
    }));
  },

  convertJournalToArtifact: (sessionId) => {
    const session = get().journalSessions.find((s) => s.id === sessionId);
    if (!session) return;

    const summaryContent = session.messages.map((m) => `${m.sender.toUpperCase()}: ${m.content}`).join('\n\n');
    get().saveArtifact({
      questId: 'quest-journal-promote',
      shipId: 'ship-dev',
      producerCrewId: 'crew-repo-analyst',
      title: `Artifact: ${session.title}`,
      type: 'decision-brief',
      summary: `Synthesized from private Captain’s Journal discussion on ${session.title}.`,
      content: `# ${session.title}\n\n## Journal Synthesis\n${summaryContent}`,
      discoveries: [
        { id: 'disc-j-' + Date.now(), type: 'opportunity', title: 'Exploratory Concept Promoted', detail: session.lastNote, evidenceSource: 'Captain’s Journal' }
      ],
      evidenceCount: 1,
      voyageCostUSD: 0.05,
      status: 'needs_review'
    });

    set((state) => ({
      journalSessions: state.journalSessions.map((s) =>
        s.id === sessionId ? { ...s, savedArtifactCount: s.savedArtifactCount + 1 } : s
      ),
      activeTab: 'artifacts'
    }));
  },

  convertJournalToQuest: (sessionId) => {
    const session = get().journalSessions.find((s) => s.id === sessionId);
    if (!session) return;

    get().createQuest({
      title: session.title,
      objective: session.lastNote || 'Execute work formulated during Captain’s Journal consultation.',
      workspaceId: session.workspaceId,
      suggestedShipId: 'ship-dev'
    });

    set((state) => ({
      journalSessions: state.journalSessions.map((s) =>
        s.id === sessionId ? { ...s, questDraftCount: s.questDraftCount + 1 } : s
      ),
      activeTab: 'quests'
    }));
  }
}));
