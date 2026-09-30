import React, { useState } from 'react';
import {
  Layers,
  Ship,
  Users,
  Compass,
  Check,
  ShieldCheck,
  Sparkles,
  ArrowRight,
  Info,
  X
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';

export const ShipyardView: React.FC = () => {
  const { ships, crew } = useFleetStore();
  const [showNotice, setShowNotice] = useState(false);

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto p-4 sm:p-6 space-y-6 max-w-5xl mx-auto w-full animate-view-fade-in">
      {/* Header */}
      <div>
        <div className="flex items-center gap-2">
          <h1 className="text-xl sm:text-2xl font-bold tracking-tight text-neutral-900 dark:text-neutral-100">
            Shipyard &amp; Capacity
          </h1>
          <span className="text-xs font-mono text-teal-600 dark:text-teal-400 px-2 py-0.5 rounded bg-teal-500/10">
            Timber Metaphor · Self-Hosted
          </span>
        </div>
        <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
          Manage your Fleet capacity, berths, and specialized Squad Charters. We charge for organizational scaling—never for model inference credits.
        </p>
      </div>

      {showNotice && (
        <div className="p-3.5 rounded-xl border border-teal-500/30 bg-teal-500/10 text-xs text-teal-800 dark:text-teal-200 flex items-center justify-between gap-3 animate-in fade-in duration-150">
          <div className="flex items-center gap-2">
            <Info className="w-4 h-4 text-teal-600 dark:text-teal-400 shrink-0" />
            <span>Shipyard Notice: You are operating on the self-hosted Community edition. Pro fleet capacity expansion blueprints are ready on request.</span>
          </div>
          <button
            onClick={() => setShowNotice(false)}
            className="p-1 rounded-md text-teal-600 hover:text-teal-800 dark:text-teal-400 dark:hover:text-teal-200 shrink-0 cursor-pointer"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        </div>
      )}

      {/* Timber Capacity Meter */}
      <div className="p-6 rounded-2xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] shadow-xs space-y-4">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2">
          <div>
            <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
              Active Fleet Timber Capacity
            </span>
            <div className="text-xl sm:text-2xl font-bold text-neutral-900 dark:text-neutral-100 mt-1">
              Community Tier (1 Active Ship / 6 Crew Berths)
            </div>
          </div>
          <span className="text-xs font-mono text-teal-600 dark:text-teal-400 px-2.5 py-1 rounded-full bg-teal-500/10 font-bold self-start sm:self-auto">
            FREE TO SAIL FOREVER
          </span>
        </div>

        <div className="grid grid-cols-1 sm:grid-cols-3 gap-4 pt-2 font-mono text-xs">
          <div className="p-3 rounded-lg bg-neutral-50 dark:bg-neutral-900/60 border border-neutral-200 dark:border-neutral-800">
            <span className="text-neutral-400 block text-[10px]">ACTIVE SHIPS</span>
            <span className="text-lg font-bold text-neutral-900 dark:text-neutral-100">1 of 3 Ships</span>
          </div>

          <div className="p-3 rounded-lg bg-neutral-50 dark:bg-neutral-900/60 border border-neutral-200 dark:border-neutral-800">
            <span className="text-neutral-400 block text-[10px]">CREW BERTHS</span>
            <span className="text-lg font-bold text-neutral-900 dark:text-neutral-100">{crew.length} of 15 Berths</span>
          </div>

          <div className="p-3 rounded-lg bg-neutral-50 dark:bg-neutral-900/60 border border-neutral-200 dark:border-neutral-800">
            <span className="text-neutral-400 block text-[10px]">CONCURRENT VOYAGES</span>
            <span className="text-lg font-bold text-teal-600 dark:text-teal-400">2 Parallel Runs</span>
          </div>
        </div>
      </div>

      {/* Plans Comparison */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-6 pt-2">
        {/* Community */}
        <div className="p-5 rounded-2xl border border-teal-500/40 bg-white dark:bg-[#191b1f] space-y-4 text-xs">
          <div className="flex items-center justify-between">
            <span className="font-bold text-sm text-neutral-900 dark:text-neutral-100">
              Community (First Ship)
            </span>
            <span className="font-mono text-xs font-bold text-teal-600 dark:text-teal-400">
              $0 / mo
            </span>
          </div>

          <p className="text-neutral-500 dark:text-neutral-400 leading-relaxed">
            Everything needed to operate a self-hosted AI specialist team with complete BYOK freedom and zero platform lock-in.
          </p>

          <ul className="space-y-2 text-neutral-700 dark:text-neutral-300">
            <li className="flex items-center gap-2">
              <Check className="w-4 h-4 text-teal-500 shrink-0" />
              <span>1 Active Ship with persistent memory</span>
            </li>
            <li className="flex items-center gap-2">
              <Check className="w-4 h-4 text-teal-500 shrink-0" />
              <span>Up to 5 persistent Crew Members</span>
            </li>
            <li className="flex items-center gap-2">
              <Check className="w-4 h-4 text-teal-500 shrink-0" />
              <span>Unlimited Workspaces, Projects, and Quests</span>
            </li>
            <li className="flex items-center gap-2">
              <Check className="w-4 h-4 text-teal-500 shrink-0" />
              <span>BYOK &amp; Local model endpoints (Ollama/LM Studio)</span>
            </li>
            <li className="flex items-center gap-2">
              <Check className="w-4 h-4 text-teal-500 shrink-0" />
              <span>Full Captain’s Approval external write safety</span>
            </li>
          </ul>

          <div className="pt-2">
            <span className="text-xs font-mono text-teal-600 dark:text-teal-400 font-semibold">
              ✓ Active on your Fleet
            </span>
          </div>
        </div>

        {/* Pro Fleet Capacity */}
        <div className="p-5 rounded-2xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-900/30 space-y-4 text-xs">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-1.5">
              <Sparkles className="w-4 h-4 text-amber-500" />
              <span className="font-bold text-sm text-neutral-900 dark:text-neutral-100">
                Pro Fleet Fleetmaster
              </span>
            </div>
            <span className="font-mono text-xs font-bold text-neutral-900 dark:text-neutral-100">
              Capacity Expansion
            </span>
          </div>

          <p className="text-neutral-500 dark:text-neutral-400 leading-relaxed">
            Expand your autonomous organization with additional Ships, curated Squad Charters, and cross-ship synchronization.
          </p>

          <ul className="space-y-2 text-neutral-700 dark:text-neutral-300">
            <li className="flex items-center gap-2">
              <Check className="w-4 h-4 text-emerald-500 shrink-0" />
              <span>Up to 5 active Ships simultaneously</span>
            </li>
            <li className="flex items-center gap-2">
              <Check className="w-4 h-4 text-emerald-500 shrink-0" />
              <span>25 persistent Crew Specialists</span>
            </li>
            <li className="flex items-center gap-2">
              <Check className="w-4 h-4 text-emerald-500 shrink-0" />
              <span>Curated Squad Charters (Dev, Marketing, Security)</span>
            </li>
            <li className="flex items-center gap-2">
              <Check className="w-4 h-4 text-emerald-500 shrink-0" />
              <span>Cross-Ship Quartermaster executive briefings</span>
            </li>
            <li className="flex items-center gap-2">
              <Check className="w-4 h-4 text-emerald-500 shrink-0" />
              <span>Priority update channel &amp; community support</span>
            </li>
          </ul>

          <button
            onClick={() => setShowNotice(true)}
            className="w-full py-2 rounded-lg bg-neutral-900 dark:bg-neutral-100 text-white dark:text-neutral-900 font-semibold hover:opacity-90 transition-opacity text-xs cursor-pointer shadow-xs"
          >
            Explore Squad Charters
          </button>
        </div>
      </div>
    </div>
  );
};
