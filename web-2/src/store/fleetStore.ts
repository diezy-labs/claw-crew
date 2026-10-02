import { create } from 'zustand';
import { apiClient } from '../utils/apiClient';
import {
  initialShips,
  initialCrew,
  initialSquads,
  initialQuests,
  initialArtifacts,
  initialApprovals,
  initialTreasuryLedger,
  initialLogbook,
  initialNotifications,
  initialTrainingSkills,
  initialGlobalSteering,
  initialSteeringDirectives,
  initialTrainingHooks,
  initialJournalSessions,
  initialChatMessages,
  seedData
} from '../utils/seedData';
import {
  ThemeMode,
  ColorTone,
  NavigationTab,
  Ship,
  CrewMember,
  Squad,
  TrainingSkill,
  GlobalSteering,
  SteeringDirective,
  TrainingHook,
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
  systemMetrics?: { gateway_latency_ms: number; active_threads: number; isolation_mode: string; memory_db_mb: number; };
  executiveBriefing?: any[];
  harborProviders?: any[];
  fleetMetrics?: { active_ships: number; assigned_crew: number; running_voyages: number; status: string; };
  diagnostics?: any[];
  snapshots?: any[];
  engineProcesses?: any[];
  fleetPolicies?: any[];
  riskTiers?: any[];

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
  squads: Squad[];
  crew: CrewMember[];
  trainingSkills: TrainingSkill[];
  globalSteering: GlobalSteering[];
  steeringDirectives: SteeringDirective[];
  trainingHooks: TrainingHook[];
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
  isRemoteAccessModalOpen: boolean;

  // Squad Actions
  addSquad: (squad: Omit<Squad, 'id' | 'createdAt' | 'updatedAt'>) => string;
  updateSquad: (id: string, updates: Partial<Squad>) => void;
  deleteSquad: (id: string) => void;

  // Training Officer Actions
  addTrainingSkill: (skill: Omit<TrainingSkill, 'id' | 'createdAt' | 'updatedAt' | 'version'>) => string;
  updateTrainingSkill: (id: string, updates: Partial<TrainingSkill>) => void;
  deleteTrainingSkill: (id: string) => void;
  addGlobalSteering: (order: Omit<GlobalSteering, 'id' | 'createdAt' | 'updatedAt' | 'version'>) => string;
  updateGlobalSteering: (id: string, updates: Partial<GlobalSteering>) => void;
  deleteGlobalSteering: (id: string) => void;
  addSteeringDirective: (steering: Omit<SteeringDirective, 'id' | 'createdAt' | 'updatedAt' | 'version'>) => string;
  updateSteeringDirective: (id: string, updates: Partial<SteeringDirective>) => void;
  deleteSteeringDirective: (id: string) => void;
  addTrainingHook: (hook: Omit<TrainingHook, 'id' | 'createdAt' | 'updatedAt' | 'version'>) => string;
  updateTrainingHook: (id: string, updates: Partial<TrainingHook>) => void;
  deleteTrainingHook: (id: string) => void;

  // Actions
  setRemoteAccessModalOpen: (open: boolean) => void;
  setTheme: (theme: ThemeMode) => void;
  toggleTheme: () => void;
  colorTone: ColorTone;
  setColorTone: (tone: ColorTone) => void;
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
  updateCrewMember: (id: string, updates: Partial<CrewMember>) => void;
  createShip: (shipData: Partial<Ship>) => string;
  markNotificationRead: (id: string) => void;
  markAllNotificationsRead: () => void;
  addNotification: (notification: Omit<NotificationItem, 'id' | 'createdAt' | 'read'>) => void;
  simulateVoyageTick: () => void;
  fetchRealData: () => Promise<void>;
  hydrateSeedData: () => Promise<void>;
  ringDeckBell: () => Promise<string>;

  // Captain's Journal Actions
  createJournalSession: (title: string, workspaceId?: string) => void;
  sendJournalMessage: (sessionId: string, content: string) => void;
  togglePinJournalSession: (id: string) => void;
  archiveJournalSession: (id: string) => void;
  convertJournalToArtifact: (sessionId: string) => void;
  convertJournalToQuest: (sessionId: string) => void;
}

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
    colorTone: (typeof localStorage !== 'undefined' ? (localStorage.getItem('galleon_color_tone') as ColorTone) : null) || 'teal',
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

