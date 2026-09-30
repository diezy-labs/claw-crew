import React, { useState, useRef, useEffect } from 'react';
import {
  Compass,
  Send,
  Sparkles,
  ShieldAlert,
  FileText,
  Ship,
  ArrowRight,
  PlusCircle,
  CheckCircle2,
  AlertTriangle,
  Coins,
  BookmarkPlus,
  Play,
  Activity,
  Layers,
  ChevronDown,
  User,
  Users,
  Plus,
  Flag,
  RotateCcw,
  Sliders,
  Cpu,
  Brain,
  Zap,
  Gauge,
  CornerDownLeft,
  Check,
  Mic,
  LayoutTemplate,
  Radio
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';
import { Artifact, NavigationTab } from '../../types';
import { QuestBrainstormModal } from './QuestBrainstormModal';
import { VoiceQuartermasterModal } from './VoiceQuartermasterModal';
import { LiveCanvasPane } from './LiveCanvasPane';
import { SubMenuScroller } from '../common/SubMenuScroller';

export type AIModelOption = {
  id: string;
  name: string;
  provider: string;
  tag: string;
  recommendedFor: string;
};

export type EffortLevel = 'low' | 'medium' | 'high';

export const AI_MODELS: AIModelOption[] = [
  {
    id: 'claude-3-7-sonnet',
    name: 'Claude 3.7 Sonnet',
    provider: 'Anthropic',
    tag: 'Hybrid Reasoning',
    recommendedFor: 'Complex Fleet Strategy & Multi-file Code'
  },
  {
    id: 'gemini-2-5-pro',
    name: 'Gemini 2.5 Pro',
    provider: 'Google',
    tag: 'Deep Context (1M)',
    recommendedFor: 'Full Repository Audits & Research Synthesis'
  },
  {
    id: 'gemini-2-5-flash',
    name: 'Gemini 2.5 Flash',
    provider: 'Google',
    tag: 'Fast & Snappy',
    recommendedFor: 'Rapid Triage & Interactive Q&A'
  },
  {
    id: 'deepseek-r1-local',
    name: 'DeepSeek-R1 (Local)',
    provider: 'Ollama',
    tag: 'Zero Cost BYOM',
    recommendedFor: 'Private Offline Scans & Zero-Token Spend'
  },
  {
    id: 'claude-3-5-sonnet',
    name: 'Claude 3.5 Sonnet',
    provider: 'Anthropic',
    tag: 'Code Specialist',
    recommendedFor: 'Refactoring & AST Manipulations'
  },
  {
    id: 'gpt-4o',
    name: 'GPT-4o',
    provider: 'OpenAI',
    tag: 'Multimodal',
    recommendedFor: 'Diagram Analysis & General Routing'
  }
];

export const QuarterdeckView: React.FC = () => {
  const {
    chatMessages,
    sendQuartermasterMessage,
    setActiveTab,
    createQuest,
    saveArtifact,
    artifacts,
    approvals,
    quests,
    ships,
    selectedWorkspace,
    selectedProject,
    setSelectedArtifactId,
    handleApproval
  } = useFleetStore();

  const [input, setInput] = useState('');
  const [composerContext, setComposerContext] = useState<string | null>(null);
  const [selectedTarget, setSelectedTarget] = useState<string>('Quartermaster');
  const [isTargetMenuOpen, setIsTargetMenuOpen] = useState(false);
  const [selectedModel, setSelectedModel] = useState<AIModelOption>(AI_MODELS[0]);
  const [isModelMenuOpen, setIsModelMenuOpen] = useState(false);
  const [effortLevel, setEffortLevel] = useState<EffortLevel>('high');
  const [isEffortMenuOpen, setIsEffortMenuOpen] = useState(false);
  const [isBrainstormModalOpen, setIsBrainstormModalOpen] = useState(false);
  const [isVoiceModalOpen, setIsVoiceModalOpen] = useState(false);
  const [isCanvasOpen, setIsCanvasOpen] = useState(false);
  const [isHeaderVisible, setIsHeaderVisible] = useState(true);

  // Auto-hide Quarterdeck header in mobile mode after inactivity or when clicking/tapping
  useEffect(() => {
    let hideTimer: ReturnType<typeof setTimeout>;

    const resetTimer = () => {
      setIsHeaderVisible(true);
      clearTimeout(hideTimer);
      if (typeof window !== 'undefined' && window.innerWidth < 640) {
        hideTimer = setTimeout(() => {
          setIsHeaderVisible(false);
        }, 4000);
      }
    };

    const handleInteraction = () => {
      resetTimer();
    };

    window.addEventListener('touchstart', handleInteraction, { passive: true });
    window.addEventListener('click', handleInteraction);

    resetTimer();

    return () => {
      clearTimeout(hideTimer);
      window.removeEventListener('touchstart', handleInteraction);
      window.removeEventListener('click', handleInteraction);
    };
  }, []);

  const [showScrollToBottom, setShowScrollToBottom] = useState(false);
  const chatStreamRef = useRef<HTMLDivElement>(null);

  const handleStreamScroll = () => {
    if (!chatStreamRef.current) return;
    const { scrollTop, scrollHeight, clientHeight } = chatStreamRef.current;
    const distanceFromBottom = scrollHeight - clientHeight - scrollTop;
    setShowScrollToBottom(distanceFromBottom > 120);
  };

  const scrollToBottom = () => {
    if (!chatStreamRef.current) return;
    chatStreamRef.current.scrollTo({ top: chatStreamRef.current.scrollHeight, behavior: 'smooth' });
  };

  const messagesEndRef = useRef<HTMLDivElement>(null);
  const targetMenuRef = useRef<HTMLDivElement>(null);
  const modelMenuRef = useRef<HTMLDivElement>(null);
  const effortMenuRef = useRef<HTMLDivElement>(null);

  const pendingApprovals = approvals.filter((a) => a.status === 'pending');

  const counterparts = [
    {
      group: 'RECOMMENDED',
      items: [
        { id: 'qm', name: 'Quartermaster', role: 'Fleet Executive · strategy, orchestration, review, decisions', icon: Compass }
      ]
    },
    {
      group: 'SHIPS',
      items: [
        { id: 'ship-dev', name: 'Developer Ship', role: 'Via Navigator Horizon · engineering delivery & release readiness', icon: Ship },
        { id: 'ship-marketing', name: 'Marketing Ship', role: 'Via Navigator · campaign and growth execution', icon: Ship },
        { id: 'ship-research', name: 'Research Ship', role: 'Via Navigator · evidence and synthesis', icon: Ship }
      ]
    },
    {
      group: 'SQUADS',
      items: [
        { id: 'squad-delivery', name: 'Delivery Squad', role: 'Core product implementation & CI', icon: Users },
        { id: 'squad-ci', name: 'CI Triage Squad', role: 'Test resilience & teardown triage', icon: Users },
        { id: 'squad-content', name: 'Content Launch Squad', role: 'Documentation & product release', icon: Users }
      ]
    },
    {
      group: 'CREW MEMBERS',
      items: [
        { id: 'crew-ast', name: 'Repository Analyst', role: 'Code engine & AST refactoring', icon: User },
        { id: 'crew-qa', name: 'QA & Risk Reviewer', role: 'Evidence verification & test coverage', icon: User },
        { id: 'crew-planner', name: 'Engineering Planner', role: 'Map steps & dependency sequencing', icon: User },
        { id: 'crew-growth', name: 'Campaign Strategist', role: 'Go-to-market synthesis', icon: User }
      ]
    },
    {
      group: 'TEMPORARY',
      items: [
        { id: 'temp-delegate', name: 'Create temporary delegate', role: 'Scoped ad-hoc agent with ephemeral memory', icon: Plus }
      ]
    }
  ];

  // Auto scroll to bottom of chat
  useEffect(() => {
    if (typeof messagesEndRef.current?.scrollIntoView === 'function') {
      messagesEndRef.current.scrollIntoView({ behavior: 'smooth' });
    }
  }, [chatMessages]);

  // Click outside listener for dropdown menus
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (targetMenuRef.current && !targetMenuRef.current.contains(e.target as Node)) {
        setIsTargetMenuOpen(false);
      }
      if (modelMenuRef.current && !modelMenuRef.current.contains(e.target as Node)) {
        setIsModelMenuOpen(false);
      }
      if (effortMenuRef.current && !effortMenuRef.current.contains(e.target as Node)) {
        setIsEffortMenuOpen(false);
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  const handleSend = (e?: React.FormEvent) => {
    if (e) e.preventDefault();
    if (!input.trim()) return;

    let prefix = `[To: ${selectedTarget} | Model: ${selectedModel.name} | Effort: ${effortLevel.toUpperCase()}]`;
    if (composerContext) {
      prefix += ` [Context: ${composerContext}]`;
    }
    const finalContent = `${prefix} ${input.trim()}`;

    sendQuartermasterMessage(finalContent);
    setInput('');
    setComposerContext(null);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const handleNewChat = () => {
    sendQuartermasterMessage(`Started fresh session with ${selectedTarget}. Model: ${selectedModel.name} (${effortLevel} effort).`);
  };

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-hidden bg-neutral-50/50 dark:bg-[#111315] animate-view-fade-in relative">
      {/* Mobile discreet reveal handle if header is auto-hidden */}
      {!isHeaderVisible && (
        <button
          onClick={() => setIsHeaderVisible(true)}
          className="sm:hidden absolute top-1 left-1/2 -translate-x-1/2 z-30 px-3 py-0.5 rounded-full bg-neutral-800/80 text-[10px] text-neutral-300 shadow-md backdrop-blur-xs flex items-center gap-1 border border-neutral-700/60 cursor-pointer animate-in fade-in duration-200"
          title="Show Header"
        >
          <span>Tap to show controls</span>
        </button>
      )}

      {/* 1. TOP CONSOLE BAR WITH MOBILE CLEANUP & AUTO-HIDE */}
      <div
        className={`border-b border-neutral-200/80 dark:border-neutral-800/80 bg-white/95 dark:bg-[#16181b]/95 backdrop-blur-md px-3 sm:px-4 py-2 sm:py-2.5 flex items-center justify-between gap-2 shrink-0 z-20 transition-all duration-300 ${
          isHeaderVisible
            ? 'translate-y-0 opacity-100'
            : '-translate-y-full opacity-0 pointer-events-none sm:translate-y-0 sm:opacity-100 sm:pointer-events-auto'
        }`}
      >
        <div className="flex items-center gap-2">
          {/* Compass icon hidden on mobile */}
          <div className="hidden sm:flex w-8 h-8 rounded-lg bg-teal-500/10 text-teal-600 dark:text-teal-400 items-center justify-center font-bold shrink-0 ring-1 ring-teal-500/20">
            <Compass className="w-4 h-4 text-teal-500" />
          </div>
          {/* Subtitle & Title hidden on mobile */}
          <div className="hidden sm:block">
            <div className="text-[10px] font-mono tracking-wider uppercase text-neutral-400 dark:text-neutral-500 font-semibold leading-none">
              QUARTERDECK &mdash; QUARTERMASTER COMMAND CONSOLE
            </div>
            <div className="text-xs font-semibold text-neutral-800 dark:text-neutral-200 mt-0.5">
              AI Chat Hub
            </div>
          </div>
        </div>

        <div className="flex items-center gap-1.5 sm:gap-2 flex-wrap">
          {/* Live Canvas A2UI Toggle */}
          <button
            onClick={() => setIsCanvasOpen(!isCanvasOpen)}
            className={`text-xs font-medium flex items-center gap-1 sm:gap-1.5 px-2 sm:px-2.5 py-1 sm:py-1.5 rounded-lg border transition-all cursor-pointer ${
              isCanvasOpen
                ? 'border-teal-500 bg-teal-500/15 text-teal-600 dark:text-teal-400 font-semibold shadow-xs'
                : 'border-neutral-200 dark:border-neutral-800 text-neutral-600 dark:text-neutral-300 hover:border-teal-500/40 bg-white dark:bg-neutral-900'
            }`}
            title="Toggle Live Canvas (A2UI Interactive Components)"
          >
            <LayoutTemplate className="w-3.5 h-3.5 text-teal-500" />
            <span className="hidden sm:inline">Live Canvas</span>
          </button>

          {/* Voice Quartermaster (Duplex Audio) Button */}
          <button
            onClick={() => setIsVoiceModalOpen(true)}
            className="flex items-center gap-1 sm:gap-1.5 px-2 sm:px-2.5 py-1 sm:py-1.5 rounded-lg border border-teal-500/40 bg-teal-500/10 text-teal-600 dark:text-teal-400 text-xs font-semibold hover:bg-teal-500/20 active:scale-[0.98] transition-all cursor-pointer shadow-xs shrink-0"
            title="Launch Voice Quartermaster (Full-Duplex Audio & Silero VAD)"
          >
            <Mic className="w-3.5 h-3.5" />
            <span className="hidden sm:inline">Voice Mode</span>
          </button>

          {/* Quick Handoff to Flag Bridge */}
          <button
            onClick={() => setActiveTab('flag-bridge')}
            className="text-xs text-neutral-600 dark:text-neutral-300 hover:text-teal-600 dark:hover:text-teal-400 font-medium flex items-center gap-1 sm:gap-1.5 px-2 sm:px-2.5 py-1 sm:py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 hover:border-teal-500/40 bg-white dark:bg-neutral-900 transition-colors cursor-pointer"
          >
            <Flag className="w-3.5 h-3.5 text-teal-500" />
            <span className="hidden md:inline">Flag Bridge Control Room</span>
          </button>

          {/* New Chat Button - Responsively adapts size on mobile */}
          <button
            onClick={handleNewChat}
            className="flex items-center gap-1 sm:gap-1.5 px-2 sm:px-3 py-1 sm:py-1.5 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 text-xs font-semibold hover:opacity-90 active:scale-[0.98] transition-all cursor-pointer shadow-xs shrink-0"
          >
            <Plus className="w-3.5 h-3.5" />
            <span className="hidden sm:inline">New Chat</span>
            <span className="sm:hidden">Chat</span>
          </button>
        </div>
      </div>

      {/* Main Viewport (Split with Live Canvas when open) */}
      <div className="flex-1 flex overflow-hidden">
        {/* Chat & Floating Composer Column */}
        <div className="flex-1 flex flex-col h-full overflow-hidden relative">

          {/* 2. CHAT STREAM (CLEAN & EXPANSIVE WITH SLEEK SCROLLBAR) */}
          <div
            ref={chatStreamRef}
            onScroll={handleStreamScroll}
            className="flex-1 overflow-y-auto px-4 py-6 space-y-6 max-w-4xl mx-auto w-full relative"
          >
        {/* Session Welcome / Executive Context Banner */}
        <div className="text-center py-4 space-y-1.5 border-b border-neutral-200/60 dark:border-neutral-800/60">
          <div className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-teal-500/10 text-teal-600 dark:text-teal-400 text-[11px] font-mono font-medium">
            <span className="w-1.5 h-1.5 rounded-full bg-teal-500 animate-pulse" />
            <span>Active Session &middot; {selectedModel.name} &middot; {effortLevel.toUpperCase()} EFFORT</span>
          </div>
          <h2 className="text-lg font-bold text-neutral-900 dark:text-neutral-100">
            Good evening, Pirate King.
          </h2>
          <p className="text-xs text-neutral-500 dark:text-neutral-400 max-w-lg mx-auto">
            Direct communication channel with {selectedTarget}. Discuss strategic priorities, delegate bounded tasks to Ships, or turn execution plans into Quests.
          </p>
        </div>

        {/* Inline Executive Decision & Ship Status Cards (Clean & Integrated in Chat) */}
        <div className="grid grid-cols-1 md:grid-cols-2 gap-3.5">
          {/* Owner Decision Needed Card */}
          <div className="p-3.5 rounded-xl border border-amber-500/40 bg-white dark:bg-[#181a1e] space-y-2.5 shadow-xs">
            <div className="flex items-center justify-between">
              <span className="text-[11px] font-semibold text-amber-500 flex items-center gap-1.5 uppercase tracking-wider font-mono">
                <ShieldAlert className="w-3.5 h-3.5" />
                Owner Decision Needed
              </span>
              <span className="text-[10px] font-mono text-neutral-400">1 Pending</span>
            </div>

            <div>
              <h4 className="text-xs font-bold text-neutral-900 dark:text-neutral-100">
                Approve GitHub Issue Draft?
              </h4>
              <p className="text-[11px] text-neutral-500 dark:text-neutral-400 mt-0.5 leading-relaxed">
                Developer Ship recommends creating draft issue for recurring integration test timeout in <span className="font-mono text-neutral-700 dark:text-neutral-300">diezy-labs/claw-crew</span>.
              </p>
            </div>

            <div className="flex items-center gap-2 pt-1">
              <button
                onClick={() => setActiveTab('approvals')}
                className="px-2.5 py-1 rounded-md bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-bold text-[11px] hover:opacity-90 active:scale-[0.98] transition-all cursor-pointer"
              >
                Review &amp; Approve
              </button>
              <button
                onClick={() => setActiveTab('approvals')}
                className="px-2.5 py-1 rounded-md border border-neutral-200 dark:border-neutral-800 text-neutral-600 dark:text-neutral-400 text-[11px] font-medium hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors cursor-pointer"
              >
                View Details
              </button>
            </div>
          </div>

          {/* Developer Ship Report Card */}
          <div className="p-3.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#181a1e] space-y-2.5 shadow-xs">
            <div className="flex items-center justify-between">
              <span className="text-[11px] font-semibold text-teal-600 dark:text-teal-400 flex items-center gap-1.5 uppercase tracking-wider font-mono">
                <Ship className="w-3.5 h-3.5 text-teal-500" />
                Developer Ship Report
              </span>
              <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-teal-500/10 text-teal-600 dark:text-teal-400 font-semibold">
                NAVIGATOR ON COURSE
              </span>
            </div>

            <div>
              <div className="text-xs font-bold text-neutral-900 dark:text-neutral-100">
                Quest: Prepare Release v1.4
              </div>
              <p className="text-[11px] text-neutral-500 dark:text-neutral-400 mt-0.5">
                Discovery: CI socket timeout blocks release candidate verification until listener patch lands.
              </p>
            </div>

            <div className="flex items-center gap-2 pt-1">
              <button
                onClick={() => setActiveTab('artifacts')}
                className="px-2.5 py-1 rounded-md border border-neutral-200 dark:border-neutral-800 text-neutral-700 dark:text-neutral-300 text-[11px] font-medium hover:bg-neutral-100 dark:hover:bg-neutral-800 flex items-center gap-1 cursor-pointer"
              >
                <FileText className="w-3 h-3 text-teal-500" />
                <span>Open Artifacts</span>
              </button>
              <button
                onClick={() => setActiveTab('quests')}
                className="px-2.5 py-1 rounded-md border border-neutral-200 dark:border-neutral-800 text-neutral-700 dark:text-neutral-300 text-[11px] font-medium hover:bg-neutral-100 dark:hover:bg-neutral-800 cursor-pointer"
              >
                Open Quest Map
              </button>
            </div>
          </div>
        </div>

        {/* Message Stream */}
        <div className="space-y-4 pt-2">
          {chatMessages.map((msg) => {
            const isQM = msg.sender === 'quartermaster';
            return (
              <div
                key={msg.id}
                className={`flex gap-3 text-xs leading-relaxed ${isQM ? '' : 'justify-end'}`}
              >
                {isQM && (
                  <div className="w-8 h-8 rounded-xl bg-teal-600/10 dark:bg-teal-500/20 text-teal-600 dark:text-teal-400 flex items-center justify-center shrink-0 font-bold font-mono text-xs shadow-xs border border-teal-500/20">
                    QM
                  </div>
                )}

                <div className={`space-y-2 max-w-2xl ${isQM ? '' : 'text-right'}`}>
                  <div
                    className={`inline-block p-4 rounded-2xl text-left transition-all ${
                      isQM
                        ? 'bg-white dark:bg-[#181a1e] text-neutral-900 dark:text-neutral-100 border border-neutral-200/80 dark:border-neutral-800 shadow-xs'
                        : 'bg-teal-600 text-white dark:bg-teal-500 dark:text-neutral-950 font-medium shadow-xs'
                    }`}
                  >
                    <div className="whitespace-pre-wrap leading-relaxed text-xs sm:text-[13px]">{msg.content}</div>

                    {/* Generated Artifact Preview Card */}
                    {msg.generatedArtifactPreview && (
                      <div className="mt-3.5 p-3 rounded-xl border border-neutral-200 dark:border-neutral-700 bg-neutral-50/70 dark:bg-neutral-900/70 space-y-2">
                        <div className="flex items-center justify-between text-xs font-semibold text-neutral-800 dark:text-neutral-200">
                          <span className="flex items-center gap-1.5">
                            <FileText className="w-3.5 h-3.5 text-teal-500" />
                            {msg.generatedArtifactPreview.title}
                          </span>
                          <span className="text-[10px] text-teal-600 dark:text-teal-400 font-mono">Proposed Artifact</span>
                        </div>
                        <p className="text-[11px] text-neutral-500 dark:text-neutral-400">
                          {msg.generatedArtifactPreview.summary}
                        </p>
                        <button
                          onClick={() => {
                            if (msg.generatedArtifactPreview) {
                              saveArtifact({
                                questId: 'quest-ci-triage',
                                shipId: 'ship-dev',
                                producerCrewId: 'crew-qa-reviewer',
                                title: msg.generatedArtifactPreview.title || 'CI Remediation Brief',
                                type: 'ci-triage',
                                summary: msg.generatedArtifactPreview.summary || '',
                                content: `# ${msg.generatedArtifactPreview.title}\n\n${msg.generatedArtifactPreview.summary}`,
                                discoveries: [
                                  { id: 'd-new', type: 'risk', title: 'Teardown Timeout', detail: 'Remediated via explicit socket deadline.', evidenceSource: 'pkg/transport/' }
                                ],
                                evidenceCount: 6,
                                voyageCostUSD: 0.15,
                                status: 'needs_review'
                              });
                              setActiveTab('artifacts');
                            }
                          }}
                          className="w-full py-1.5 px-2.5 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-semibold text-xs hover:opacity-90 flex items-center justify-center gap-1 active:scale-[0.99] transition-all cursor-pointer"
                        >
                          <BookmarkPlus className="w-3.5 h-3.5" />
                          Save to Artifact Gallery
                        </button>
                      </div>
                    )}
                  </div>

                  {/* Suggested actions list */}
                  {msg.suggestedActions && msg.suggestedActions.length > 0 && (
                    <div className="flex flex-wrap gap-1.5 pt-0.5">
                      {msg.suggestedActions.map((action, idx) => (
                        <button
                          key={idx}
                          onClick={() => {
                            if (action.actionType === 'open_tab' && action.payload) {
                              setActiveTab(action.payload);
                            } else if (action.actionType === 'create_quest') {
                              createQuest({ title: action.payload?.title || 'Release v1.4 Verification' });
                              setActiveTab('quests');
                            } else if (action.actionType === 'save_artifact' && action.payload) {
                              saveArtifact({
                                questId: 'quest-ci-triage',
                                shipId: 'ship-dev',
                                producerCrewId: 'crew-qa-reviewer',
                                title: action.payload.title,
                                type: 'ci-triage',
                                summary: action.payload.summary,
                                content: `# ${action.payload.title}\n\n${action.payload.summary}`,
                                discoveries: [],
                                evidenceCount: 4,
                                voyageCostUSD: 0.20,
                                status: 'needs_review'
                              });
                              setActiveTab('artifacts');
                            }
                          }}
                          className="px-2.5 py-1 rounded-lg text-[11px] font-medium border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-neutral-700 dark:text-neutral-300 hover:border-teal-500 hover:text-teal-600 dark:hover:text-teal-300 transition-colors flex items-center gap-1.5 cursor-pointer shadow-xs active:scale-[0.98]"
                        >
                          <Radio className="w-3 h-3 text-teal-500 animate-pulse" />
                          <span>{action.label}</span>
                        </button>
                      ))}
                    </div>
                  )}

                  <div className="text-[10px] text-neutral-400 font-mono">
                    {msg.timestamp}
                  </div>
                </div>

                {!isQM && (
                  <div className="w-8 h-8 rounded-xl bg-neutral-200 dark:bg-neutral-800 text-neutral-700 dark:text-neutral-300 flex items-center justify-center shrink-0 font-bold font-mono text-xs shadow-xs border border-neutral-300 dark:border-neutral-700">
                    PK
                  </div>
                )}
              </div>
            );
          })}
          <div ref={messagesEndRef} />
        </div>

      {/* Discreet floating 'Scroll to Bottom' button when user has scrolled up */}
      {showScrollToBottom && (
        <button
          type="button"
          onClick={scrollToBottom}
          className="absolute right-4 sm:right-6 bottom-24 z-20 p-2 rounded-full bg-white dark:bg-[#181a1e] border border-neutral-200 dark:border-neutral-700 shadow-lg text-neutral-600 dark:text-neutral-300 hover:text-teal-600 dark:hover:text-teal-400 hover:border-teal-500/50 hover:scale-105 active:scale-95 transition-all cursor-pointer flex items-center justify-center animate-in fade-in zoom-in-75 duration-150"
          title="Scroll to latest messages"
          aria-label="Scroll to latest messages"
        >
          <ChevronDown className="w-4 h-4" />
        </button>
      )}

      {/* 3. STATIC FLOATING COMPOSER & CONTEXT WITH OCEAN WAVE SPARK */}
      <div className="sticky bottom-0 z-20 px-3 sm:px-4 py-2.5 sm:py-3 bg-gradient-to-t from-white via-white/95 to-transparent dark:from-[#111315] dark:via-[#111315]/95 dark:to-transparent shrink-0 backdrop-blur-xs">
        <div className="max-w-4xl mx-auto w-full space-y-2">
          {/* Quick Context Injection Pills */}
          <div className="flex items-center gap-1.5 overflow-x-auto scrollbar-none py-0.5 text-xs px-1">
            <span className="text-[10px] font-mono text-neutral-400 uppercase tracking-wider shrink-0 mr-1">Context:</span>
            <button
              type="button"
              onClick={() => setComposerContext(`Workspace: ${selectedWorkspace}`)}
              className={`px-2 py-0.5 rounded-md text-[11px] font-mono border transition-all cursor-pointer whitespace-nowrap shrink-0 ${
                composerContext?.includes('Workspace')
                  ? 'border-sky-500 bg-sky-500/10 text-sky-600 dark:text-sky-400 font-semibold'
                  : 'border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-neutral-600 dark:text-neutral-400 hover:border-neutral-300 dark:hover:border-neutral-700'
              }`}
            >
              + Workspace
            </button>
            <button
              type="button"
              onClick={() => setComposerContext(`Project: ${selectedProject}`)}
              className={`px-2 py-0.5 rounded-md text-[11px] font-mono border transition-all cursor-pointer whitespace-nowrap shrink-0 ${
                composerContext?.includes('Project')
                  ? 'border-sky-500 bg-sky-500/10 text-sky-600 dark:text-sky-400 font-semibold'
                  : 'border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-neutral-600 dark:text-neutral-400 hover:border-neutral-300 dark:hover:border-neutral-700'
              }`}
            >
              + Project
            </button>
            <button
              type="button"
              onClick={() => setComposerContext('Ship: Developer Delivery Ship')}
              className={`px-2 py-0.5 rounded-md text-[11px] font-mono border transition-all cursor-pointer whitespace-nowrap shrink-0 ${
                composerContext?.includes('Developer Ship')
                  ? 'border-sky-500 bg-sky-500/10 text-sky-600 dark:text-sky-400 font-semibold'
                  : 'border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-neutral-600 dark:text-neutral-400 hover:border-neutral-300 dark:hover:border-neutral-700'
              }`}
            >
              + Developer Ship
            </button>
            <button
              type="button"
              onClick={() => setIsBrainstormModalOpen(true)}
              className="px-2 py-0.5 rounded-md text-[11px] font-mono border border-sky-500/30 text-sky-600 dark:text-sky-400 bg-sky-500/5 hover:bg-sky-500/10 transition-colors flex items-center gap-1 cursor-pointer whitespace-nowrap shrink-0"
              title="Brainstorm & forge new Quest"
            >
              <PlusCircle className="w-3 h-3" />
              <span>Create Quest</span>
            </button>
          </div>

          {/* Attached Context Badge */}
          {composerContext && (
            <div className="flex items-center justify-between text-xs font-mono px-3 py-1 rounded-lg bg-sky-500/10 text-sky-600 dark:text-sky-400 border border-sky-500/20">
              <span className="truncate">Attached Context: {composerContext}</span>
              <button
                onClick={() => setComposerContext(null)}
                className="text-xs hover:text-neutral-900 dark:hover:text-white px-1.5 cursor-pointer"
              >
                ✕
              </button>
            </div>
          )}

          {/* Main Input Box with Ocean Wave Spark Outline (Slow, Gentle, Rhythmic Pulse) */}
          <div className="rounded-2xl border bg-white dark:bg-[#181a1e] ocean-spark-glow transition-all relative overflow-hidden group">
            {/* Luminous Ocean Wave Spark Accent Line */}
            <div className="absolute top-0 inset-x-0 h-[2px] bg-gradient-to-r from-transparent via-cyan-400 dark:via-cyan-300 to-transparent opacity-85 pointer-events-none animate-pulse" />

            <textarea
              rows={2}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder={`Ask ${selectedTarget}... (e.g. 'Audit release checklist', 'Turn this issue into a Quest', or 'Triage socket timeout')`}
              className="w-full bg-transparent px-4 py-3 text-xs sm:text-sm text-neutral-900 dark:text-neutral-100 placeholder:text-neutral-400 focus:outline-none resize-none leading-relaxed relative z-10"
            />

            {/* Bottom Toolbar inside Composer — 3 Buttons Relocated Here */}
            <div className="px-3 py-2 bg-neutral-50/80 dark:bg-neutral-900/60 border-t border-neutral-100 dark:border-neutral-800/80 flex items-center justify-between gap-2 flex-wrap rounded-b-2xl">
              {/* Left Group: Counterpart, AI Model & Effort Selectors */}
              <div className="flex items-center gap-1.5 sm:gap-2 flex-wrap">
                {/* 1. Counterpart Selector Dropdown */}
                <div className="relative" ref={targetMenuRef}>
                  <button
                    type="button"
                    onClick={() => setIsTargetMenuOpen(!isTargetMenuOpen)}
                    className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 hover:border-teal-500/50 text-xs font-medium text-neutral-900 dark:text-neutral-100 transition-colors shadow-xs cursor-pointer"
                  >
                    <span className="text-neutral-400 text-[11px] hidden sm:inline">Chatting with:</span>
                    <span className="font-semibold text-teal-600 dark:text-teal-400">{selectedTarget}</span>
                    <ChevronDown className="w-3 h-3 text-neutral-400" />
                  </button>

                  {isTargetMenuOpen && (
                    <div className="absolute bottom-full mb-2 left-0 w-80 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#181a1e] shadow-2xl z-50 p-2 space-y-2 max-h-96 overflow-y-auto animate-in fade-in duration-100">
                      {counterparts.map((grp) => (
                        <div key={grp.group} className="space-y-1">
                          <span className="text-[10px] font-semibold text-neutral-400 uppercase tracking-wider px-2">
                            {grp.group}
                          </span>
                          <div className="space-y-0.5">
                            {grp.items.map((item) => {
                              const Icon = item.icon;
                              const isChosen = selectedTarget === item.name;
                              return (
                                <button
                                  key={item.id}
                                  type="button"
                                  onClick={() => {
                                    setSelectedTarget(item.name);
                                    setIsTargetMenuOpen(false);
                                  }}
                                  className={`w-full text-left p-2 rounded-lg text-xs transition-colors flex items-start gap-2.5 cursor-pointer ${
                                    isChosen
                                      ? 'bg-teal-500/10 text-teal-700 dark:text-teal-300 font-semibold'
                                      : 'text-neutral-700 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-900'
                                  }`}
                                >
                                  <Icon className="w-4 h-4 text-teal-500 mt-0.5 shrink-0" />
                                  <div>
                                    <div className="font-semibold text-neutral-900 dark:text-neutral-100">{item.name}</div>
                                    <div className="text-[11px] text-neutral-400 leading-tight mt-0.5">{item.role}</div>
                                  </div>
                                </button>
                              );
                            })}
                          </div>
                        </div>
                      ))}
                    </div>
                  )}
                </div>

                {/* 2. AI Model Selector Dropdown */}
                <div className="relative" ref={modelMenuRef}>
                  <button
                    type="button"
                    onClick={() => setIsModelMenuOpen(!isModelMenuOpen)}
                    title="Select AI Model"
                    className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 hover:border-teal-500/50 text-xs text-neutral-800 dark:text-neutral-200 transition-colors shadow-xs cursor-pointer"
                  >
                    <Cpu className="w-3.5 h-3.5 text-teal-500" />
                    <span className="font-semibold truncate max-w-[120px] sm:max-w-none">{selectedModel.name}</span>
                    <ChevronDown className="w-3 h-3 text-neutral-400" />
                  </button>

                  {isModelMenuOpen && (
                    <div className="absolute bottom-full mb-2 left-0 w-72 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#181a1e] shadow-2xl z-50 p-2 space-y-1 animate-in fade-in duration-100">
                      <div className="px-2 py-1 text-[10px] font-semibold text-neutral-400 uppercase tracking-wider border-b border-neutral-100 dark:border-neutral-800/80 mb-1">
                        Select Provider &amp; Model
                      </div>
                      {AI_MODELS.map((mod) => (
                        <button
                          key={mod.id}
                          type="button"
                          onClick={() => {
                            setSelectedModel(mod);
                            setIsModelMenuOpen(false);
                          }}
                          className={`w-full text-left p-2 rounded-lg text-xs transition-colors flex items-start justify-between cursor-pointer ${
                            selectedModel.id === mod.id
                              ? 'bg-teal-500/10 text-teal-700 dark:text-teal-300 font-semibold'
                              : 'text-neutral-700 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-900'
                          }`}
                        >
                          <div>
                            <div className="font-semibold text-neutral-900 dark:text-neutral-100">{mod.name}</div>
                            <div className="text-[10px] text-neutral-400 mt-0.5">{mod.recommendedFor}</div>
                          </div>
                          <span className="text-[9px] font-mono px-1.5 py-0.5 rounded bg-neutral-200 dark:bg-neutral-800 text-neutral-500 shrink-0 ml-2">
                            {mod.tag}
                          </span>
                        </button>
                      ))}
                    </div>
                  )}
                </div>

                {/* 3. Effort Selector Dropdown (Low ~ High) */}
                <div className="relative" ref={effortMenuRef}>
                  <button
                    type="button"
                    onClick={() => setIsEffortMenuOpen(!isEffortMenuOpen)}
                    title="Thinking / Reasoning Effort (Low ~ High)"
                    className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 hover:border-teal-500/50 text-xs text-neutral-800 dark:text-neutral-200 transition-colors shadow-xs cursor-pointer"
                  >
                    <Brain className="w-3.5 h-3.5 text-amber-500" />
                    <span className="text-neutral-400 hidden sm:inline">Effort:</span>
                    <span className={`font-semibold capitalize ${
                      effortLevel === 'high' ? 'text-amber-500' : effortLevel === 'medium' ? 'text-teal-500' : 'text-blue-500'
                    }`}>
                      {effortLevel}
                    </span>
                    <ChevronDown className="w-3 h-3 text-neutral-400" />
                  </button>

                  {isEffortMenuOpen && (
                    <div className="absolute bottom-full mb-2 left-0 w-64 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#181a1e] shadow-2xl z-50 p-2 space-y-1 animate-in fade-in duration-100">
                      <div className="px-2 py-1 text-[10px] font-semibold text-neutral-400 uppercase tracking-wider border-b border-neutral-100 dark:border-neutral-800/80 mb-1">
                        Reasoning / Thinking Effort
                      </div>

                      <button
                        type="button"
                        onClick={() => {
                          setEffortLevel('low');
                          setIsEffortMenuOpen(false);
                        }}
                        className={`w-full text-left p-2 rounded-lg text-xs transition-colors cursor-pointer flex items-center justify-between ${
                          effortLevel === 'low'
                            ? 'bg-blue-500/10 text-blue-600 dark:text-blue-400 font-semibold'
                            : 'text-neutral-700 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-900'
                        }`}
                      >
                        <div>
                          <div className="font-semibold">Low Effort</div>
                          <div className="text-[10px] text-neutral-400">~1k tokens &middot; Fast direct triage</div>
                        </div>
                        {effortLevel === 'low' && <Check className="w-3.5 h-3.5 text-blue-500" />}
                      </button>

                      <button
                        type="button"
                        onClick={() => {
                          setEffortLevel('medium');
                          setIsEffortMenuOpen(false);
                        }}
                        className={`w-full text-left p-2 rounded-lg text-xs transition-colors cursor-pointer flex items-center justify-between ${
                          effortLevel === 'medium'
                            ? 'bg-teal-500/10 text-teal-600 dark:text-teal-400 font-semibold'
                            : 'text-neutral-700 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-900'
                        }`}
                      >
                        <div>
                          <div className="font-semibold">Medium Effort</div>
                          <div className="text-[10px] text-neutral-400">~4k tokens &middot; Balanced problem solving</div>
                        </div>
                        {effortLevel === 'medium' && <Check className="w-3.5 h-3.5 text-teal-500" />}
                      </button>

                      <button
                        type="button"
                        onClick={() => {
                          setEffortLevel('high');
                          setIsEffortMenuOpen(false);
                        }}
                        className={`w-full text-left p-2 rounded-lg text-xs transition-colors cursor-pointer flex items-center justify-between ${
                          effortLevel === 'high'
                            ? 'bg-amber-500/10 text-amber-600 dark:text-amber-400 font-semibold'
                            : 'text-neutral-700 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-900'
                        }`}
                      >
                        <div>
                          <div className="font-semibold">High Effort</div>
                          <div className="text-[10px] text-neutral-400">~16k tokens &middot; Deep verification &amp; AST audits</div>
                        </div>
                        {effortLevel === 'high' && <Check className="w-3.5 h-3.5 text-amber-500" />}
                      </button>
                    </div>
                  )}
                </div>
              </div>

              {/* Right Group: Command Send Button */}
              <div className="flex items-center gap-2">
                <span className="text-[10px] text-neutral-400 hidden sm:inline">
                  Enter ↵ to send
                </span>
                <button
                  type="button"
                  onClick={() => handleSend()}
                  disabled={!input.trim()}
                  className="px-3.5 py-1.5 rounded-xl bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-bold text-xs hover:opacity-90 disabled:opacity-30 transition-all flex items-center gap-1.5 cursor-pointer shadow-xs active:scale-[0.98]"
                >
                  <Send className="w-3 h-3" />
                  <span>Command</span>
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

      </div>

      {/* Live Canvas Modal / Popup — Click outside or click button again to hide, NO close X button */}
      {isCanvasOpen && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center p-3 sm:p-6 bg-black/60 backdrop-blur-xs animate-in fade-in duration-150"
          onClick={() => setIsCanvasOpen(false)}
        >
          <div
            className="w-full max-w-4xl h-[85vh] max-h-[820px] rounded-2xl overflow-hidden shadow-2xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#15171a] animate-in zoom-in-95 duration-150"
            onClick={(e) => e.stopPropagation()}
          >
            <LiveCanvasPane />
          </div>
        </div>
      )}

      {/* Voice Quartermaster Modal (Duplex Audio / Silero VAD) */}
      <VoiceQuartermasterModal
        isOpen={isVoiceModalOpen}
        onClose={() => setIsVoiceModalOpen(false)}
        onSendTranscript={(text) => sendQuartermasterMessage(text)}
      />

      {/* Quest Brainstorming Studio Modal */}
      <QuestBrainstormModal
        isOpen={isBrainstormModalOpen}
        onClose={() => setIsBrainstormModalOpen(false)}
        onSendToChat={(brief) => {
          setInput(brief);
        }}
      />
    </div>
  </div>
  );
};
