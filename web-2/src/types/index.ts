export type ThemeMode = 'dark' | 'light';
export type ColorTone = 'teal' | 'emerald' | 'sapphire' | 'amber' | 'amethyst' | 'crimson';

export type NavigationTab =
  | 'quarterdeck'
  | 'realm'
  | 'flag-bridge'
  | 'quests'
  | 'captains-journal'
  | 'mission-board'
  | 'ships'
  | 'crew'
  | 'artifacts'
  | 'approvals'
  | 'treasury'
  | 'logbook'
  | 'harbor'
  | 'fleet-code'
  | 'crows-nest'
  | 'engine-room'
  | 'shipyard'
  | 'settings'
  | 'quartermaster'; // backward compatibility alias

export type TerminologyMode = 'adventure' | 'professional';
export type DensityMode = 'comfortable' | 'compact';
export type UpdateChannel = 'stable' | 'preview' | 'nightly';

export type SettingsCategory =
  | 'profile'
  | 'appearance'
  | 'notifications'
  | 'journal'
  | 'defaults'
  | 'privacy'
  | 'backup'
  | 'runtime'
  | 'updates'
  | 'accessibility'
  | 'developer'
  | 'experimental'
  | 'about';

export interface FleetSettings {
  profile: {
    displayName: string;
    narrativeTitle: string;
    preferredAddress: string;
    communicationStyle: string;
    defaultLanguage: string;
    workingHours: string;
    weeklyBriefing: string;
  };
  appearance: {
    theme: ThemeMode | 'system';
    colorTone?: ColorTone;
    density: DensityMode;
    terminology: TerminologyMode;
    showFunctionalSubtitles: boolean;
    showAdvancedTechnicalNames: boolean;
    displayLanguage: string;
    outputLanguage: string;
  };
  notifications: {
    captainApprovalRequested: boolean;
    strategicDecisionRequested: boolean;
    questBlocked: boolean;
    questCompleted: boolean;
    everyVoyageProgress: boolean;
    scheduledQuestFailed: boolean;
    budgetSoftLimitReached: boolean;
    providerAttention: boolean;
    shipHealthWarning: boolean;
    desktopNotifications: boolean;
    soundEnabled: boolean;
    quietHoursMode: 'working_hours' | 'custom' | 'never';
  };
  journal: {
    defaultChatTarget: string;
    defaultPrivacy: string;
    autoNameSessions: boolean;
    learningMode: 'ask' | 'propose' | 'never';
    retentionActive: string;
    retentionTemporary: string;
    autoArchiveDays: number;
    includeArchivedInSearch: boolean;
  };
  defaults: {
    defaultFleet: string;
    defaultWorkspace: string;
    defaultProject: string;
    defaultPriority: 'low' | 'medium' | 'high' | 'urgent';
    defaultPlanningMode: 'guided' | 'advanced';
    defaultRouting: string;
    defaultModelProfile: string;
    defaultLearningMode: string;
  };
  privacy: {
    dataDirectory: string;
    shareAnonymousDiagnostics: boolean;
    shareCrashReports: boolean;
    artifactRetention: string;
    logbookRetention: string;
    runTraceRetention: string;
    redactKnownSecrets: boolean;
    confirmExportingSensitive: boolean;
  };
  runtime: {
    launchAtSignIn: boolean;
    startRuntimeWithApp: boolean;
    keepRuntimeActiveOnClose: boolean;
    closeAction: 'minimize_tray' | 'quit';
    openAtStartup: string;
    localAddress: string;
  };
  updates: {
    channel: UpdateChannel;
    autoCheck: boolean;
    autoDownload: boolean;
    lastChecked: string;
  };
  accessibility: {
    textSize: string;
    increaseContrast: boolean;
    reduceMotion: boolean;
    alwaysShowFocus: boolean;
    useKeyboardShortcuts: boolean;
    confirmDestructiveActions: boolean;
    explainTechnicalErrors: boolean;
  };
  developer: {
    developerMode: boolean;
    localApiEnabled: boolean;
    includeDebugMetadata: boolean;
    showInternalEntityIds: boolean;
    verboseLocalLogs: boolean;
  };
  experimental: {
    multiShipPreview: boolean;
    tempAgentPromotion: boolean;
    advancedMapStudio: boolean;
    localModelAutoRouting: boolean;
    crossShipQuestPreview: boolean;
    newQuartermasterBriefing: boolean;
  };
}

export type QuestTab = 'active' | 'planned' | 'recurring' | 'completed' | 'treasures' | 'maps';

