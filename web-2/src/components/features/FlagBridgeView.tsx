import React, { useState } from 'react';
import {
  Compass,
  MessageSquare,
  ShieldAlert,
  Ship,
  Sparkles,
  ArrowRight,
  CheckCircle2,
  AlertTriangle,
  Coins,
  Activity,
  FileText,
  Clock,
  Layers,
  Radio,
  RefreshCw,
  ExternalLink,
  ChevronRight,
  TrendingUp,
  Cpu,
  ShieldCheck,
  Check,
  Sliders,
  Zap
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';
import { NavigationTab } from '../../types';
import { SubMenuScroller } from '../common/SubMenuScroller';

export type FlagBridgeTab =
  | 'overview'
  | 'briefings'
  | 'ship-reports'
  | 'decisions'
  | 'treasury'
  | 'health'
  | 'strategy';

export const FlagBridgeView: React.FC = () => {
  const {
    ships,
    quests,
    approvals,
    artifacts,
    treasuryLedger,
    handleApproval,
    setActiveTab,
    setSelectedArtifactId,
    sendQuartermasterMessage
  } = useFleetStore();

  const [activeTab, setActiveFlagTab] = useState<FlagBridgeTab>('overview');
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [recommendationDismissed, setRecommendationDismissed] = useState(false);

  const pendingApprovals = approvals.filter((a) => a.status === 'pending');
  const underwayQuests = quests.filter((q) => q.status === 'underway');
  const totalSpent = treasuryLedger.reduce((sum, item) => sum + item.costUSD, 0);

  const handleRefresh = () => {
    setIsRefreshing(true);
    setTimeout(() => {
      setIsRefreshing(false);
    }, 600);
  };

  const handleAskQM = (initialPrompt?: string) => {
    if (initialPrompt) {
      sendQuartermasterMessage(initialPrompt);
    }
    setActiveTab('quarterdeck');
  };

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto p-4 sm:p-6 gap-6 max-w-6xl mx-auto w-full animate-view-fade-in">
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 border-b border-neutral-200 dark:border-neutral-800 pb-5 shrink-0">
        <div className="space-y-1">
          <div className="flex items-center gap-2 text-xs font-mono text-teal-600 dark:text-teal-400">
            <Compass className="w-4 h-4 text-teal-500 animate-spin-slow shrink-0" />
            <span>FLAG BRIDGE</span>
            <span className="text-neutral-400 dark:text-neutral-600">&middot;</span>
            <span className="text-neutral-500 dark:text-neutral-400 font-sans font-medium">Quartermaster Control Room</span>
          </div>
          <h1 className="text-2xl sm:text-3xl font-bold tracking-tight text-neutral-900 dark:text-neutral-100">
            Flag Bridge
          </h1>
          <p className="text-xs sm:text-sm text-neutral-500 dark:text-neutral-400 max-w-2xl">
            See, steer, and decide across your Fleet.
          </p>
        </div>

        {/* Top Control Actions */}
        <div className="flex items-center gap-2 self-start sm:self-center shrink-0">
          <button
            onClick={() => handleAskQM('Give me an executive briefing on Fleet readiness')}
            className="flex items-center gap-1.5 px-3.5 py-2 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 text-xs font-semibold hover:opacity-90 active:scale-[0.98] transition-all cursor-pointer shadow-xs"
          >
            <MessageSquare className="w-3.5 h-3.5" />
            <span>Ask QM</span>
          </button>

          <button
            onClick={handleRefresh}
            disabled={isRefreshing}
            className="p-2 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] text-neutral-600 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors cursor-pointer"
            title="Refresh Fleet signals"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${isRefreshing ? 'animate-spin' : ''}`} />
          </button>
        </div>
      </div>

      {/* Tabs Navigation Sub-header with < and > arrows */}
      <div className="pb-3 pt-1 border-b border-neutral-200 dark:border-neutral-800 shrink-0">
        <SubMenuScroller className="gap-1.5" containerClassName="w-full">
          {[
            { id: 'overview', label: 'Overview' },
            { id: 'briefings', label: 'Briefings' },
            { id: 'ship-reports', label: 'Ship Reports' },
            { id: 'decisions', label: 'Decisions', badge: pendingApprovals.length },
            { id: 'treasury', label: 'Treasury' },
            { id: 'health', label: 'Health' },
            { id: 'strategy', label: 'Strategy' }
          ].map((tab) => {
            const isActive = activeTab === tab.id;
            return (
              <button
                key={tab.id}
                onClick={() => setActiveFlagTab(tab.id as FlagBridgeTab)}
                className={`px-3 py-1.5 rounded-lg transition-all whitespace-nowrap flex items-center gap-1.5 cursor-pointer shrink-0 ${
                  isActive
                    ? 'bg-neutral-200/90 dark:bg-neutral-800 text-neutral-900 dark:text-neutral-100 font-semibold shadow-2xs border border-neutral-300 dark:border-neutral-700'
                    : 'text-neutral-600 dark:text-neutral-400 hover:text-neutral-900 dark:hover:text-white hover:bg-neutral-100 dark:hover:bg-neutral-900/60 border border-transparent'
                }`}
              >
                <span>{tab.label}</span>
                {tab.badge !== undefined && tab.badge > 0 && (
                  <span className="w-1.5 h-1.5 rounded-full bg-amber-500 animate-pulse" />
                )}
              </button>
            );
          })}
        </SubMenuScroller>
      </div>

      {/* Overview Tab Content */}
      {activeTab === 'overview' && (
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
          {/* LEFT COLUMN */}
          <div className="flex flex-col gap-5">
            {/* 1. Executive Briefing */}
            <div className="p-4 sm:p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-3.5 shadow-xs">
              <div className="flex items-center justify-between">
                <div>
                  <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100">
                    Executive Briefing
                  </h3>
                  <p className="text-xs text-amber-600 dark:text-amber-400 font-medium mt-0.5">
                    3 items need attention
                  </p>
                </div>
                <span className="text-[10px] font-mono text-neutral-400">
                  Synthesized 4m ago
                </span>
              </div>

              <div className="space-y-2 text-xs text-neutral-700 dark:text-neutral-300">
                <div className="flex items-start gap-2.5 p-2 rounded-lg bg-neutral-50 dark:bg-neutral-900/50 border border-neutral-100 dark:border-neutral-800/80">
                  <span className="font-mono text-amber-500 font-bold shrink-0">1.</span>
                  <div>
                    <span className="font-semibold text-neutral-900 dark:text-neutral-100">Release blocker requires choice</span>
                    <p className="text-[11px] text-neutral-500 dark:text-neutral-400 mt-0.5">
                      CI teardown socket timeout requires patch review or skip approval before tag cut.
                    </p>
                  </div>
                </div>

                <div className="flex items-start gap-2.5 p-2 rounded-lg bg-neutral-50 dark:bg-neutral-900/50 border border-neutral-100 dark:border-neutral-800/80">
                  <span className="font-mono text-teal-500 font-bold shrink-0">2.</span>
                  <div>
                    <span className="font-semibold text-neutral-900 dark:text-neutral-100">3 Artifacts ready for review</span>
                    <p className="text-[11px] text-neutral-500 dark:text-neutral-400 mt-0.5">
                      Release readiness package, CI triage report, and security boundary audit await sign-off.
                    </p>
                  </div>
                </div>

                <div className="flex items-start gap-2.5 p-2 rounded-lg bg-neutral-50 dark:bg-neutral-900/50 border border-neutral-100 dark:border-neutral-800/80">
                  <span className="font-mono text-blue-500 font-bold shrink-0">3.</span>
                  <div>
                    <span className="font-semibold text-neutral-900 dark:text-neutral-100">Credential expires soon</span>
                    <p className="text-[11px] text-neutral-500 dark:text-neutral-400 mt-0.5">
                      Harbor webhook signing secret rotates in 48 hours. Zero downtime migration ready.
                    </p>
                  </div>
                </div>
              </div>

              <div className="flex items-center gap-2 pt-1 border-t border-neutral-100 dark:border-neutral-800/80">
                <button
                  onClick={() => setActiveFlagTab('decisions')}
                  className="px-3 py-1.5 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-bold text-xs hover:opacity-90 transition-all cursor-pointer"
                >
                  Review decisions
                </button>
                <button
                  onClick={() => setActiveTab('approvals')}
                  className="px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 text-neutral-700 dark:text-neutral-300 text-xs font-medium hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors cursor-pointer"
                >
                  Open approval
                </button>
              </div>
            </div>

            {/* 2. Active Voyages */}
            <div className="p-4 sm:p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-3.5 shadow-xs">
              <div className="flex items-center justify-between">
                <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100 flex items-center gap-1.5">
                  <Ship className="w-4 h-4 text-teal-500" />
                  <span>Active Voyages</span>
                </h3>
                <button
                  onClick={() => setActiveTab('mission-board')}
                  className="text-xs text-teal-600 dark:text-teal-400 hover:underline flex items-center gap-1 font-medium cursor-pointer"
                >
                  Open Mission Board →
                </button>
              </div>

              <div className="space-y-3">
                <div className="space-y-1">
                  <div className="flex items-center justify-between text-xs">
                    <span className="font-semibold text-neutral-800 dark:text-neutral-200">CI Triage</span>
                    <span className="font-mono text-teal-600 dark:text-teal-400 font-bold">72%</span>
                  </div>
                  <div className="w-full bg-neutral-200 dark:bg-neutral-800 h-1.5 rounded-full overflow-hidden">
                    <div className="bg-teal-500 h-full rounded-full transition-all duration-300" style={{ width: '72%' }} />
                  </div>
                  <div className="text-[10px] text-neutral-400">Assigned: Developer Delivery Ship &middot; QA Reviewer</div>
                </div>

                <div className="space-y-1">
                  <div className="flex items-center justify-between text-xs">
                    <span className="font-semibold text-neutral-800 dark:text-neutral-200">Release Readiness</span>
                    <span className="font-mono text-amber-500 font-bold">Awaiting owner</span>
                  </div>
                  <div className="w-full bg-neutral-200 dark:bg-neutral-800 h-1.5 rounded-full overflow-hidden">
                    <div className="bg-amber-500 h-full rounded-full transition-all duration-300" style={{ width: '85%' }} />
                  </div>
                  <div className="text-[10px] text-neutral-400">Assigned: Developer Delivery Ship &middot; Horizon Navigator</div>
                </div>

                <div className="space-y-1">
                  <div className="flex items-center justify-between text-xs">
                    <span className="font-semibold text-neutral-800 dark:text-neutral-200">Campaign Discovery</span>
                    <span className="font-mono text-teal-600 dark:text-teal-400 font-bold">41%</span>
                  </div>
                  <div className="w-full bg-neutral-200 dark:bg-neutral-800 h-1.5 rounded-full overflow-hidden">
                    <div className="bg-teal-500 h-full rounded-full transition-all duration-300" style={{ width: '41%' }} />
                  </div>
                  <div className="text-[10px] text-neutral-400">Assigned: Marketing Ship &middot; Growth Specialist</div>
                </div>
              </div>
            </div>

            {/* 3. Ship Reports Summary */}
            <div className="p-4 sm:p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-3.5 shadow-xs">
              <div className="flex items-center justify-between">
                <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100">
                  Ship Reports
                </h3>
                <button
                  onClick={() => setActiveFlagTab('ship-reports')}
                  className="text-xs text-teal-600 dark:text-teal-400 hover:underline flex items-center gap-1 font-medium cursor-pointer"
                >
                  View reports →
                </button>
              </div>

              <div className="space-y-2">
                <div className="flex items-center justify-between p-2.5 rounded-lg border border-neutral-100 dark:border-neutral-800/80 bg-neutral-50/50 dark:bg-neutral-900/30 text-xs">
                  <div className="flex items-center gap-2">
                    <div className="w-2 h-2 rounded-full bg-amber-500" />
                    <span className="font-semibold text-neutral-900 dark:text-neutral-100">Developer Ship</span>
                  </div>
                  <span className="font-mono text-amber-500 text-[11px] font-bold">Attention</span>
                </div>

                <div className="flex items-center justify-between p-2.5 rounded-lg border border-neutral-100 dark:border-neutral-800/80 bg-neutral-50/50 dark:bg-neutral-900/30 text-xs">
                  <div className="flex items-center gap-2">
                    <div className="w-2 h-2 rounded-full bg-emerald-500" />
                    <span className="font-semibold text-neutral-900 dark:text-neutral-100">Marketing Ship</span>
                  </div>
                  <span className="font-mono text-emerald-500 text-[11px] font-bold">Healthy</span>
                </div>

                <div className="flex items-center justify-between p-2.5 rounded-lg border border-neutral-100 dark:border-neutral-800/80 bg-neutral-50/50 dark:bg-neutral-900/30 text-xs">
                  <div className="flex items-center gap-2">
                    <div className="w-2 h-2 rounded-full bg-teal-500" />
                    <span className="font-semibold text-neutral-900 dark:text-neutral-100">Research Ship</span>
                  </div>
                  <span className="font-mono text-teal-500 text-[11px] font-bold">On course</span>
                </div>
              </div>
            </div>
          </div>

          {/* RIGHT COLUMN */}
          <div className="flex flex-col gap-5">
            {/* 1. Fleet Pulse */}
            <div className="p-4 sm:p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-3.5 shadow-xs">
              <div className="flex items-center justify-between">
                <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100 flex items-center gap-1.5">
                  <Activity className="w-4 h-4 text-teal-500" />
                  <span>Fleet Pulse</span>
                </h3>
                <span className="text-[10px] font-mono text-emerald-500 flex items-center gap-1">
                  <span className="w-1.5 h-1.5 rounded-full bg-emerald-500 animate-pulse" />
                  Live Sync
                </span>
              </div>

              <div className="grid grid-cols-2 gap-3 text-xs">
                <div className="p-2.5 rounded-lg bg-neutral-50 dark:bg-neutral-900/50 border border-neutral-100 dark:border-neutral-800/80">
                  <div className="text-[11px] text-neutral-400">Health</div>
                  <div className="font-semibold font-mono text-neutral-900 dark:text-neutral-100 mt-0.5">
                    2 healthy &middot; 1 attention
                  </div>
                </div>

                <div className="p-2.5 rounded-lg bg-neutral-50 dark:bg-neutral-900/50 border border-neutral-100 dark:border-neutral-800/80">
                  <div className="text-[11px] text-neutral-400">Treasury</div>
                  <div className="font-semibold font-mono text-neutral-900 dark:text-neutral-100 mt-0.5">
                    $3.84 / $8.00 today
                  </div>
                </div>

                <div className="p-2.5 rounded-lg bg-neutral-50 dark:bg-neutral-900/50 border border-neutral-100 dark:border-neutral-800/80">
                  <div className="text-[11px] text-neutral-400">Capacity</div>
                  <div className="font-semibold font-mono text-neutral-900 dark:text-neutral-100 mt-0.5">
                    8/10 active Crew seats
                  </div>
                </div>

                <div className="p-2.5 rounded-lg bg-neutral-50 dark:bg-neutral-900/50 border border-neutral-100 dark:border-neutral-800/80">
                  <div className="text-[11px] text-neutral-400">Decisions</div>
                  <div className="font-semibold font-mono text-amber-500 mt-0.5">
                    {pendingApprovals.length} require owner
                  </div>
                </div>
              </div>

              <div className="flex items-center gap-2 pt-1 border-t border-neutral-100 dark:border-neutral-800/80">
                <button
                  onClick={() => setActiveTab('crows-nest')}
                  className="px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 text-neutral-700 dark:text-neutral-300 text-xs font-medium hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors cursor-pointer"
                >
                  Open health
                </button>
                <button
                  onClick={() => setActiveTab('treasury')}
                  className="px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 text-neutral-700 dark:text-neutral-300 text-xs font-medium hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors cursor-pointer"
                >
                  Open Treasury
                </button>
              </div>
            </div>

            {/* 2. Quartermaster Recommendations */}
            <div className="p-4 sm:p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-3.5 shadow-xs">
              <div className="flex items-center justify-between">
                <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100 flex items-center gap-1.5">
                  <Sparkles className="w-4 h-4 text-amber-500" />
                  <span>Quartermaster Recommendations</span>
                </h3>
              </div>

              {!recommendationDismissed ? (
                <div className="space-y-2.5 text-xs">
                  <div className="p-3 rounded-lg border border-teal-500/20 bg-teal-50/50 dark:bg-teal-950/20 text-neutral-800 dark:text-neutral-200">
                    <div className="font-semibold text-teal-800 dark:text-teal-300">
                      Route QA support to Developer Ship
                    </div>
                    <p className="text-[11px] text-neutral-600 dark:text-neutral-400 mt-1">
                      Release candidate blocked by socket teardown. Suggest pairing QA Reviewer with Repository Analyst.
                    </p>
                  </div>

                  <div className="p-3 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-900/30 text-neutral-800 dark:text-neutral-200">
                    <div className="font-semibold text-neutral-900 dark:text-neutral-100">
                      Review release Artifact first
                    </div>
                    <p className="text-[11px] text-neutral-500 dark:text-neutral-400 mt-1">
                      Changelog and security audit completed with 6 evidence sources attached.
                    </p>
                  </div>

                  <div className="flex items-center justify-between pt-1">
                    <button
                      onClick={() => handleAskQM('Apply Quartermaster recommendations for release v1.4')}
                      className="px-3 py-1.5 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-bold text-xs hover:opacity-90 transition-all cursor-pointer"
                    >
                      Review recommendations
                    </button>
                    <button
                      onClick={() => setRecommendationDismissed(true)}
                      className="text-[11px] text-neutral-400 hover:text-neutral-600 dark:hover:text-neutral-200 transition-colors"
                    >
                      Dismiss
                    </button>
                  </div>
                </div>
              ) : (
                <div className="p-3 rounded-lg border border-neutral-200 dark:border-neutral-800 text-xs text-neutral-400 flex items-center justify-between">
                  <span>Recommendation dismissed.</span>
                  <button
                    onClick={() => setRecommendationDismissed(false)}
                    className="text-teal-600 dark:text-teal-400 font-semibold hover:underline cursor-pointer"
                  >
                    Undo
                  </button>
                </div>
              )}
            </div>

            {/* 3. Recent Outcomes */}
            <div className="p-4 sm:p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-3.5 shadow-xs">
              <div className="flex items-center justify-between">
                <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100 flex items-center gap-1.5">
                  <FileText className="w-4 h-4 text-teal-500" />
                  <span>Recent Outcomes</span>
                </h3>
                <button
                  onClick={() => setActiveTab('artifacts')}
                  className="text-xs text-teal-600 dark:text-teal-400 hover:underline flex items-center gap-1 font-medium cursor-pointer"
                >
                  Open Artifacts →
                </button>
              </div>

              <div className="space-y-2 text-xs">
                <div
                  onClick={() => {
                    setSelectedArtifactId('art-1');
                    setActiveTab('artifacts');
                  }}
                  className="p-2.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950/60 hover:border-teal-500/50 cursor-pointer transition-colors flex items-center justify-between group"
                >
                  <div>
                    <div className="font-semibold text-neutral-900 dark:text-neutral-100 group-hover:text-teal-600 dark:group-hover:text-teal-400">
                      Release notes Artifact ready
                    </div>
                    <div className="text-[11px] text-neutral-400">v1.4 Readiness Brief &middot; 4 sources</div>
                  </div>
                  <span className="text-[10px] font-mono text-emerald-500 font-semibold">Validated</span>
                </div>

                <div
                  onClick={() => {
                    setSelectedArtifactId('art-2');
                    setActiveTab('artifacts');
                  }}
                  className="p-2.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950/60 hover:border-teal-500/50 cursor-pointer transition-colors flex items-center justify-between group"
                >
                  <div>
                    <div className="font-semibold text-neutral-900 dark:text-neutral-100 group-hover:text-teal-600 dark:group-hover:text-teal-400">
                      CI root-cause report validated
                    </div>
                    <div className="text-[11px] text-neutral-400">Socket teardown fix identified</div>
                  </div>
                  <span className="text-[10px] font-mono text-amber-500 font-semibold">Treasure</span>
                </div>

                <div
                  onClick={() => {
                    setSelectedArtifactId('art-3');
                    setActiveTab('artifacts');
                  }}
                  className="p-2.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950/60 hover:border-teal-500/50 cursor-pointer transition-colors flex items-center justify-between group"
                >
                  <div>
                    <div className="font-semibold text-neutral-900 dark:text-neutral-100 group-hover:text-teal-600 dark:group-hover:text-teal-400">
                      Campaign research complete
                    </div>
                    <div className="text-[11px] text-neutral-400">Competitive intelligence matrix</div>
                  </div>
                  <span className="text-[10px] font-mono text-emerald-500 font-semibold">Review</span>
                </div>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Briefings Tab */}
      {activeTab === 'briefings' && (
        <div className="space-y-4">
          <div className="p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-3">
            <div className="flex items-center justify-between border-b border-neutral-100 dark:border-neutral-800 pb-3">
              <div>
                <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100">
                  Daily Briefing &middot; Today, 08:30 AM
                </h3>
                <p className="text-xs text-neutral-400">
                  Synthesized across 3 Ships, 4 Quests, and 8 Logbook events
                </p>
              </div>
              <button
                onClick={() => handleAskQM('Give me more details about today’s briefing')}
                className="text-xs text-teal-600 dark:text-teal-400 font-semibold hover:underline"
              >
                Discuss with QM →
              </button>
            </div>
            <div className="text-xs text-neutral-700 dark:text-neutral-300 space-y-2 leading-relaxed">
              <p>
                <strong>Developer Ship:</strong> Navigated through milestone 3 of v1.4 Release Readiness. All 29 unit tests pass. 1 active risk discovery identified around socket timeout on slow runners.
              </p>
              <p>
                <strong>Marketing Ship:</strong> Content calendar approved. Launch assets synthesized and stored in Artifact Gallery.
              </p>
              <p>
                <strong>Treasury Posture:</strong> $3.84 consumed out of $25 monthly cap. BYOK routing recommendations saved estimated $1.20 by delegating AST scans to local model endpoint.
              </p>
            </div>
          </div>
        </div>
      )}

      {/* Ship Reports Tab */}
      {activeTab === 'ship-reports' && (
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          {ships.map((ship) => (
            <div
              key={ship.id}
              className="p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-3"
            >
              <div className="flex items-center justify-between">
                <span className="text-xs font-bold text-neutral-900 dark:text-neutral-100">
                  {ship.name}
                </span>
                <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-teal-500/10 text-teal-600 dark:text-teal-400">
                  {ship.status}
                </span>
              </div>
              <p className="text-xs text-neutral-500 dark:text-neutral-400">
                {ship.tagline}
              </p>
              <div className="pt-2 border-t border-neutral-100 dark:border-neutral-800 text-[11px] text-neutral-400 space-y-1">
                <div>Navigator: <strong className="text-neutral-700 dark:text-neutral-300">{ship.navigatorName}</strong></div>
                <div>Scope: <span className="font-mono">{ship.homeScope}</span></div>
              </div>
              <button
                onClick={() => setActiveTab('ships')}
                className="w-full mt-2 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-700 text-xs font-semibold hover:border-teal-500 transition-colors"
              >
                Inspect Ship Charter
              </button>
            </div>
          ))}
        </div>
      )}

      {/* Decisions Tab */}
      {activeTab === 'decisions' && (
        <div className="space-y-4">
          {pendingApprovals.length === 0 ? (
            <div className="p-8 text-center border border-neutral-200 dark:border-neutral-800 rounded-xl bg-white dark:bg-[#191b1f]">
              <ShieldCheck className="w-8 h-8 text-emerald-500 mx-auto mb-2" />
              <div className="text-sm font-semibold">No pending Owner decisions</div>
              <div className="text-xs text-neutral-400 mt-1">All agent side-effects and sensitive actions are authorized.</div>
            </div>
          ) : (
            pendingApprovals.map((appr) => (
              <div
                key={appr.id}
                className="p-5 rounded-xl border border-amber-500/30 bg-white dark:bg-[#191b1f] space-y-3"
              >
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <ShieldAlert className="w-4 h-4 text-amber-500" />
                    <span className="text-sm font-bold text-neutral-900 dark:text-neutral-100">
                      {appr.title}
                    </span>
                  </div>
                  <span className="text-xs font-mono text-amber-500 font-semibold uppercase">
                    Action: {appr.actionType}
                  </span>
                </div>
                <p className="text-xs text-neutral-600 dark:text-neutral-400">
                  {appr.draftSummary || appr.justification}
                </p>
                <div className="p-2.5 rounded-lg bg-neutral-50 dark:bg-neutral-900/60 font-mono text-[11px] text-neutral-500">
                  Target: {appr.targetResource} &middot; Cryptographic Digest: 0x8f2a...
                </div>
                <div className="flex items-center gap-2 pt-2">
                  <button
                    onClick={() => handleApproval(appr.id, 'approved')}
                    className="px-3 py-1.5 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-bold text-xs hover:opacity-90 transition-all cursor-pointer"
                  >
                    Grant Authorization
                  </button>
                  <button
                    onClick={() => handleApproval(appr.id, 'rejected')}
                    className="px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-700 text-neutral-600 dark:text-neutral-400 text-xs font-medium hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors cursor-pointer"
                  >
                    Deny Action
                  </button>
                </div>
              </div>
            ))
          )}
        </div>
      )}

      {/* Treasury Tab */}
      {activeTab === 'treasury' && (
        <div className="p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-4">
          <div className="flex items-center justify-between">
            <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100">
              BYOK / BYOM Model Provider Runway
            </h3>
            <button
              onClick={() => setActiveTab('treasury')}
              className="text-xs text-teal-600 dark:text-teal-400 font-semibold hover:underline"
            >
              Full Ledger →
            </button>
          </div>
          <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
            <div className="p-4 rounded-lg bg-neutral-50 dark:bg-neutral-900/50 border border-neutral-100 dark:border-neutral-800">
              <div className="text-xs text-neutral-400">Total Spent</div>
              <div className="text-xl font-bold font-mono text-neutral-900 dark:text-neutral-100 mt-1">
                ${totalSpent.toFixed(2)}
              </div>
            </div>
            <div className="p-4 rounded-lg bg-neutral-50 dark:bg-neutral-900/50 border border-neutral-100 dark:border-neutral-800">
              <div className="text-xs text-neutral-400">Monthly Spending Cap</div>
              <div className="text-xl font-bold font-mono text-neutral-900 dark:text-neutral-100 mt-1">
                $25.00
              </div>
            </div>
            <div className="p-4 rounded-lg bg-neutral-50 dark:bg-neutral-900/50 border border-neutral-100 dark:border-neutral-800">
              <div className="text-xs text-neutral-400">Local Free Inference</div>
              <div className="text-xl font-bold font-mono text-emerald-500 mt-1">
                142 Tasks (Ollama)
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Health Tab */}
      {activeTab === 'health' && (
        <div className="p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-4">
          <div className="flex items-center justify-between">
            <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100">
              Fleet Runtime &amp; Sandboxing Health
            </h3>
            <button
              onClick={() => setActiveTab('crows-nest')}
              className="text-xs text-teal-600 dark:text-teal-400 font-semibold hover:underline"
            >
              Crow’s Nest Observability →
            </button>
          </div>
          <div className="space-y-2 text-xs">
            <div className="flex items-center justify-between p-2.5 rounded-lg border border-neutral-100 dark:border-neutral-800">
              <span>Go Engine Goroutine Pool (16 workers)</span>
              <span className="font-mono text-emerald-500 font-semibold">Healthy (18ms)</span>
            </div>
            <div className="flex items-center justify-between p-2.5 rounded-lg border border-neutral-100 dark:border-neutral-800">
              <span>Rust Tauri Landlock Sandbox</span>
              <span className="font-mono text-emerald-500 font-semibold">Active &middot; Enforced</span>
            </div>
            <div className="flex items-center justify-between p-2.5 rounded-lg border border-neutral-100 dark:border-neutral-800">
              <span>Claude 3.7 &amp; Gemini 2.5 Endpoints</span>
              <span className="font-mono text-emerald-500 font-semibold">Connected (0.42s)</span>
            </div>
          </div>
        </div>
      )}

      {/* Strategy Tab */}
      {activeTab === 'strategy' && (
        <div className="p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-4">
          <div className="flex items-center justify-between">
            <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100">
              Long-term Fleet Evolution &amp; Capacity
            </h3>
            <button
              onClick={() => setActiveTab('shipyard')}
              className="text-xs text-teal-600 dark:text-teal-400 font-semibold hover:underline"
            >
              Shipyard Timber →
            </button>
          </div>
          <p className="text-xs text-neutral-500 dark:text-neutral-400">
            Current Tier: Community (1 Active Ship / 6 Berths). Pro Tier unlocks 5 Ships, unlimited Berths, and Cross-Ship Navigator synchronization.
          </p>
          <div className="pt-2">
            <button
              onClick={() => setActiveTab('shipyard')}
              className="px-4 py-2 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-bold text-xs"
            >
              View Capacity &amp; Upgrades
            </button>
          </div>
        </div>
      )}
    </div>
  );
};
