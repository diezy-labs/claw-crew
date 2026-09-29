import React, { useState } from 'react';
import {
  Activity,
  CheckCircle2,
  AlertTriangle,
  Server,
  Cpu,
  Clock,
  HardDrive,
  RefreshCw,
  Terminal,
  ShieldCheck
} from 'lucide-react';

export const CrowsNestView: React.FC = () => {
  const [isRunningDoctor, setIsRunningDoctor] = useState(false);
  const [doctorSuccess, setDoctorSuccess] = useState(true);

  const checks = [
    { name: 'Go Engine Daemon Gateway', status: 'HEALTHY', latency: '14ms', detail: 'Serving on port :8080' },
    { name: 'Rust / Tauri Landlock Sandbox', status: 'HEALTHY', latency: '<1ms', detail: 'Kernel OS isolation active' },
    { name: 'Local SQLite Operational Store', status: 'HEALTHY', latency: '2ms', detail: 'WAL mode enabled; 0 deadlocks' },
    { name: 'Anthropic Claude API Endpoint', status: 'HEALTHY', latency: '142ms', detail: 'Quota nominal' },
    { name: 'Google Gemini AI Endpoint', status: 'HEALTHY', latency: '98ms', detail: 'Major capability server-side active' },
    { name: 'Ollama Local Daemon', status: 'HEALTHY', latency: '12ms', detail: 'Model deepseek-r1 loaded in VRAM' },
    { name: 'Git Repository Working Tree', status: 'HEALTHY', latency: '8ms', detail: 'Branch feat/enhance-agent-phase clean' }
  ];

  const handleRunDoctor = () => {
    setIsRunningDoctor(true);
    setTimeout(() => {
      setIsRunningDoctor(false);
      setDoctorSuccess(true);
    }, 800);
  };

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto p-4 sm:p-6 space-y-6 max-w-5xl mx-auto w-full animate-view-fade-in">
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
        <div>
          <div className="flex items-center gap-2">
            <h1 className="text-xl sm:text-2xl font-bold tracking-tight text-neutral-900 dark:text-neutral-100">
              Crow’s Nest
            </h1>
            <span className="text-xs font-mono text-emerald-500 px-2 py-0.5 rounded bg-emerald-500/10">
              ALL SYSTEMS NOMINAL
            </span>
          </div>
          <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
            Fleet Health &amp; Technical Observability. Diagnostic monitoring across Go runtime, Rust sandbox, and model endpoints.
          </p>
        </div>

        <button
          onClick={handleRunDoctor}
          disabled={isRunningDoctor}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-neutral-800 dark:text-neutral-200 text-xs font-medium hover:bg-neutral-50 dark:hover:bg-neutral-900 self-start sm:self-auto disabled:opacity-50"
        >
          <RefreshCw className={`w-3.5 h-3.5 ${isRunningDoctor ? 'animate-spin' : ''}`} />
          <span>Run Fleet Doctor</span>
        </button>
      </div>

      {/* Overview Metric Cards */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
        <div className="p-3.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-1">
          <span className="text-[10px] font-mono text-neutral-400 uppercase">Gateway Latency</span>
          <div className="text-xl font-bold font-mono text-emerald-500">18 ms</div>
          <div className="text-[10px] text-neutral-400">Zero packet drops</div>
        </div>

        <div className="p-3.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-1">
          <span className="text-[10px] font-mono text-neutral-400 uppercase">Active Goroutines</span>
          <div className="text-xl font-bold font-mono text-neutral-800 dark:text-neutral-200">42</div>
          <div className="text-[10px] text-neutral-400">2 background workers</div>
        </div>

        <div className="p-3.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-1">
          <span className="text-[10px] font-mono text-neutral-400 uppercase">Host Sandboxing</span>
          <div className="text-xl font-bold font-mono text-teal-500">Landlock</div>
          <div className="text-[10px] text-neutral-400">Tauri security shield</div>
        </div>

        <div className="p-3.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-1">
          <span className="text-[10px] font-mono text-neutral-400 uppercase">Local Memory DB</span>
          <div className="text-xl font-bold font-mono text-neutral-800 dark:text-neutral-200">14.2 MB</div>
          <div className="text-[10px] text-neutral-400">1,480 vector nodes</div>
        </div>
      </div>

      {/* Doctor Diagnostics Table */}
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
            System Component Diagnostics ({checks.length} checks)
          </span>
          <span className="text-[11px] font-mono text-neutral-400">
            Last verified: 30s ago
          </span>
        </div>

        <div className="rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] overflow-hidden text-xs">
          <div className="divide-y divide-neutral-100 dark:divide-neutral-800">
            {checks.map((c) => (
              <div
                key={c.name}
                className="p-3.5 flex items-center justify-between gap-3 hover:bg-neutral-50/50 dark:hover:bg-neutral-900/30 transition-colors"
              >
                <div className="flex items-center gap-2.5">
                  <CheckCircle2 className="w-4 h-4 text-emerald-500 shrink-0" />
                  <div>
                    <span className="font-semibold text-neutral-900 dark:text-neutral-100">
                      {c.name}
                    </span>
                    <div className="text-[11px] text-neutral-400 font-mono">
                      {c.detail}
                    </div>
                  </div>
                </div>

                <div className="flex items-center gap-3 shrink-0 font-mono text-[11px]">
                  <span className="text-neutral-400">{c.latency}</span>
                  <span className="text-[10px] px-2 py-0.5 rounded bg-emerald-500/20 text-emerald-500 font-bold">
                    {c.status}
                  </span>
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
};