export interface JournalSession {
  id: string;
  title: string;
  updatedAt: string;
  workspaceId: string;
  lastNote: string;
  isPinned: boolean;
  isArchived: boolean;
  isTemporary: boolean;
  savedArtifactCount: number;
  questDraftCount: number;
  messages: ChatMessage[];
}

export type RiskClass = 'read_only' | 'draft' | 'write' | 'sensitive' | 'destructive';

export type QuestStatus =
  | 'backlog'
  | 'ready'
  | 'underway'
  | 'awaiting_captain'
  | 'review'
  | 'completed'
  | 'treasured'
  | 'blocked'
  | 'anchored';

export interface CrewMember {
  id: string;
  name: string;
  shipId: string;
  role: string;
  purpose: string;
  skills: string[];
  tools: string[];
  modelProfile: string;
  authority: 'read_only' | 'draft_only' | 'gated_write';
  memoryScope: 'crew' | 'ship' | 'workspace';
  status: 'active' | 'standby' | 'paused';
  avatar?: string;
  lastVoyage?: string;
  costLast30Days: number;
}

export interface ShipCharter {
  purpose: string;
  acceptedQuestTypes: string[];
  crewAuthority: string;
  prohibitedActions: string[];
  budgetPerVoyageUSD: number;
  monthlyBudgetUSD: number;
  memorySharing: 'ship_scoped' | 'project_scoped' | 'isolated';
}

export interface Ship {
  id: string;
  name: string;
  fleetId: string;
  tagline: string;
  homeScope: string;
  navigatorName: string;
  status: 'active' | 'attention' | 'anchored';
  charter: ShipCharter;
  crewIds: string[];
  activeVoyagesCount: number;
  monthlySpentUSD: number;
}

export interface MapStep {
  stepNumber: number;
  title: string;
  assignedCrewId?: string;
  status: 'pending' | 'in_progress' | 'completed' | 'blocked';
  outputArtifactType?: string;
}

export interface Quest {
  id: string;
  title: string;
  objective: string;
  workspaceId: string;
  projectId: string;
  priority: 'low' | 'medium' | 'high' | 'urgent';
  status: QuestStatus;
  suggestedShipId: string;
  assignedShipId?: string;
  requiredArtifacts: string[];
  budgetLimitUSD: number;
  estimatedCostUSD: number;
  mapSteps: MapStep[];
  activeVoyageProgress?: number; // 0 - 100
  createdAt: string;
  updatedAt: string;
  discoveriesCount: number;
}

export interface Discovery {
  id: string;
  type: 'risk' | 'opportunity' | 'unknown' | 'recommendation';
  title: string;
  detail: string;
  evidenceSource: string;
}

export interface Artifact {
  id: string;
  questId: string;
  shipId: string;
  producerCrewId: string;
  title: string;
  type: 'health-brief' | 'ci-triage' | 'readiness-checklist' | 'content-strategy' | 'adr-draft' | 'decision-brief';
  summary: string;
  content: string;
  discoveries: Discovery[];
  evidenceCount: number;
  voyageCostUSD: number;
  status: 'needs_review' | 'approved' | 'treasure';
  createdAt: string;
}

export interface CaptainApproval {
  id: string;
  questId: string;
  shipId: string;
  crewId: string;
  title: string;
  actionType: 'github_issue_create' | 'publish_content' | 'deploy_service' | 'modify_policy';
  targetResource: string;
  draftSummary: string;
  justification: string;
  effect: string;
  costUSD: number;
  status: 'pending' | 'approved' | 'rejected';
  createdAt: string;
}

export interface LogbookEntry {
  id: string;
  timestamp: string;
  actorType: 'owner' | 'quartermaster' | 'navigator' | 'crew' | 'system';
  actorName: string;
  action: string;
  entityType: 'quest' | 'artifact' | 'ship' | 'approval' | 'policy';
  entityId: string;
  correlationId: string;
  severity: 'info' | 'success' | 'warning' | 'alert';
}

export interface TreasuryLedger {
  id: string;
  date: string;
  shipId: string;
  questTitle: string;
  provider: string;
  model: string;
  tokensUsed: number;
  costUSD: number;
}

export interface NotificationItem {
  id: string;
  title: string;
  description: string;
  type: 'approval' | 'quest' | 'artifact' | 'treasury' | 'health';
  read: boolean;
  createdAt: string;
  actionLinkTab?: NavigationTab;
}

export interface ChatMessage {
  id: string;
  sender: 'owner' | 'quartermaster';
  content: string;
  timestamp: string;
  suggestedActions?: {
    label: string;
    actionType: 'create_quest' | 'save_artifact' | 'assign_ship' | 'open_tab';
    payload?: any;
  }[];
  generatedArtifactPreview?: Partial<Artifact>;
}
