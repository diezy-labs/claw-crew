import {
  Ship,
  CrewMember,
  Squad,
  Quest,
  Artifact,
  CaptainApproval,
  LogbookEntry,
  TreasuryLedger,
  NotificationItem,
  TrainingSkill,
  GlobalSteering,
  SteeringDirective,
  TrainingHook,
  JournalSession,
  ChatMessage
} from '../types';

export const initialShips: Ship[] = [
  {
    id: 'ship-dev',
    name: 'Developer Delivery Ship',
    fleetId: 'fleet-diezy',
    tagline: 'Autonomous coding, refactoring, test repair, and CI verification.',
    homeScope: 'repository',
    navigatorName: 'Horizon (Lead)',
    status: 'active',
    activeVoyagesCount: 1,
    monthlySpentUSD: 2.45,
    squadIds: ['squad-dev'],
    crewIds: ['crew-repo-analyst', 'crew-eng-planner'],
    charter: {
      purpose: 'Execute precise software changes, maintain architecture patterns, and repair test regressions.',
      acceptedQuestTypes: ['bug_fix', 'feature_development', 'test_generation', 'code_refactor'],
      crewAuthority: 'Create and run branch-scoped tests, edit local files. Direct git push requires Captain’s Approval.',
      prohibitedActions: ['Direct master branch push', 'Production DB migration', 'Deleting secret keys'],
      budgetPerVoyageUSD: 2.50,
      monthlyBudgetUSD: 30.00,
      memorySharing: 'ship_scoped'
    }
  },
  {
    id: 'ship-growth',
    name: 'Growth & Brand Ship',
    fleetId: 'fleet-diezy',
    tagline: 'Market analysis, documentation, release notes, and community signals.',
    homeScope: 'content',
    navigatorName: 'Beacon (Lead)',
    status: 'active',
    activeVoyagesCount: 1,
    monthlySpentUSD: 0.88,
    squadIds: ['squad-growth'],
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
    squadIds: [],
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

export const initialCrew: CrewMember[] = [
  {
    id: 'crew-repo-analyst',
    name: 'Repository Analyst',
    shipId: 'ship-dev',
    squadId: 'squad-dev',
    role: 'Static Analysis & Codebase Cartographer',
    purpose: 'Maps repository architecture, dependencies, git history, and technical debt risks.',
    skills: ['AST Parsing', 'Dependency Graphing', 'Commit Chronology', 'Risk Scoring'],
    steering: 'Cite approved sources and separate evidence from inference.',
    tools: ['local_filesystem', 'git_log_parser', 'repo_scanner'],
    modelProfile: 'Claude 3.7 Sonnet (Hybrid Reasoning)',
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
    squadId: 'squad-dev',
    role: 'Work Breakdown & Dependency Strategist',
    purpose: 'Converts high-level Captain objectives into bounded, executable steps.',
    skills: ['Task Decomposition', 'Risk Assessment', 'Milestone Mapping'],
    steering: 'Always verify prerequisite tasks before proposing autonomous execution.',
    tools: ['task_planner', 'quest_mapper', 'spec_generator'],
    modelProfile: 'Gemini 2.5 Pro (Deep Context)',
    authority: 'draft_only',
    memoryScope: 'ship',
    status: 'active',
    lastVoyage: '5m ago',
    costLast30Days: 0.61
  },
  {
    id: 'crew-market-analyst',
    name: 'Market Intelligence Scout',
    shipId: 'ship-growth',
    squadId: 'squad-growth',
    role: 'Competitive Signals & Audience Researcher',
    purpose: 'Monitors competitor updates, developer tool releases, and public community sentiment.',
    skills: ['Web Scraping', 'Sentiment Extraction', 'Competitive Matrix'],
    steering: 'Filter out marketing hype; extract concrete developer pain points.',
    tools: ['web_search', 'feed_monitor', 'diff_analyzer'],
    modelProfile: 'Gemini 2.5 Flash (Fast)',
    authority: 'read_only',
    memoryScope: 'workspace',
    status: 'active',
    lastVoyage: '1h ago',
    costLast30Days: 0.45
  },
  {
    id: 'crew-brand-reviewer',
    name: 'Voice & Integrity Reviewer',
    shipId: 'ship-growth',
    squadId: 'squad-growth',
    role: 'Copy Integrity & Policy Validator',
    purpose: 'Audits outward copy for narrative alignment and clarity.',
    skills: ['Brand Voice Consistency', 'Clarity Editing', 'Tone Calibration'],
    steering: 'Enforce concise nautical sovereign metaphors without becoming confusing.',
    tools: ['copy_lint', 'style_guide_validator'],
    modelProfile: 'Claude 3.7 Sonnet',
    authority: 'draft_only',
    memoryScope: 'ship',
    status: 'active',
    lastVoyage: '2h ago',
    costLast30Days: 0.43
  },
  {
    id: 'crew-tech-evaluator',
    name: 'Architecture & Security Benchmarker',
    shipId: 'ship-research',
    role: 'Decision Brief Synthesizer',
    purpose: 'Evaluates libraries, models, security boundaries, and runtime performance tradeoffs.',
    skills: ['Benchmark Synthesis', 'Security Boundary Audit', 'Cost Modeling'],
    steering: 'Provide concrete trade-off matrices with quantifiable latency and memory numbers.',
    tools: ['benchmark_harness', 'security_audit', 'cost_calculator'],
    modelProfile: 'DeepSeek-R1 (Local Ollama)',
    authority: 'read_only',
    memoryScope: 'crew',
    status: 'active',
    lastVoyage: '30m ago',
    costLast30Days: 1.25
  }
];

export const initialSquads: Squad[] = [
  {
    id: 'squad-dev',
    name: 'Core Software Delivery Squad',
    shipId: 'ship-dev',
    purpose: 'Rapid issue resolution, patch synthesis, AST refactoring, and regression test suites.',
    crewIds: ['crew-repo-analyst', 'crew-eng-planner'],
    leaderCrewId: 'crew-eng-planner',
    status: 'active',
    createdAt: '2026-03-01T08:00:00Z',
    updatedAt: '2026-03-28T14:30:00Z'
  },
  {
    id: 'squad-growth',
    name: 'Growth & Product Narrative Squad',
    shipId: 'ship-growth',
    purpose: 'Synthesize changelogs, architecture documentation, and developer release briefings.',
    crewIds: ['crew-market-analyst', 'crew-brand-reviewer'],
    leaderCrewId: 'crew-brand-reviewer',
    status: 'active',
    createdAt: '2026-03-05T09:00:00Z',
    updatedAt: '2026-03-29T11:00:00Z'
  }
];

export const initialQuests: Quest[] = [
  {
    id: 'quest-ci-ws',
    title: 'Resolve WebSocket Goroutine Timeout in CI',
    objective: 'Implement explicit SetReadDeadline and graceful connection termination during test suite cleanup.',
    workspaceId: 'galleon-core',
    projectId: 'transport-layer',
    priority: 'urgent',
    status: 'underway',
    suggestedShipId: 'ship-dev',
    assignedShipId: 'ship-dev',
    requiredArtifacts: ['WebSocket Timeout Patch Briefing', 'CI Test Run Verification Log'],
    budgetLimitUSD: 2.50,
    estimatedCostUSD: 0.85,
    mapSteps: [
      { stepNumber: 1, title: 'Locate listener termination loop in pkg/transport', status: 'completed' },
      { stepNumber: 2, title: 'Inject context cancellation and test abort hook', status: 'in_progress' },
      { stepNumber: 3, title: 'Verify test passes 5 consecutive runs without leaks', status: 'pending' }
    ],
    activeVoyageProgress: 65,
    createdAt: '2026-03-30T10:15:00Z',
    updatedAt: '2026-03-31T04:20:00Z',
    discoveriesCount: 3
  },
  {
    id: 'quest-rel-notes',
    title: 'Synthesize Fleet AI v2.4 Release Notes',
    objective: 'Aggregate recent pull requests, security policy upgrades, and multi-agent coordination capabilities into release documentation.',
    workspaceId: 'galleon-core',
    projectId: 'docs-release',
    priority: 'high',
    status: 'ready',
    suggestedShipId: 'ship-growth',
    assignedShipId: 'ship-growth',
    requiredArtifacts: ['v2.4 Release Notes Document', 'Breaking Changes Checklist'],
    budgetLimitUSD: 1.50,
    estimatedCostUSD: 0.40,
    mapSteps: [
      { stepNumber: 1, title: 'Extract commit changelog from v2.3.0..HEAD', status: 'pending' },
      { stepNumber: 2, title: 'Categorize changes by Engine, UI, and Security', status: 'pending' },
      { stepNumber: 3, title: 'Format into verified Markdown artifact for Captain sign-off', status: 'pending' }
    ],
    activeVoyageProgress: 0,
    createdAt: '2026-03-30T14:00:00Z',
    updatedAt: '2026-03-31T01:10:00Z',
    discoveriesCount: 0
  },
  {
    id: 'quest-audit-gw',
    title: 'Audit Model Gateway Latency and Token Budgets',
    objective: 'Benchmark token latency between Anthropic, Gemini, and local Ollama instances across varying context lengths.',
    workspaceId: 'galleon-core',
    projectId: 'telemetry',
    priority: 'medium',
    status: 'ready',
    suggestedShipId: 'ship-research',
    assignedShipId: 'ship-research',
    requiredArtifacts: ['Gateway Benchmark Analysis Table'],
    budgetLimitUSD: 1.00,
    estimatedCostUSD: 0.20,
    mapSteps: [
      { stepNumber: 1, title: 'Sample 50 queries across all three providers', status: 'pending' },
      { stepNumber: 2, title: 'Measure TTFT (Time To First Token) and overall throughput', status: 'pending' },
      { stepNumber: 3, title: 'Output latency comparison to Treasury and Crows Nest', status: 'pending' }
    ],
    activeVoyageProgress: 0,
    createdAt: '2026-03-29T18:00:00Z',
    updatedAt: '2026-03-30T12:00:00Z',
    discoveriesCount: 1
  }
];

export const initialApprovals: CaptainApproval[] = [
  {
    id: 'appr-1',
    questId: 'quest-ci-ws',
    shipId: 'ship-dev',
    crewId: 'crew-repo-analyst',
    title: 'Authorize GitHub Pull Request Submission: WebSocket CI Fix',
    actionType: 'github_issue_create',
    targetResource: 'diezy-labs/claw-crew#pulls',
    draftSummary: 'Developer Delivery Ship completed the fix for WebSocket teardown. Requires Captain’s signature to create PR against master.',
    justification: 'Fixes 100% reproducible socket teardown hang in CI runner.',
    effect: 'Merges cleanly without breaking existing transport ABI.',
    costUSD: 0.05,
    status: 'pending',
    createdAt: '15m ago'
  },
  {
    id: 'appr-2',
    questId: 'quest-audit-gw',
    shipId: 'ship-research',
    crewId: 'crew-tech-evaluator',
    title: 'Expand Local Ollama Memory Cap to 16GB RAM',
    actionType: 'modify_policy',
    targetResource: 'host.system.vram',
    draftSummary: 'Research Ship requests increasing the local model execution budget to load deepseek-r1:14b into VRAM/system RAM.',
    justification: 'Required for high-accuracy local AST parsing without token charges.',
    effect: 'Allocates up to 16GB host memory during local scans.',
    costUSD: 0.00,
    status: 'pending',
    createdAt: '1h ago'
  }
];

export const initialArtifacts: Artifact[] = [
  {
    id: 'art-1',
    questId: 'quest-ci-ws',
    shipId: 'ship-dev',
    producerCrewId: 'crew-repo-analyst',
    title: 'WebSocket Read Deadline Remediation Patch',
    type: 'ci-triage',
    summary: 'Modifies pkg/transport/ws_listener.go to inject SetReadDeadline before terminating reader loop, preventing CI runner thread lock.',
    content: `// Remediation in pkg/transport/ws_listener.go
func (l *WebSocketListener) Close() error {
    l.mu.Lock()
    defer l.mu.Unlock()
    if l.conn != nil {
        _ = l.conn.SetReadDeadline(time.Now())
        return l.conn.Close()
    }
    return nil
}`,
    discoveries: [
      {
        id: 'disc-1',
        type: 'risk',
        title: 'Goroutine leak under quick client disconnects',
        detail: 'Socket read calls without deadlines block indefinitely on Windows and Linux runners.',
        evidenceSource: 'pkg/transport/ws_listener.go:42'
      }
    ],
    evidenceCount: 3,
    voyageCostUSD: 0.12,
    status: 'approved',
    createdAt: '30m ago'
  },
  {
    id: 'art-2',
    questId: 'quest-audit-gw',
    shipId: 'ship-research',
    producerCrewId: 'crew-tech-evaluator',
    title: 'Model Provider Latency & Cost Matrix (March 2026)',
    type: 'decision-brief',
    summary: 'Quantified comparison between Anthropic Claude 3.7 Sonnet, Google Gemini 2.5 Pro/Flash, and local Ollama deepseek-r1.',
    content: `| Provider | Model | TTFT (ms) | Throughput (t/s) | Cost / 1k tokens |
|---|---|---|---|---|
| Anthropic | Claude 3.7 Sonnet | 420ms | 68 t/s | $0.003 |
| Google | Gemini 2.5 Pro | 380ms | 94 t/s | $0.00125 |
| Google | Gemini 2.5 Flash | 185ms | 135 t/s | $0.00015 |
| Ollama (Local) | deepseek-r1:14b | 210ms | 45 t/s | $0.00000 |`,
    discoveries: [
      {
        id: 'disc-2',
        type: 'recommendation',
        title: 'Use Gemini 2.5 Flash for triage and Claude 3.7 for patch writes',
        detail: 'Reduces monthly token consumption by 72% while preserving AST correctness.',
        evidenceSource: 'test/benchmark_eval.json'
      }
    ],
    evidenceCount: 5,
    voyageCostUSD: 0.04,
    status: 'treasure',
    createdAt: '2h ago'
  }
];

export const initialTreasuryLedger: TreasuryLedger[] = [
  {
    id: 'tr-1',
    date: '2026-03-31T04:00:00Z',
    shipId: 'ship-dev',
    questTitle: 'Resolve WebSocket Goroutine Timeout in CI',
    provider: 'Anthropic',
    model: 'claude-3-7-sonnet',
    tokensUsed: 18000,
    costUSD: 0.12
  },
  {
    id: 'tr-2',
    date: '2026-03-31T03:30:00Z',
    shipId: 'ship-growth',
    questTitle: 'Synthesize Fleet AI v2.4 Release Notes',
    provider: 'Google',
    model: 'gemini-2-5-pro',
    tokensUsed: 29700,
    costUSD: 0.04
  },
  {
    id: 'tr-3',
    date: '2026-03-31T02:15:00Z',
    shipId: 'ship-research',
    questTitle: 'Audit Model Gateway Latency and Token Budgets',
    provider: 'Ollama',
    model: 'deepseek-r1:14b',
    tokensUsed: 53500,
    costUSD: 0.00
  }
];

export const initialLogbook: LogbookEntry[] = [
  {
    id: 'log-1',
    timestamp: '08:00:15',
    actorType: 'system',
    actorName: 'Galleon Sovereign Kernel',
    action: 'Booted Sovereign Multi-Agent Fleet Environment v2.4.0',
    entityType: 'ship',
    entityId: 'fleet-all',
    correlationId: 'boot-240',
    severity: 'info'
  },
  {
    id: 'log-2',
    timestamp: '08:02:10',
    actorType: 'quartermaster',
    actorName: 'Developer Delivery Ship Navigator',
    action: 'Embarked on Quest: Resolve WebSocket Goroutine Timeout in CI',
    entityType: 'quest',
    entityId: 'quest-ci-ws',
    correlationId: 'voyage-ws',
    severity: 'info'
  },
  {
    id: 'log-3',
    timestamp: '08:15:40',
    actorType: 'crew',
    actorName: 'Repository Analyst',
    action: 'Generated Artifact: WebSocket Read Deadline Remediation Patch',
    entityType: 'artifact',
    entityId: 'art-1',
    correlationId: 'art-gen-1',
    severity: 'info'
  }
];

export const initialNotifications: NotificationItem[] = [
  {
    id: 'notif-1',
    title: 'Captain Approval Requested',
    description: 'Developer Delivery Ship needs authorization to create a Pull Request on GitHub.',
    type: 'approval',
    read: false,
    createdAt: '15m ago',
    actionLinkTab: 'approvals'
  },
  {
    id: 'notif-2',
    title: 'Quest Underway',
    description: 'Voyage in progress: Resolving WebSocket Goroutine Timeout (65% completed).',
    type: 'quest',
    read: false,
    createdAt: '30m ago',
    actionLinkTab: 'quests'
  }
];

export const initialTrainingSkills: TrainingSkill[] = [
  {
    id: 'skill-ast-parser',
    name: 'AST Codebase Navigation',
    status: 'active',
    version: 1,
    createdAt: '2026-03-01T00:00:00Z',
    createdBy: 'Captain',
    updatedAt: '2026-03-25T00:00:00Z',
    updatedBy: 'Captain',
    purpose: 'Analyzes Abstract Syntax Trees across Rust, TypeScript, and Go codebases.',
    instructions: 'Traverse syntax tree nodes, identify export signatures, and graph dependencies.',
    accessScope: ['ship-dev', 'ship-research']
  },
  {
    id: 'skill-benchmarking',
    name: 'Synthetic Gateway Benchmarking',
    status: 'active',
    version: 1,
    createdAt: '2026-03-10T00:00:00Z',
    createdBy: 'Captain',
    updatedAt: '2026-03-28T00:00:00Z',
    updatedBy: 'Captain',
    purpose: 'Runs high-throughput TTFT and streaming latency measurements against LLM endpoints.',
    instructions: 'Send bounded probe payloads, measure time to first token, and report p99 latency.',
    accessScope: ['ship-research']
  }
];

export const initialGlobalSteering: GlobalSteering[] = [
  {
    id: 'steering-1',
    name: 'Zero Compromise on Production Safety',
    directive: 'Never perform destructive file deletions or unauthenticated remote calls without explicit Owner sign-off.',
    priority: 'critical',
    enforcement: 'required',
    appliesTo: ['all-ships'],
    status: 'active',
    version: 1,
    createdAt: '2026-03-01T00:00:00Z',
    createdBy: 'Captain',
    updatedAt: '2026-03-20T00:00:00Z',
    updatedBy: 'Captain'
  }
];

export const initialSteeringDirectives: SteeringDirective[] = [
  {
    id: 'dir-1',
    name: 'Evidence Distinction Policy',
    targetType: 'role',
    targetId: 'crew-repo-analyst',
    guidance: 'Always distinguish facts in the code from assumptions. Cite exact line numbers.',
    priority: 1,
    overridePolicy: 'override',
    status: 'active',
    version: 1,
    createdAt: '2026-03-05T00:00:00Z',
    createdBy: 'Captain',
    updatedAt: '2026-03-22T00:00:00Z',
    updatedBy: 'Captain'
  }
];

export const initialTrainingHooks: TrainingHook[] = [
  {
    id: 'hook-1',
    name: 'Pre-Voyage Policy Verification',
    triggerEvent: 'voyage_start',
    actionType: 'verify_budget',
    actionConfig: { maxBudgetUSD: 2.50 },
    executionMode: 'automatic',
    failureHandling: 'notify',
    status: 'active',
    version: 1,
    createdAt: '2026-03-01T00:00:00Z',
    createdBy: 'Captain',
    updatedAt: '2026-03-20T00:00:00Z',
    updatedBy: 'Captain'
  }
];

export const initialJournalSessions: JournalSession[] = [
  {
    id: 'journal-1',
    title: 'Fleet AI Architecture Strategy Review',
    updatedAt: 'Just now',
    workspaceId: 'galleon-core',
    lastNote: 'Aligned on single source of truth across Tauri v2 IPC and Web REST APIs.',
    isPinned: true,
    isArchived: false,
    isTemporary: false,
    savedArtifactCount: 1,
    questDraftCount: 1,
    messages: [
      {
        id: 'msg-j1',
        sender: 'owner',
        content: 'Review the architecture and ensure all UI elements call real backend endpoints.',
        timestamp: '08:00 AM'
      },
      {
        id: 'msg-j2',
        sender: 'quartermaster',
        content: 'Aye Captain! All mock arrays have been unified into persistent backend collections across Tauri and Web.',
        timestamp: '08:01 AM'
      }
    ]
  }
];

export const initialChatMessages: ChatMessage[] = [
  {
    id: 'chat-init-1',
    sender: 'quartermaster',
    content: 'Greetings, Sovereign Captain! Quartermaster aboard and reporting for duty. Your Fleet has 3 active Ships anchored in the harbor. How may I assist your strategy today?',
    timestamp: '08:00 AM',
    suggestedActions: [
      { label: 'Review CI Fix Progress', actionType: 'open_tab', payload: 'quests' },
      { label: 'Inspect Pending Approvals', actionType: 'open_tab', payload: 'approvals' }
    ]
  }
];

export const initialDiagnostics = [
  { id: 'd-1', component: 'Tauri Native IPC Host', status: 'healthy', latency: '4ms', detail: 'Local process IPC active' },
  { id: 'd-2', component: 'Multi-Platform Gateway', status: 'healthy', latency: '12ms', detail: 'Serving on port 3000' },
  { id: 'd-3', component: 'Host Landlock Sandbox', status: 'healthy', latency: '0ms', detail: 'Scoped directory isolation active' },
  { id: 'd-4', component: 'Local Ollama Endpoint', status: 'healthy', latency: '18ms', detail: 'http://127.0.0.1:11434 online' },
  { id: 'd-5', component: 'WebSocket Read Deadline', status: 'warning', latency: '65ms', detail: 'Socket timeout deadline needs enforcement' }
];

export const initialSnapshots = [
  {
    id: 'snap-1',
    title: 'Sovereign Fleet Golden Baseline Snapshot',
    createdAt: '2 hours ago',
    size: '14.2 MB',
    schemaVersion: 'v2.4',
    entitiesCount: '3 Ships · 5 Crew · 3 Quests · 2 Artifacts'
  }
];

export const initialEngineProcesses = [
  { id: 'proc-1', name: 'fleet-ai-gateway', command: 'node server.js', pid: 1024, cpu: 0.4, memoryMB: 48, uptime: '2h 15m', status: 'running' as const },
  { id: 'proc-2', name: 'ollama-runner', command: 'ollama serve', pid: 2048, cpu: 1.2, memoryMB: 380, uptime: '4h 10m', status: 'running' as const },
  { id: 'proc-3', name: 'ast-scanner-worker', command: 'cargo check --worker', pid: 3102, cpu: 0.1, memoryMB: 28, uptime: '1h 05m', status: 'idle' as const }
];

// NOTE: initialFleetPolicies / initialRiskTiers removed — risk-tier & policy
// facts are the Go engine's SSOT (GET /api/fleet/policies, fleet/services.go).
// Re-adding them here duplicates domain state across tiers and will fail the
// tests/architecture/no_duplicate_state gate (F2).

// Seed data structure for /api/fleet/seed endpoint response
export const seedData = {
  ships: initialShips,
  crew: initialCrew,
  squads: initialSquads,
  quests: initialQuests,
  artifacts: initialArtifacts,
  approvals: initialApprovals,
  logbook: initialLogbook,
  treasuryLedger: initialTreasuryLedger,
  notifications: initialNotifications,
  journalSessions: initialJournalSessions,
  chatMessages: initialChatMessages,
  trainingSkills: initialTrainingSkills,
  globalSteering: initialGlobalSteering,
  steeringDirectives: initialSteeringDirectives,
  trainingHooks: initialTrainingHooks
};

export const initialHarborTools = [
  {
    name: 'GitHub Repository Connector',
    target: 'diezy-labs/claw-crew (Branch: feat/enhance-agent-phase2)',
    auth: 'Read-only default + Captain Approval for issues/PRs',
    status: 'Synced'
  },
  {
    name: 'Local Filesystem Sandbox',
    target: 'Tauri Host Landlock Sandboxing Active',
    auth: 'Scoped to current repository directory',
    status: 'Secure'
  },
  {
    name: 'Webhook Event Listener',
    target: 'https://fleet.diezy-labs.com/api/webhooks',
    auth: 'HMAC SHA-256 Signature Verification',
    status: 'Active'
  }
];
