import React, { useState } from 'react';
import {
  LayoutTemplate,
  Maximize2,
  Minimize2,
  RefreshCw,
  Sliders,
  CheckCircle2,
  Sparkles,
  Layers,
  Code2,
  Eye,
  Download,
  Trash2,
  Play,
  Activity,
  ArrowRight,
  ShieldAlert
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';

interface LiveCanvasPaneProps {
  onClose?: () => void;
  isSplitView?: boolean;
  onToggleSplitView?: () => void;
}

export const LiveCanvasPane: React.FC<LiveCanvasPaneProps> = ({
  onClose,
  isSplitView = false,
  onToggleSplitView
}) => {
  const { ships, quests, artifacts, selectedWorkspace, selectedProject, setActiveTab } = useFleetStore();
  const [activeCanvasTab, setActiveCanvasTab] = useState<'architecture' | 'voyage_flow' | 'telemetry_matrix'>('architecture');
  const [interactiveParam, setInteractiveParam] = useState<number>(3);
  const [isSimulatingLiveStream, setIsSimulatingLiveStream] = useState(false);

  const handleRefreshCanvas = () => {
    setIsSimulatingLiveStream(true);
    setTimeout(() => setIsSimulatingLiveStream(false), 600);
  };

  return (
    <div className="flex flex-col h-full bg-white dark:bg-[#15171a] border-l border-neutral-200 dark:border-neutral-800 text-xs shadow-xl animate-in slide-in-from-right duration-200">
      {/* Canvas Top Bar */}
      <div className="h-11 px-3.5 border-b border-neutral-200 dark:border-neutral-800 flex items-center justify-between gap-2 shrink-0 bg-neutral-50/80 dark:bg-[#181a1e]/80">
        <div className="flex items-center gap-2">
          <div className="w-6 h-6 rounded-md bg-teal-500/10 text-teal-600 dark:text-teal-400 flex items-center justify-center font-bold">
            <LayoutTemplate className="w-3.5 h-3.5" />
          </div>
          <div>
            <div className="font-semibold text-neutral-900 dark:text-neutral-100 flex items-center gap-1.5">
              <span>Live Canvas (A2UI)</span>
              <span className="text-[9px] font-mono px-1 py-0.2 rounded bg-emerald-500/20 text-emerald-500 font-bold">
                STREAMING
              </span>
            </div>
          </div>
        </div>

        <div className="flex items-center gap-1">
          <button
            onClick={handleRefreshCanvas}
            className="p-1.5 rounded-md hover:bg-neutral-200 dark:hover:bg-neutral-800 text-neutral-500 transition-colors"
            title="Refresh Canvas Stream"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${isSimulatingLiveStream ? 'animate-spin text-teal-500' : ''}`} />
          </button>
          {onToggleSplitView && (
            <button
              onClick={onToggleSplitView}
              className="p-1.5 rounded-md hover:bg-neutral-200 dark:hover:bg-neutral-800 text-neutral-500 transition-colors"
              title={isSplitView ? 'Close Split View' : 'Expand Split View'}
            >
              {isSplitView ? <Minimize2 className="w-3.5 h-3.5" /> : <Maximize2 className="w-3.5 h-3.5" />}
            </button>
          )}
        </div>
      </div>

      {/* Sub-selector of active dynamic component */}
      <div className="px-3 py-1.5 border-b border-neutral-200 dark:border-neutral-800 flex items-center gap-1 bg-white/60 dark:bg-neutral-900/40 overflow-x-auto scrollbar-none">
        <button
          onClick={() => setActiveCanvasTab('architecture')}
          className={`px-2.5 py-1 rounded-md text-[11px] font-medium transition-colors ${
            activeCanvasTab === 'architecture'
              ? 'bg-teal-500/10 text-teal-700 dark:text-teal-300 font-semibold border border-teal-500/20'
              : 'text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200'
          }`}
        >
          Fleet Topology Map
        </button>
        <button
          onClick={() => setActiveCanvasTab('voyage_flow')}
          className={`px-2.5 py-1 rounded-md text-[11px] font-medium transition-colors ${
            activeCanvasTab === 'voyage_flow'
              ? 'bg-teal-500/10 text-teal-700 dark:text-teal-300 font-semibold border border-teal-500/20'
              : 'text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200'
          }`}
        >
          Voyage Execution Flow
        </button>
        <button
          onClick={() => setActiveCanvasTab('telemetry_matrix')}
          className={`px-2.5 py-1 rounded-md text-[11px] font-medium transition-colors ${
            activeCanvasTab === 'telemetry_matrix'
              ? 'bg-teal-500/10 text-teal-700 dark:text-teal-300 font-semibold border border-teal-500/20'
              : 'text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200'
          }`}
        >
          Agent Tool Matrix
        </button>
      </div>

      {/* Canvas Viewport Body */}
      <div className="flex-1 overflow-y-auto p-4 space-y-4 scrollbar-none">
        {activeCanvasTab === 'architecture' && (
          <div className="space-y-4">
            <div className="p-3 rounded-xl border border-teal-500/30 bg-teal-500/5 space-y-1.5">
              <div className="flex items-center justify-between">
                <span className="font-semibold text-teal-700 dark:text-teal-300">
                  Interactive Vessel Hierarchy ({ships.length} Ships)
                </span>
                <span className="text-[10px] font-mono text-neutral-400">
                  {selectedWorkspace} &middot; {selectedProject}
                </span>
              </div>
              <p className="text-[11px] text-neutral-600 dark:text-neutral-300 leading-relaxed">
                Agent-to-UI diagram rendered from current Fleet State. Click nodes to inspect Ship Charters or assign live Quests.
              </p>
            </div>

            {/* Visual Node Diagram */}
            <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-[#121316] space-y-3">
              <div className="flex items-center justify-center">
                <div className="p-2.5 rounded-xl border border-teal-500 bg-teal-500/15 text-teal-700 dark:text-teal-300 text-center font-bold shadow-md">
                  <div className="text-[10px] uppercase font-mono text-teal-500">Fleet Executive</div>
                  <div>Quartermaster Core</div>
                </div>
              </div>

              <div className="flex justify-center">
                <div className="w-0.5 h-6 bg-teal-500/40" />
              </div>

              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                {ships.map((s) => (
                  <div
                    key={s.id}
                    onClick={() => setActiveTab('ships')}
                    className="p-3 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] hover:border-teal-500/50 transition-all cursor-pointer shadow-xs space-y-1.5"
                  >
                    <div className="flex items-center justify-between">
                      <span className="font-bold text-neutral-900 dark:text-neutral-100">{s.name}</span>
                      <span className="text-[9px] font-mono px-1.5 py-0.2 rounded bg-emerald-500/20 text-emerald-500 font-semibold">
                        ACTIVE
                      </span>
                    </div>
                    <div className="text-[10px] text-neutral-500">Navigator: {s.navigatorName}</div>
                    <div className="text-[10px] font-mono text-teal-600 dark:text-teal-400">
                      {s.crewIds.length} Crew Berths &middot; ${s.monthlySpentUSD.toFixed(2)} / ${s.charter.monthlyBudgetUSD.toFixed(2)}
                    </div>
                  </div>
                ))}
              </div>
            </div>

            {/* Interactive Slider for Simulation */}
            <div className="p-3 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-2">
              <div className="flex items-center justify-between">
                <span className="font-semibold text-neutral-800 dark:text-neutral-200">
                  Dynamic Concurrency Limiter
                </span>
                <span className="font-mono text-teal-600 dark:text-teal-400 font-bold">{interactiveParam} Parallel Voyages</span>
              </div>
              <input
                type="range"
                min={1}
                max={5}
                value={interactiveParam}
                onChange={(e) => setInteractiveParam(Number(e.target.value))}
                className="w-full accent-teal-500 cursor-pointer"
              />
              <div className="flex justify-between text-[10px] text-neutral-400">
                <span>1 Voyage (Strict)</span>
                <span>3 Voyages (Balanced)</span>
                <span>5 Voyages (Max Capacity)</span>
              </div>
            </div>
          </div>
        )}

        {activeCanvasTab === 'voyage_flow' && (
          <div className="space-y-3">
            <div className="p-3 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-2">
              <div className="flex items-center justify-between">
                <span className="font-bold text-neutral-900 dark:text-neutral-100">
                  Active Voyage: CI Triage &amp; Isolation
                </span>
                <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-teal-500/20 text-teal-600 dark:text-teal-400 font-semibold">
                  68% PROGRESS
                </span>
              </div>
              <div className="w-full bg-neutral-200 dark:bg-neutral-800 h-2 rounded-full overflow-hidden">
                <div className="bg-teal-500 h-full w-[68%]" />
              </div>
            </div>

            {/* Step Timeline */}
            <div className="space-y-2">
              {[
                { step: 1, title: 'Analyze GitHub Action runner logs', status: 'completed', duration: '1.2s' },
                { step: 2, title: 'Trace socket handle leak in ws.rs', status: 'completed', duration: '4.8s' },
                { step: 3, title: 'Synthesize regression remediation brief', status: 'in_progress', duration: 'Running...' },
                { step: 4, title: 'Draft Captain Approval for GitHub issue', status: 'pending', duration: 'Queued' }
              ].map((st) => (
                <div
                  key={st.step}
                  className={`p-2.5 rounded-lg border flex items-center justify-between ${
                    st.status === 'completed'
                      ? 'border-emerald-500/30 bg-emerald-500/5'
                      : st.status === 'in_progress'
                      ? 'border-teal-500 bg-teal-500/10 ring-1 ring-teal-500/40'
                      : 'border-neutral-200 dark:border-neutral-800 opacity-60'
                  }`}
                >
                  <div className="flex items-center gap-2">
                    <span className="w-5 h-5 rounded-full bg-neutral-200 dark:bg-neutral-800 flex items-center justify-center font-mono font-bold text-[10px]">
                      {st.step}
                    </span>
                    <span className="font-medium text-neutral-800 dark:text-neutral-200">{st.title}</span>
                  </div>
                  <span className="text-[10px] font-mono text-neutral-400">{st.duration}</span>
                </div>
              ))}
            </div>
          </div>
        )}

        {activeCanvasTab === 'telemetry_matrix' && (
          <div className="space-y-3">
            <span className="font-semibold text-neutral-700 dark:text-neutral-300 block">
              Active Tool Permissions &amp; Sandboxing Limits
            </span>
            <div className="rounded-xl border border-neutral-200 dark:border-neutral-800 overflow-hidden divide-y divide-neutral-100 dark:divide-neutral-800">
              {[
                { tool: 'local_file_read', scope: 'Repository root', state: 'Active', tier: 'Tier 1' },
                { tool: 'deterministic_test_runner', scope: 'Cargo test suite', state: 'Active', tier: 'Tier 2' },
                { tool: 'github_issue_creator', scope: 'Drafts only', state: 'Gated', tier: 'Tier 4' },
                { tool: 'production_deployer', scope: 'Kubernetes API', state: 'Denied', tier: 'Tier 5' }
              ].map((t) => (
                <div key={t.tool} className="p-2.5 flex items-center justify-between text-[11px]">
                  <div>
                    <span className="font-mono font-bold text-neutral-900 dark:text-neutral-100">{t.tool}</span>
                    <span className="text-neutral-400 block text-[10px]">{t.scope}</span>
                  </div>
                  <div className="flex items-center gap-1.5 font-mono">
                    <span className="text-neutral-400">{t.tier}</span>
                    <span className={`px-1.5 py-0.2 rounded font-semibold text-[9px] ${
                      t.state === 'Active' ? 'bg-emerald-500/20 text-emerald-500' : t.state === 'Gated' ? 'bg-amber-500/20 text-amber-500' : 'bg-rose-500/20 text-rose-500'
                    }`}>
                      {t.state}
                    </span>
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}
      </div>

      {/* Canvas Footer */}
      <div className="p-2.5 border-t border-neutral-200 dark:border-neutral-800 bg-neutral-50/80 dark:bg-[#181a1e]/80 flex items-center justify-between text-[10px] text-neutral-500">
        <span className="font-mono">A2UI Protocol v1.4 &middot; Channel 8080</span>
        <button
          onClick={() => setActiveTab('artifacts')}
          className="text-teal-600 dark:text-teal-400 hover:underline font-semibold cursor-pointer"
        >
          Export to Artifact Gallery &rarr;
        </button>
      </div>
    </div>
  );
};
