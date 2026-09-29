import React from 'react';
import {
  Coins,
  ShieldAlert,
  Ship,
  Sparkles,
  ArrowUpRight,
  PlusCircle,
  Play,
  CheckCircle2,
  AlertCircle,
  X,
  Activity,
  Cpu
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';

export const RightRail: React.FC = () => {
  const {
    treasuryLedger,
    approvals,
    ships,
    quests,
    setActiveTab,
    createQuest,
    setSelectedQuestId,
    isFleetPulseOpen,
    setFleetPulseOpen
  } = useFleetStore();

  const totalSpent = treasuryLedger.reduce((acc, curr) => acc + curr.costUSD, 0);
  const monthlyCap = 25.00;
  const spendPct = Math.min(100, Math.round((totalSpent / monthlyCap) * 100));

  const pendingApprovals = approvals.filter((a) => a.status === 'pending');
  const underwayQuests = quests.filter((q) => q.status === 'underway');

  const content = (
    <div className="flex flex-col justify-between h-full space-y-5">
      <div className="space-y-5">
        {/* Title */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
              Fleet Pulse
            </span>
            <span className="flex items-center gap-1 text-[10px] text-teal-600 dark:text-teal-400 font-mono">
              <span className="w-1.5 h-1.5 rounded-full bg-teal-500 animate-pulse" />
              Live Sync
            </span>
          </div>
          {/* Close button for mobile drawer */}
          <button
            onClick={() => setFleetPulseOpen(false)}
            className="xl:hidden p-1 rounded-md text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200 hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors"
            title="Close Fleet Pulse drawer"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Treasury Widget */}
        <div className="p-3.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/80 dark:bg-neutral-900/40 space-y-2 shadow-xs">
          <div className="flex items-center justify-between text-xs font-medium">
            <span className="flex items-center gap-1.5 text-neutral-700 dark:text-neutral-300">
              <Coins className="w-3.5 h-3.5 text-amber-500" />
              Treasury (BYOK)
            </span>
            <button
              onClick={() => {
                setActiveTab('treasury');
                setFleetPulseOpen(false);
              }}
              className="text-[11px] text-teal-600 dark:text-teal-400 hover:underline flex items-center gap-0.5 cursor-pointer font-medium"
            >
              Ledger
              <ArrowUpRight className="w-3 h-3" />
            </button>
          </div>

          <div className="flex items-baseline justify-between font-mono">
            <span className="text-lg font-bold text-neutral-900 dark:text-neutral-100">
              ${totalSpent.toFixed(2)}
            </span>
            <span className="text-xs text-neutral-400 font-mono">
              / ${monthlyCap.toFixed(2)} cap
            </span>
          </div>

          <div className="w-full bg-neutral-200 dark:bg-neutral-800 h-1.5 rounded-full overflow-hidden">
            <div
              className={`h-full rounded-full transition-all duration-500 ${
                spendPct > 80 ? 'bg-amber-500' : 'bg-teal-500'
              }`}
              style={{ width: `${spendPct}%` }}
            />
          </div>

          <p className="text-[10px] text-neutral-500 dark:text-neutral-400">
            No credit lock-in. Pay your model providers directly.
          </p>
        </div>

        {/* Pending Approvals Widget */}
        <div className="p-3.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/80 dark:bg-neutral-900/40 space-y-2 shadow-xs">
          <div className="flex items-center justify-between text-xs font-medium">
            <span className="flex items-center gap-1.5 text-neutral-700 dark:text-neutral-300">
              <ShieldAlert className="w-3.5 h-3.5 text-amber-500" />
              Captain’s Approval
            </span>
            <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-amber-500/10 text-amber-500 font-semibold">
              {pendingApprovals.length} Pending
            </span>
          </div>

          {pendingApprovals.length > 0 ? (
            <div className="space-y-1.5 pt-1">
              {pendingApprovals.map((appr) => (
                <div
                  key={appr.id}
                  onClick={() => {
                    setActiveTab('approvals');
                    setFleetPulseOpen(false);
                  }}
                  className="p-2.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950/60 hover:border-teal-500/50 cursor-pointer transition-colors text-xs active:scale-[0.99]"
                >
                  <div className="font-medium text-neutral-800 dark:text-neutral-200 truncate">
                    {appr.title}
                  </div>
                  <div className="text-[10px] text-neutral-500 font-mono mt-0.5 truncate">
                    {appr.targetResource}
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <div className="text-[11px] text-neutral-400 py-1 flex items-center gap-1">
              <CheckCircle2 className="w-3.5 h-3.5 text-emerald-500" />
              All external side-effects approved.
            </div>
          )}
        </div>

        {/* Active Voyages Widget */}
        <div className="p-3.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/80 dark:bg-neutral-900/40 space-y-2 shadow-xs">
          <div className="flex items-center justify-between text-xs font-medium">
            <span className="flex items-center gap-1.5 text-neutral-700 dark:text-neutral-300">
              <Ship className="w-3.5 h-3.5 text-teal-500" />
              Active Voyages
            </span>
            <button
              onClick={() => {
                setActiveTab('mission-board');
                setFleetPulseOpen(false);
              }}
              className="text-[11px] text-teal-600 dark:text-teal-400 hover:underline flex items-center gap-0.5 cursor-pointer font-medium"
            >
              Board
              <ArrowUpRight className="w-3 h-3" />
            </button>
          </div>

          {underwayQuests.length > 0 ? (
            <div className="space-y-2 pt-1">
              {underwayQuests.map((q) => (
                <div
                  key={q.id}
                  onClick={() => {
                    setSelectedQuestId(q.id);
                    setActiveTab('mission-board');
                    setFleetPulseOpen(false);
                  }}
                  className="p-2.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950/60 hover:border-teal-500/50 cursor-pointer transition-colors text-xs space-y-1.5 active:scale-[0.99]"
                >
                  <div className="flex items-center justify-between font-medium">
                    <span className="truncate text-neutral-800 dark:text-neutral-200">{q.title}</span>
                    <span className="font-mono text-[10px] text-teal-600 dark:text-teal-400">
                      {q.activeVoyageProgress}%
                    </span>
                  </div>
                  <div className="w-full bg-neutral-200 dark:bg-neutral-800 h-1 rounded-full overflow-hidden">
                    <div
                      className="bg-teal-500 h-full rounded-full transition-all duration-300"
                      style={{ width: `${q.activeVoyageProgress}%` }}
                    />
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <div className="text-[11px] text-neutral-400 py-1">
              No active runs. All ships anchored.
            </div>
          )}
        </div>

        {/* Quick Launch Actions */}
        <div className="space-y-2 pt-1">
          <span className="text-[10px] font-semibold uppercase tracking-wider text-neutral-400">
            Quick Actions
          </span>
          <button
            onClick={() => {
              createQuest({
                title: 'New Strategic Quest',
                objective: 'Execute high-leverage objective for current project.'
              });
              setActiveTab('mission-board');
              setFleetPulseOpen(false);
            }}
            className="w-full flex items-center gap-2 px-3 py-2 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-neutral-100/60 dark:bg-neutral-900/60 text-xs font-medium text-neutral-700 dark:text-neutral-300 hover:bg-neutral-200/60 dark:hover:bg-neutral-800/80 transition-all active:scale-[0.98] cursor-pointer"
          >
            <PlusCircle className="w-3.5 h-3.5 text-teal-500" />
            <span>Launch New Quest</span>
          </button>
          <button
            onClick={() => {
              setActiveTab('crew');
              setFleetPulseOpen(false);
            }}
            className="w-full flex items-center gap-2 px-3 py-2 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-neutral-100/60 dark:bg-neutral-900/60 text-xs font-medium text-neutral-700 dark:text-neutral-300 hover:bg-neutral-200/60 dark:hover:bg-neutral-800/80 transition-all active:scale-[0.98] cursor-pointer"
          >
            <Sparkles className="w-3.5 h-3.5 text-amber-500" />
            <span>Make Me a Squad</span>
          </button>
        </div>
      </div>

      {/* Footer system status */}
      <div className="pt-4 border-t border-neutral-200 dark:border-neutral-800 text-[11px] text-neutral-500 dark:text-neutral-400">
        <div className="flex items-center justify-between">
          <span className="flex items-center gap-1.5">
            <Cpu className="w-3 h-3 text-emerald-500" />
            <span>Go Engine Gateway</span>
          </span>
          <span className="font-mono text-emerald-600 dark:text-emerald-400">ONLINE (18ms)</span>
        </div>
      </div>
    </div>
  );

  return (
    <>
      {/* Desktop Fixed Right Rail */}
      <aside className="w-72 shrink-0 border-l border-neutral-200 dark:border-neutral-800 bg-white/60 dark:bg-[#141619]/60 backdrop-blur-sm p-4 hidden xl:flex flex-col justify-between overflow-y-auto h-[calc(100vh-3.5rem)] select-none">
        {content}
      </aside>

      {/* Mobile/Tablet Slide-Out Drawer Sheet */}
      {isFleetPulseOpen && (
        <div className="fixed inset-0 z-50 xl:hidden">
          {/* Backdrop */}
          <div
            className="fixed inset-0 bg-black/60 backdrop-blur-xs transition-opacity"
            onClick={() => setFleetPulseOpen(false)}
          />

          {/* Drawer content */}
          <div className="fixed inset-y-0 right-0 max-w-xs w-full bg-white dark:bg-[#16181b] border-l border-neutral-200 dark:border-neutral-800 shadow-2xl p-4 overflow-y-auto animate-slide-in-right z-50">
            {content}
          </div>
        </div>
      )}
    </>
  );
};