const persistStateCollection = (name: string, data: any) => {
  apiClient.saveCollection(name, data).catch((err) => {
    console.warn(`[Store] Background sync failed for ${name}:`, err);
  });
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
  squads: initialSquads,
  crew: initialCrew,
  trainingSkills: initialTrainingSkills,
  globalSteering: initialGlobalSteering,
  steeringDirectives: initialSteeringDirectives,
  trainingHooks: initialTrainingHooks,
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
  isRemoteAccessModalOpen: false,

  setRemoteAccessModalOpen: (open) => set({ isRemoteAccessModalOpen: open }),
  setActiveSettingsCategory: (cat) => set({ activeSettingsCategory: cat }),
  toggleSidebarCollapsed: () => set((state) => ({ isSidebarCollapsed: !state.isSidebarCollapsed })),
  setSidebarCollapsed: (collapsed) => set({ isSidebarCollapsed: collapsed }),
  setMobileSidebarOpen: (open) => set({ isMobileSidebarOpen: open }),
  setFleetPulseOpen: (open) => set({ isFleetPulseOpen: open }),
  toggleFleetPulse: () => set((state) => ({ isFleetPulseOpen: !state.isFleetPulseOpen })),
  toggleAnchor: () => {
    const isDropped = !get().isAnchorDropped;
    set((state) => {
      const nextNotifs = [
        {
          id: 'notif-' + Date.now(),
          title: isDropped ? 'Anchor Dropped: All Autonomous Voyages Paused' : 'Anchor Weighed: Voyages Resumed',
          description: isDropped
            ? 'Emergency pause activated. All autonomous background agent loops are safely halted.'
            : 'Fleet operations resumed. Autonomous agent voyages and SOP executions are active.',
          type: 'health' as const,
          read: false,
          createdAt: 'Just now',
          actionLinkTab: 'crows-nest' as const
        },
        ...state.notifications
      ];
      const nextLogbook = [
        {
          id: 'log-' + Date.now(),
          timestamp: new Date().toLocaleTimeString(),
          actorType: 'owner' as const,
          actorName: 'Pirate King (You)',
          action: isDropped ? 'EMERGENCY HALT: Dropped Anchor (Paused all agent voyages)' : 'RESUME: Weighed Anchor (Resumed agent voyages)',
          entityType: 'ship' as const,
          entityId: 'fleet-all',
          correlationId: 'anchor-' + Date.now(),
          severity: isDropped ? ('alert' as const) : ('info' as const)
        },
        ...state.logbook
      ];
      persistStateCollection('notifications', nextNotifs);
      persistStateCollection('logbook', nextLogbook);
      return {
        isAnchorDropped: isDropped,
        notifications: nextNotifs,
        logbook: nextLogbook
      };
    });
  },

  fetchRealData: async () => {
    try {
      const seedMap: Record<string, any[]> = {
        squads: initialSquads,
        ships: initialShips,
        crew: initialCrew,
        trainingSkills: initialTrainingSkills,
        globalSteering: initialGlobalSteering,
        steeringDirectives: initialSteeringDirectives,
        trainingHooks: initialTrainingHooks,
        quests: initialQuests,
        artifacts: initialArtifacts,
        approvals: initialApprovals,
        logbook: initialLogbook,
        treasuryLedger: initialTreasuryLedger,
        notifications: initialNotifications,
        journalSessions: initialJournalSessions,
        chatMessages: initialChatMessages
      };

      for (const [col, defaultList] of Object.entries(seedMap)) {
        try {
          let data = await apiClient.getCollection<any>(col);
          if (!data || data.length === 0) {
            data = defaultList;
            await apiClient.saveCollection(col, data);
          }
          set({ [col]: data } as any);
        } catch (err) {
          console.warn(`[Store] Error loading collection ${col}:`, err);
        }
      }

      try {
        const [sysMetrics, briefing, providers, fleetMet, diag, snaps, procs, pols] = await Promise.all([
          apiClient.getSystemMetrics(),
          apiClient.getExecutiveBriefing(),
          apiClient.getHarborProviders(),
          apiClient.getFleetMetrics(),
          apiClient.getDiagnostics(),
          apiClient.getSnapshots(),
          apiClient.getEngineProcesses(),
          apiClient.getFleetPolicies()
        ]);

        set({
          systemMetrics: sysMetrics as { gateway_latency_ms: number; active_threads: number; isolation_mode: string; memory_db_mb: number; } | undefined,
          executiveBriefing: briefing as any[],
          harborProviders: providers as any[],
          fleetMetrics: fleetMet as { active_ships: number; assigned_crew: number; running_voyages: number; status: string; } | undefined,
          diagnostics: diag as any[],
          snapshots: snaps as any[],
          engineProcesses: procs as any[],
          fleetPolicies: pols?.policies || [],
          riskTiers: pols?.riskTiers || []
        });
      } catch (err) {
        console.warn('[Store] Error loading system/fleet telemetry:', err);
      }
    } catch (e) {
      console.warn('[Store] fetchRealData unexpected error:', e);
    }
  },

  hydrateSeedData: async () => {
    try {
      // API response type for /api/fleet/seed
      interface SeedResponse {
        seed: {
          squads: any[];
          ships: any[];
          crew: any[];
          quests: any[];
          artifacts: any[];
          approvals: any[];
          logbook: any[];
          treasuryLedger: any[];
          notifications: any[];
          journalSessions: any[];
          chatMessages: any[];
          trainingSkills: any[];
          globalSteering: any[];
          steeringDirectives: any[];
          trainingHooks: any[];
        };
      }
      const res = await apiClient.get<SeedResponse>('/api/fleet/seed');
      if (res && res.seed) {
        // Update collections with seed data
        const seedMap: Record<string, any[]> = {
          squads: res.seed.squads,
          ships: res.seed.ships,
          crew: res.seed.crew,
          quests: res.seed.quests,
          artifacts: res.seed.artifacts,
          approvals: res.seed.approvals,
          logbook: res.seed.logbook,
          treasuryLedger: res.seed.treasuryLedger,
          notifications: res.seed.notifications,
          journalSessions: res.seed.journalSessions,
          chatMessages: res.seed.chatMessages,
          trainingSkills: res.seed.trainingSkills,
          globalSteering: res.seed.globalSteering,
          steeringDirectives: res.seed.steeringDirectives,
          trainingHooks: res.seed.trainingHooks
        };
        for (const [col, data] of Object.entries(seedMap)) {
          if (Array.isArray(data) && data.length > 0) {
            set({ [col]: data } as any);
          }
        }
      }
    } catch (err) {
      console.warn('[Store] hydrateSeedData API error, using mock:', err);
      // Fallback to mock data
      const seedMap: Record<string, any[]> = {
        squads: seedData.squads,
        ships: seedData.ships,
        crew: seedData.crew,
        quests: seedData.quests,
        artifacts: seedData.artifacts,
        approvals: seedData.approvals,
        logbook: seedData.logbook,
        treasuryLedger: seedData.treasuryLedger,
        notifications: seedData.notifications,
        journalSessions: seedData.journalSessions,
        chatMessages: seedData.chatMessages,
        trainingSkills: seedData.trainingSkills,
        globalSteering: seedData.globalSteering,
        steeringDirectives: seedData.steeringDirectives,
        trainingHooks: seedData.trainingHooks
      };
      for (const [col, data] of Object.entries(seedMap)) {
        set({ [col]: data } as any);
      }
    }
  },

  ringDeckBell: async () => {
    return await apiClient.ringDeckBell();
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

      if (logAudit) {
        persistStateCollection('logbook', nextLogbook);
      }

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

  colorTone: (typeof localStorage !== 'undefined' ? (localStorage.getItem('galleon_color_tone') as ColorTone) : null) || 'teal',

  setColorTone: (tone) => {
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem('galleon_color_tone', tone);
    }
    if (typeof document !== 'undefined') {
      document.documentElement.setAttribute('data-color-tone', tone);
    }
    set((state) => ({
      colorTone: tone,
      settings: {
        ...state.settings,
        appearance: {
          ...state.settings.appearance,
          colorTone: tone
        }
      }
    }));
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

    const nextWithUser = [...get().chatMessages, userMsg];
    set({ chatMessages: nextWithUser });
    apiClient.saveCollection('chatMessages', nextWithUser).catch(console.error);

    // Call real Quartermaster AI Assistant API
    apiClient.chatQuartermaster(content).then((aiResponse) => {
      const qmMsg: ChatMessage = {
        id: 'qm-' + Date.now(),
        sender: 'quartermaster',
        content: aiResponse.reply,
        timestamp: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
        suggestedActions: (aiResponse.suggestedActions || []).map((a: any) => ({
          label: a.label,
          actionType: a.actionType as any,
          payload: a.payload
        })),
        generatedArtifactPreview: aiResponse.generatedArtifactPreview
      };

      const finalMessages = [...get().chatMessages, qmMsg];
      set({ chatMessages: finalMessages });
      apiClient.saveCollection('chatMessages', finalMessages).catch(console.error);
    }).catch((err) => {
      console.error('Quartermaster API call failed:', err);
    });
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

    set((state) => {
      const nextQuests = [newQuest, ...state.quests];
      const nextNotifs = [
        {
          id: 'notif-' + Date.now(),
          title: 'Quest Created',
          description: `"${newQuest.title}" was routed to ${newQuest.suggestedShipId === 'ship-dev' ? 'Developer Delivery Ship' : 'Specialist Ship'}.`,
          type: 'quest' as const,
          read: false,
          createdAt: 'Just now',
          actionLinkTab: 'mission-board' as const
        },
        ...state.notifications
      ];
      const nextLogbook = [
        {
          id: 'log-' + Date.now(),
          timestamp: new Date().toLocaleTimeString(),
          actorType: 'quartermaster' as const,
          actorName: 'Quartermaster Executive',
          action: `Created and routed Quest: "${newQuest.title}"`,
          entityType: 'quest' as const,
          entityId: id,
          correlationId: 'voyage-' + Math.random().toString(36).substring(7),
          severity: 'info' as const
        },
        ...state.logbook
      ];

      persistStateCollection('quests', nextQuests);
      persistStateCollection('notifications', nextNotifs);
      persistStateCollection('logbook', nextLogbook);

      return {
        quests: nextQuests,
        notifications: nextNotifs,
        logbook: nextLogbook
      };
    });
  },

  updateQuestStatus: (id, status) => {
    set((state) => {
      const nextQuests = state.quests.map((q) => (q.id === id ? { ...q, status, updatedAt: new Date().toISOString() } : q));
      persistStateCollection('quests', nextQuests);
      return { quests: nextQuests };
    });
  },

  runQuestVoyage: (id) => {
    set((state) => {
      const nextQuests = state.quests.map((q) =>
        q.id === id ? { ...q, status: 'underway' as QuestStatus, activeVoyageProgress: 15, updatedAt: new Date().toISOString() } : q
      );
      const nextNotifs = [
        {
          id: 'notif-' + Date.now(),
          title: 'Voyage Set Sail',
          description: 'Specialist Crew began execution of assigned Map steps.',
          type: 'quest' as const,
          read: false,
          createdAt: 'Just now',
          actionLinkTab: 'mission-board' as const
        },
        ...state.notifications
      ];
      persistStateCollection('quests', nextQuests);
      persistStateCollection('notifications', nextNotifs);
      return { quests: nextQuests, notifications: nextNotifs };
    });
  },

  handleApproval: (id, decision) => {
    const appr = get().approvals.find((a) => a.id === id);
    if (!appr) return;

    set((state) => {
      const nextApprovals = state.approvals.map((a) => (a.id === id ? { ...a, status: decision } : a));
      const nextNotifs = [
        {
          id: 'notif-' + Date.now(),
          title: decision === 'approved' ? 'Action Approved by Captain' : 'Action Rejected by Captain',
          description: `"${appr.title}" has been ${decision}. Logbook updated with signature.`,
          type: 'approval' as const,
          read: false,
          createdAt: 'Just now',
          actionLinkTab: 'approvals' as const
        },
        ...state.notifications
      ];
      const nextLogbook = [
        {
          id: 'log-' + Date.now(),
          timestamp: new Date().toLocaleTimeString(),
          actorType: 'owner' as const,
          actorName: 'Pirate King (You)',
          action: `${decision.toUpperCase()}: ${appr.title} (${appr.targetResource})`,
          entityType: 'approval' as const,
          entityId: id,
          correlationId: 'appr-' + id,
          severity: decision === 'approved' ? ('success' as const) : ('warning' as const)
        },
        ...state.logbook
      ];
      persistStateCollection('approvals', nextApprovals);
      persistStateCollection('notifications', nextNotifs);
      persistStateCollection('logbook', nextLogbook);
      return { approvals: nextApprovals, notifications: nextNotifs, logbook: nextLogbook };
    });
  },

  promoteArtifactToTreasure: (id) => {
    const art = get().artifacts.find((a) => a.id === id);
    if (!art) return;

    set((state) => {
      const nextArtifacts = state.artifacts.map((a) => (a.id === id ? { ...a, status: 'treasure' as const } : a));
      const nextNotifs = [
        {
          id: 'notif-' + Date.now(),
          title: 'Treasure Claimed!',
          description: `"${art.title}" was verified and claimed as high-value organizational Treasure.`,
          type: 'artifact' as const,
          read: false,
          createdAt: 'Just now',
          actionLinkTab: 'artifacts' as const
        },
        ...state.notifications
      ];
      const nextLogbook = [
        {
          id: 'log-' + Date.now(),
          timestamp: new Date().toLocaleTimeString(),
          actorType: 'owner' as const,
          actorName: 'Pirate King (You)',
          action: `Promoted Artifact to Treasure: ${art.title}`,
          entityType: 'artifact' as const,
          entityId: id,
          correlationId: 'treasure-' + id,
          severity: 'success' as const
        },
        ...state.logbook
      ];
      persistStateCollection('artifacts', nextArtifacts);
      persistStateCollection('notifications', nextNotifs);
      persistStateCollection('logbook', nextLogbook);
      return { artifacts: nextArtifacts, notifications: nextNotifs, logbook: nextLogbook };
    });
  },

  saveArtifact: (artifactData) => {
    const id = 'art-' + Date.now();
    const newArtifact: Artifact = {
      ...artifactData,
      id,
      createdAt: new Date().toISOString()
    };

    set((state) => {
      const nextArtifacts = [newArtifact, ...state.artifacts];
      const nextNotifs = [
        {
          id: 'notif-' + Date.now(),
          title: 'Artifact Saved',
          description: `"${newArtifact.title}" is now available in the Artifacts Gallery.`,
          type: 'artifact' as const,
          read: false,
          createdAt: 'Just now',
          actionLinkTab: 'artifacts' as const
        },
        ...state.notifications
      ];
      persistStateCollection('artifacts', nextArtifacts);
      persistStateCollection('notifications', nextNotifs);
      return { artifacts: nextArtifacts, notifications: nextNotifs };
    });
  },

  addCrewMember: (crewData) => {
    const id = 'crew-' + Date.now();
    const newMember: CrewMember = {
      ...crewData,
      id
    };

    set((state) => {
      // If member has squad assigned, update the squad's crewIds list
      const updatedSquads = newMember.squadId
        ? state.squads.map((sq) =>
            sq.id === newMember.squadId && !sq.crewIds.includes(id)
              ? { ...sq, crewIds: [...sq.crewIds, id] }
              : sq
          )
        : state.squads;

      const nextCrew = [...state.crew, newMember];
      const nextShips = state.ships.map((s) => (s.id === newMember.shipId ? { ...s, crewIds: [...s.crewIds, id] } : s));
      const nextNotifs = [
        {
          id: 'notif-' + Date.now(),
          title: 'Crew Berth Assigned',
          description: `${newMember.name} joined ${state.ships.find((s) => s.id === newMember.shipId)?.name || 'the Fleet'}.`,
          type: 'quest' as const,
          read: false,
          createdAt: 'Just now',
          actionLinkTab: 'crew' as const
        },
        ...state.notifications
      ];

      persistStateCollection('crew', nextCrew);
      persistStateCollection('squads', updatedSquads);
      persistStateCollection('ships', nextShips);
      persistStateCollection('notifications', nextNotifs);

      return {
        crew: nextCrew,
        squads: updatedSquads,
        ships: nextShips,
        notifications: nextNotifs
      };
    });
  },

  updateCrewMember: (id, updates) => {
    set((state) => {
      // If squadId changed, update squad mappings
      let updatedSquads = state.squads;
      if (updates.squadId !== undefined) {
        updatedSquads = state.squads.map((sq) => {
          if (sq.id === updates.squadId && !sq.crewIds.includes(id)) {
            return { ...sq, crewIds: [...sq.crewIds, id] };
          }
          if (sq.id !== updates.squadId && sq.crewIds.includes(id)) {
            return { ...sq, crewIds: sq.crewIds.filter((cId) => cId !== id) };
          }
          return sq;
        });
      }

      const nextCrew = state.crew.map((c) => (c.id === id ? { ...c, ...updates } : c));
      persistStateCollection('crew', nextCrew);
      if (updates.squadId !== undefined) {
        persistStateCollection('squads', updatedSquads);
      }

      return {
        crew: nextCrew,
        squads: updatedSquads,
        notifications: [
          {
            id: 'notif-' + Date.now(),
            title: 'Specialist Updated',
            description: `Updated profile & bounds for ${state.crew.find((c) => c.id === id)?.name || 'Specialist'}.`,
            type: 'quest' as const,
            read: false,
            createdAt: 'Just now',
            actionLinkTab: 'crew' as const
          },
          ...state.notifications
        ]
      };
    });
  },

  // Squad Actions
  addSquad: (squadData) => {
    const id = 'squad-' + Date.now();
    const newSquad: Squad = {
      ...squadData,
      id,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString()
    };
    set((state) => {
      // Update crew members that are part of this squad
      const updatedCrew = state.crew.map((c) =>
        newSquad.crewIds.includes(c.id) ? { ...c, squadId: id, shipId: newSquad.shipId || c.shipId } : c
      );
      // Update parent ship if specified
      const updatedShips = newSquad.shipId
        ? state.ships.map((s) =>
            s.id === newSquad.shipId
              ? {
                  ...s,
                  squadIds: Array.from(new Set([...(s.squadIds || []), id])),
                  crewIds: Array.from(new Set([...s.crewIds, ...newSquad.crewIds]))
                }
              : s
          )
        : state.ships;

      const nextSquads = [...state.squads, newSquad];
      persistStateCollection('squads', nextSquads);
      persistStateCollection('crew', updatedCrew);
      persistStateCollection('ships', updatedShips);

      return {
        squads: nextSquads,
        crew: updatedCrew,
        ships: updatedShips,
        notifications: [
          {
            id: 'notif-' + Date.now(),
            title: 'Squad Formed',
            description: `${newSquad.name} commissioned with ${newSquad.crewIds.length} specialists.`,
            type: 'quest' as const,
            read: false,
            createdAt: 'Just now',
            actionLinkTab: 'squads' as const
          },
          ...state.notifications
        ]
      };
    });
    return id;
  },

  updateSquad: (id, updates) => {
    set((state) => {
      // If crewIds updated, sync with crew squadId
      let updatedCrew = state.crew;
      if (updates.crewIds) {
        updatedCrew = state.crew.map((c) => {
          if (updates.crewIds?.includes(c.id)) {
            return { ...c, squadId: id };
          }
          if (c.squadId === id && !updates.crewIds?.includes(c.id)) {
            return { ...c, squadId: undefined };
          }
          return c;
        });
      }

      const nextSquads = state.squads.map((sq) =>
        sq.id === id ? { ...sq, ...updates, updatedAt: new Date().toISOString() } : sq
      );
      persistStateCollection('squads', nextSquads);
      persistStateCollection('crew', updatedCrew);

      return {
        squads: nextSquads,
        crew: updatedCrew,
        notifications: [
          {
            id: 'notif-' + Date.now(),
            title: 'Squad Updated',
            description: `Updated directives for ${state.squads.find((s) => s.id === id)?.name || 'Squad'}.`,
            type: 'quest' as const,
            read: false,
            createdAt: 'Just now',
            actionLinkTab: 'squads' as const
          },
          ...state.notifications
        ]
      };
    });
  },

  deleteSquad: (id) => {
    set((state) => {
      const nextSquads = state.squads.filter((sq) => sq.id !== id);
      const nextCrew = state.crew.map((c) => (c.squadId === id ? { ...c, squadId: undefined } : c));
      const nextShips = state.ships.map((s) => ({
        ...s,
        squadIds: (s.squadIds || []).filter((sqId) => sqId !== id)
      }));
      persistStateCollection('squads', nextSquads);
      persistStateCollection('crew', nextCrew);
      persistStateCollection('ships', nextShips);
      return {
        squads: nextSquads,
        crew: nextCrew,
        ships: nextShips
      };
    });
  },

  // Training Officer Actions
  addTrainingSkill: (skillData) => {
    const id = 'skill-' + Date.now();
    const newSkill: TrainingSkill = {
      ...skillData,
      id,
      version: 1,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString()
    };
    set((state) => {
      const nextSkills = [...state.trainingSkills, newSkill];
      persistStateCollection('trainingSkills', nextSkills);
      return { trainingSkills: nextSkills };
    });
    return id;
  },

  updateTrainingSkill: (id, updates) => {
    set((state) => {
      const nextSkills = state.trainingSkills.map((sk) =>
        sk.id === id ? { ...sk, ...updates, version: sk.version + 1, updatedAt: new Date().toISOString() } : sk
      );
      persistStateCollection('trainingSkills', nextSkills);
      return { trainingSkills: nextSkills };
    });
  },

  deleteTrainingSkill: (id) => {
    set((state) => {
      const nextSkills = state.trainingSkills.filter((sk) => sk.id !== id);
      persistStateCollection('trainingSkills', nextSkills);
      return { trainingSkills: nextSkills };
    });
  },

  addGlobalSteering: (orderData) => {
    const id = 'gs-' + Date.now();
    const newOrder: GlobalSteering = {
      ...orderData,
      id,
      version: 1,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString()
    };
    set((state) => {
      const nextOrders = [...state.globalSteering, newOrder];
      persistStateCollection('globalSteering', nextOrders);
      return { globalSteering: nextOrders };
    });
    return id;
  },

  updateGlobalSteering: (id, updates) => {
    set((state) => {
      const nextOrders = state.globalSteering.map((gs) =>
        gs.id === id ? { ...gs, ...updates, version: gs.version + 1, updatedAt: new Date().toISOString() } : gs
      );
      persistStateCollection('globalSteering', nextOrders);
      return { globalSteering: nextOrders };
    });
  },

  deleteGlobalSteering: (id) => {
    set((state) => {
      const nextOrders = state.globalSteering.filter((gs) => gs.id !== id);
      persistStateCollection('globalSteering', nextOrders);
      return { globalSteering: nextOrders };
    });
  },

  addSteeringDirective: (dirData) => {
    const id = 'sd-' + Date.now();
    const newDir: SteeringDirective = {
      ...dirData,
      id,
      version: 1,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString()
    };
    set((state) => {
      const nextDirs = [...state.steeringDirectives, newDir];
      persistStateCollection('steeringDirectives', nextDirs);
      return { steeringDirectives: nextDirs };
    });
    return id;
  },

  updateSteeringDirective: (id, updates) => {
    set((state) => {
      const nextDirs = state.steeringDirectives.map((sd) =>
        sd.id === id ? { ...sd, ...updates, version: sd.version + 1, updatedAt: new Date().toISOString() } : sd
      );
      persistStateCollection('steeringDirectives', nextDirs);
      return { steeringDirectives: nextDirs };
    });
  },

  deleteSteeringDirective: (id) => {
    set((state) => {
      const nextDirs = state.steeringDirectives.filter((sd) => sd.id !== id);
      persistStateCollection('steeringDirectives', nextDirs);
      return { steeringDirectives: nextDirs };
    });
  },

  addTrainingHook: (hookData) => {
    const id = 'hook-' + Date.now();
    const newHook: TrainingHook = {
      ...hookData,
      id,
      version: 1,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString()
    };
    set((state) => {
      const nextHooks = [...state.trainingHooks, newHook];
      persistStateCollection('trainingHooks', nextHooks);
      return { trainingHooks: nextHooks };
    });
    return id;
  },

  updateTrainingHook: (id, updates) => {
    set((state) => {
      const nextHooks = state.trainingHooks.map((h) =>
        h.id === id ? { ...h, ...updates, version: h.version + 1, updatedAt: new Date().toISOString() } : h
      );
      persistStateCollection('trainingHooks', nextHooks);
      return { trainingHooks: nextHooks };
    });
  },

  deleteTrainingHook: (id) => {
    set((state) => {
      const nextHooks = state.trainingHooks.filter((h) => h.id !== id);
      persistStateCollection('trainingHooks', nextHooks);
      return { trainingHooks: nextHooks };
    });
  },

  createShip: (shipData) => {
    const id = 'ship-' + Date.now();
    const newShip: Ship = {
      id,
      name: shipData.name || 'New Specialist Vessel',
      fleetId: 'fleet-diezy',
      tagline: shipData.tagline || 'Persistent department container for autonomous fleet missions.',
      homeScope: shipData.homeScope || 'engineering',
      navigatorName: shipData.navigatorName || 'Orion Navigator',
      status: 'active',
      activeVoyagesCount: 0,
      monthlySpentUSD: 0,
      squadIds: shipData.squadIds || [],
      crewIds: shipData.crewIds || [],
      charter: shipData.charter || {
        purpose: shipData.tagline || 'Autonomous mission execution under fleet governance policy.',
        acceptedQuestTypes: ['repository_health', 'ci_triage', 'feature_delivery', 'release_readiness'],
        crewAuthority: 'Autonomous reading and staging. Impactful external writes require Captain’s Approval.',
        prohibitedActions: ['Direct production deploys without Captain sign-off'],
        budgetPerVoyageUSD: 2.00,
        monthlyBudgetUSD: 30.00,
        memorySharing: 'ship_scoped'
      }
    };

    set((state) => {
      const nextShips = [...state.ships, newShip];
      const nextLogs = [
        {
          id: 'log-' + Date.now(),
          timestamp: 'Just now',
          actorType: 'owner' as const,
          actorName: 'Captain',
          action: `Commissioned Vessel: ${newShip.name}`,
          entityType: 'ship' as const,
          entityId: newShip.id,
          correlationId: 'cid-' + Date.now(),
          severity: 'info' as const
        },
        ...state.logbook
      ];
      const nextNotifs = [
        {
          id: 'notif-' + Date.now(),
          title: 'Ship Commissioned',
          description: `${newShip.name} successfully commissioned into Fleet AI.`,
          type: 'quest' as const,
          read: false,
          createdAt: 'Just now',
          actionLinkTab: 'ships' as const
        },
        ...state.notifications
      ];
      persistStateCollection('ships', nextShips);
      persistStateCollection('logbook', nextLogs);
      persistStateCollection('notifications', nextNotifs);
      return {
        ships: nextShips,
        logbook: nextLogs,
        notifications: nextNotifs
      };
    });
    return id;
  },

  markNotificationRead: (id) => {
    set((state) => {
      const nextNotifs = state.notifications.map((n) => (n.id === id ? { ...n, read: true } : n));
      persistStateCollection('notifications', nextNotifs);
      return { notifications: nextNotifs };
    });
  },

  markAllNotificationsRead: () => {
    set((state) => {
      const nextNotifs = state.notifications.map((n) => ({ ...n, read: true }));
      persistStateCollection('notifications', nextNotifs);
      return { notifications: nextNotifs };
    });
  },

  addNotification: (notifData) => {
    const newNotif: NotificationItem = {
      ...notifData,
      id: 'notif-' + Date.now(),
      createdAt: 'Just now',
      read: false
    };

    set((state) => {
      const nextNotifs = [newNotif, ...state.notifications];
      persistStateCollection('notifications', nextNotifs);
      return { notifications: nextNotifs };
    });
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

      const nextNotifs = [
        {
          id: 'notif-' + Date.now(),
          title: 'Voyage Completed Map Steps',
          description: 'A Voyage reached 100% and produced a reviewable Artifact.',
          type: 'artifact' as const,
          read: false,
          createdAt: 'Just now',
          actionLinkTab: 'artifacts' as const
        },
        ...state.notifications
      ];
      persistStateCollection('quests', quests);
      persistStateCollection('notifications', nextNotifs);

      return {
        quests,
        notifications: nextNotifs
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

    set((state) => {
      const nextSessions = [newSession, ...state.journalSessions];
      persistStateCollection('journalSessions', nextSessions);
      return {
        journalSessions: nextSessions,
        selectedJournalSessionId: id
      };
    });
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

    set((state) => {
      const nextSessions = state.journalSessions.map((s) =>
        s.id === sessionId
          ? {
              ...s,
              lastNote: content.slice(0, 80) + '...',
              updatedAt: 'Just now',
              messages: [...s.messages, userMsg]
            }
          : s
      );
      persistStateCollection('journalSessions', nextSessions);
      return { journalSessions: nextSessions };
    });

    // Real Quartermaster AI Reflection inside private journal
    apiClient.chatQuartermaster(content).then((aiResponse) => {
      const qmMsg: ChatMessage = {
        id: 'jqm-' + Date.now(),
        sender: 'quartermaster',
        content: aiResponse.reply,
        timestamp: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
        suggestedActions: (aiResponse.suggestedActions || []).map((a: any) => ({
          label: a.label,
          actionType: a.actionType as any,
          payload: a.payload
        })),
        generatedArtifactPreview: aiResponse.generatedArtifactPreview
      };

      set((state) => {
        const nextSessions = state.journalSessions.map((s) =>
          s.id === sessionId ? { ...s, messages: [...s.messages, qmMsg] } : s
        );
        persistStateCollection('journalSessions', nextSessions);
        return { journalSessions: nextSessions };
      });
    }).catch((err) => {
      console.warn('[Journal] Quartermaster API fallback:', err);
    });
  },

  togglePinJournalSession: (id) => {
    set((state) => {
      const nextSessions = state.journalSessions.map((s) =>
        s.id === id ? { ...s, isPinned: !s.isPinned } : s
      );
      persistStateCollection('journalSessions', nextSessions);
      return { journalSessions: nextSessions };
    });
  },

  archiveJournalSession: (id) => {
    set((state) => {
      const nextSessions = state.journalSessions.map((s) =>
        s.id === id ? { ...s, isArchived: !s.isArchived } : s
      );
      persistStateCollection('journalSessions', nextSessions);
      return { journalSessions: nextSessions };
    });
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

    set((state) => {
      const nextSessions = state.journalSessions.map((s) =>
        s.id === sessionId ? { ...s, savedArtifactCount: s.savedArtifactCount + 1 } : s
      );
      persistStateCollection('journalSessions', nextSessions);
      return {
        journalSessions: nextSessions,
        activeTab: 'artifacts'
      };
    });
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

    set((state) => {
      const nextSessions = state.journalSessions.map((s) =>
        s.id === sessionId ? { ...s, questDraftCount: s.questDraftCount + 1 } : s
      );
      persistStateCollection('journalSessions', nextSessions);
      return {
        journalSessions: nextSessions,
        activeTab: 'quests'
      };
    });
  }
}));

// Initialize document data-color-tone on boot
if (typeof document !== 'undefined') {
  const initialTone = (localStorage.getItem('galleon_color_tone') as ColorTone) || 'teal';
  document.documentElement.setAttribute('data-color-tone', initialTone);
}

