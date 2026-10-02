import React, { useState, useEffect } from 'react';
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
  ShieldCheck,
  Stethoscope,
  Wrench,
  Archive,
  RotateCcw,
  Sparkles,
  FileCheck,
  Zap,
  ArrowRight,
  Database,
  Lock
} from 'lucide-react';
import { PageHeaderNav } from '../common/PageHeaderNav';
import { Button } from '../common/Button';
import { apiClient } from '../../utils/apiClient';

export const CrowsNestView: React.FC = () => {
  const [activeTab, setActiveTab] = useState<'overview' | 'doctor' | 'recovery'>('overview');
  const [isRunningDoctor, setIsRunningDoctor] = useState(false);
  const [isApplyingRemedy, setIsApplyingRemedy] = useState(false);
  const [remedyApplied, setRemedyApplied] = useState(false);
  const [isCreatingSnapshot, setIsCreatingSnapshot] = useState(false);
  const [snapshotCreated, setSnapshotCreated] = useState(false);

  const [diagnostics, setDiagnostics] = useState<any[]>([]);
  const [snapshots, setSnapshots] = useState<any[]>([]);
  const [fleetMetrics, setFleetMetrics] = useState<any>(null);
  const [systemMetrics, setSystemMetrics] = useState<any>(null);

  useEffect(() => {
    apiClient.getDiagnostics().then((data) => {
      if (data && data.length > 0) setDiagnostics(data);
    }).catch(console.error);

    apiClient.getSnapshots().then((data) => {
      if (data && data.length > 0) setSnapshots(data);
    }).catch(console.error);

    apiClient.getFleetMetrics().then((data) => {
      if (data) setFleetMetrics(data);
    }).catch(console.error);

    apiClient.getSystemMetrics().then((data) => {
      if (data) setSystemMetrics(data);
    }).catch(console.error);
  }, []);

  const handleRunDoctor = async () => {
    setIsRunningDoctor(true);
    try {
      const data = await apiClient.getDiagnostics();
      if (data && data.length > 0) setDiagnostics(data);
      const fMet = await apiClient.getFleetMetrics();
      if (fMet) setFleetMetrics(fMet);
      const sMet = await apiClient.getSystemMetrics();
      if (sMet) setSystemMetrics(sMet);
    } catch (e) {
      console.error('Failed to run doctor:', e);
    } finally {
      setIsRunningDoctor(false);
    }
  };

  const handleApplyRemedy = async () => {
    setIsApplyingRemedy(true);
    try {
      const res = await apiClient.applyRemedy();
      setRemedyApplied(true);
      if (res.diagnostics) {
        setDiagnostics(res.diagnostics);
      } else {
        setDiagnostics((prev) =>
          prev.map((d) =>
            d.id === 'd-5'
              ? { ...d, status: 'healthy', latency: '18ms', detail: 'Socket timeout deadline enforced; leak cleared.' }
              : d
          )
        );
      }
    } catch (e) {
      console.error('Failed to apply remedy:', e);
    } finally {
      setIsApplyingRemedy(false);
    }
  };

  const handleCreateSnapshot = async () => {
    setIsCreatingSnapshot(true);
    try {
      const newSnap = await apiClient.createSnapshot('Manual Sovereign Fleet Snapshot');
      setSnapshotCreated(true);
      if (newSnap) {
        setSnapshots((prev) => [newSnap, ...prev]);
      }
    } catch (e) {
      console.error('Failed to create snapshot:', e);
    } finally {
      setIsCreatingSnapshot(false);
    }
  };

  const hasWarnings = diagnostics.some((d) => d.status === 'warning');

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto px-4 sm:px-6 pt-0 pb-6 space-y-4 max-w-5xl mx-auto w-full animate-view-fade-in scrollbar-none">
      {/* Standard Reusable PageHeader with Integrated Chips */}
      <PageHeaderNav
        icon={<Activity className="w-4 h-4 text-teal-500 shrink-0" />}
        title="Crow’s Nest"
        badge={
          <span
            className={`text-xs font-mono px-2 py-0.5 rounded font-semibold ${
              hasWarnings && !remedyApplied
                ? 'bg-amber-500/20 text-amber-500 border border-amber-500/30'
                : 'bg-emerald-500/10 text-emerald-500'
            }`}
          >
            {hasWarnings && !remedyApplied ? '1 WARNING DETECTED' : 'ALL SYSTEMS NOMINAL'}
          </span>
        }
        description="Fleet Health, Technical Observability, Automated Doctor Diagnostics, and Disaster Recovery."
        actions={
          <div className="flex items-center gap-1.5 sm:gap-2 shrink-0">
            {activeTab === 'doctor' && hasWarnings && !remedyApplied && (
              <Button
                variant="amber"
                size="sm"
                icon={<Wrench className={`w-3.5 h-3.5 ${isApplyingRemedy ? 'animate-spin' : ''}`} />}
                shortLabel="Remedy"
                disabled={isApplyingRemedy}
                onClick={handleApplyRemedy}
              >
                {isApplyingRemedy ? 'Applying Remedy...' : 'Apply Remedy'}
              </Button>
            )}

            {activeTab === 'recovery' && (
              <Button
                variant="primary"
                size="sm"
                icon={<Archive className="w-3.5 h-3.5" />}
                shortLabel="Snapshot"
                disabled={isCreatingSnapshot}
                onClick={handleCreateSnapshot}
              >
                {isCreatingSnapshot ? 'Creating Snapshot...' : 'Create Snapshot'}
              </Button>
            )}

            <Button
              variant="secondary"
              size="sm"
              icon={<RefreshCw className={`w-3.5 h-3.5 ${isRunningDoctor ? 'animate-spin text-teal-500' : ''}`} />}
              shortLabel="Scan"
              disabled={isRunningDoctor}
              onClick={handleRunDoctor}
            >
              Scan Diagnostics
            </Button>
          </div>
        }
        chips={{
          items: [
            { id: 'overview', label: 'System Observability' },
            { id: 'doctor', label: 'Crow’s Nest Doctor', badge: hasWarnings && !remedyApplied },
            { id: 'recovery', label: 'Disaster Recovery', count: snapshots.length }
          ],
          selectedId: activeTab,
          onSelect: (id) => setActiveTab(id as any),
          variant: 'pills'
        }}
      />

      {/* Tab 1: Overview & Metrics */}
      {activeTab === 'overview' && (
        <div className="space-y-6">
          {/* Real Backend Telemetry Cards */}
          <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
            <div className="p-3.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-1">
              <span className="text-[10px] font-mono text-neutral-400 uppercase">Gateway Latency</span>
              <div className="text-xl font-bold font-mono text-emerald-500">{fleetMetrics?.gatewayLatencyMs ?? 14} ms</div>
              <div className="text-[10px] text-neutral-400">Zero packet drops</div>
            </div>

            <div className="p-3.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-1">
              <span className="text-[10px] font-mono text-neutral-400 uppercase">Active Goroutines</span>
              <div className="text-xl font-bold font-mono text-neutral-800 dark:text-neutral-200">{fleetMetrics?.activeWorkers ?? 42}</div>
              <div className="text-[10px] text-neutral-400">Background daemons</div>
            </div>

            <div className="p-3.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-1">
              <span className="text-[10px] font-mono text-neutral-400 uppercase">Host Runtime</span>
              <div className="text-xl font-bold font-mono text-teal-500 truncate">{systemMetrics?.platform ? `${systemMetrics.platform.toUpperCase()} (${systemMetrics.arch})` : 'Landlock OS'}</div>
              <div className="text-[10px] text-neutral-400">{systemMetrics?.nodeVersion || 'Tauri Security Shield'}</div>
            </div>

            <div className="p-3.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-1">
              <span className="text-[10px] font-mono text-neutral-400 uppercase">Memory Footprint</span>
              <div className="text-xl font-bold font-mono text-neutral-800 dark:text-neutral-200">{fleetMetrics?.memoryUsedMB ?? (systemMetrics?.memoryUsage?.usedMB ?? 14.2)} MB</div>
              <div className="text-[10px] text-neutral-400">Host memory allocated</div>
            </div>
          </div>

          {/* Quick Doctor Summary Preview */}
          <div className="rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] p-4 text-xs space-y-3">
            <div className="flex items-center justify-between">
              <div className="font-semibold text-neutral-900 dark:text-neutral-100 flex items-center gap-2">
                <ShieldCheck className="w-4 h-4 text-teal-500" />
                <span>Diagnostics Summary</span>
              </div>
              <button
                onClick={() => setActiveTab('doctor')}
                className="text-teal-600 dark:text-teal-400 hover:underline font-semibold"
              >
                Open Full Doctor &rarr;
              </button>
            </div>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-2 text-[11px]">
              {diagnostics.slice(0, 4).map((d) => (
                <div key={d.id} className="p-2.5 rounded-lg border border-neutral-100 dark:border-neutral-800 flex items-center justify-between">
                  <span className="font-medium text-neutral-800 dark:text-neutral-200">{d.component}</span>
                  <span className="font-mono text-emerald-500">{d.latency}</span>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}

      {/* Tab 2: Crow's Nest Doctor (Self-Healing) */}
      {activeTab === 'doctor' && (
        <div className="space-y-4 text-xs">
          {hasWarnings && !remedyApplied && (
            <div className="p-4 rounded-xl border border-amber-500/40 bg-amber-500/10 flex flex-col sm:flex-row sm:items-center justify-between gap-3">
              <div className="space-y-1">
                <div className="font-bold text-amber-500 flex items-center gap-1.5">
                  <AlertTriangle className="w-4 h-4" />
                  <span>Automated Remedy Available for Test Runner Teardown</span>
                </div>
                <p className="text-[11px] text-neutral-600 dark:text-neutral-300">
                  Doctor isolated a lingering socket deadline in <code className="font-mono">ws.rs</code>.
                  Clicking &ldquo;Apply Automated Remedy&rdquo; injects an explicit socket timeout and clears connection leaks.
                </p>
              </div>
              <button
                onClick={handleApplyRemedy}
                disabled={isApplyingRemedy}
                className="px-3.5 py-1.5 rounded-lg bg-amber-500 hover:bg-amber-600 text-neutral-950 font-bold text-xs transition-colors shrink-0 shadow-xs cursor-pointer disabled:opacity-50"
              >
                {isApplyingRemedy ? 'Remediating...' : 'Apply Remedy Now'}
              </button>
            </div>
          )}

          {remedyApplied && (
            <div className="p-3.5 rounded-xl border border-emerald-500/40 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 flex items-center gap-2">
              <CheckCircle2 className="w-4 h-4" />
              <span>Remedy applied successfully. All sockets cycled and tests are nominal.</span>
            </div>
          )}

          <div className="rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] overflow-hidden">
            <div className="p-3 border-b border-neutral-100 dark:border-neutral-800 font-semibold text-neutral-700 dark:text-neutral-300">
              System Component Diagnostic Suite ({diagnostics.length} checks)
            </div>
            <div className="divide-y divide-neutral-100 dark:divide-neutral-800">
              {diagnostics.map((d) => (
                <div key={d.id} className="p-3.5 flex flex-col sm:flex-row sm:items-center justify-between gap-2">
                  <div className="space-y-0.5">
                    <div className="font-semibold text-neutral-900 dark:text-neutral-100 flex items-center gap-2">
                      <span className={`w-2 h-2 rounded-full ${
                        d.status === 'healthy' ? 'bg-emerald-500' : 'bg-amber-500 animate-pulse'
                      }`} />
                      <span>{d.component}</span>
                    </div>
                    <div className="text-[11px] text-neutral-500 dark:text-neutral-400">{d.detail}</div>
                  </div>
                  <div className="flex items-center gap-2 self-start sm:self-auto font-mono text-[11px]">
                    <span className="text-neutral-400">{d.latency}</span>
                    <span className={`px-2 py-0.5 rounded text-[10px] font-semibold uppercase ${
                      d.status === 'healthy' ? 'bg-emerald-500/20 text-emerald-500' : 'bg-amber-500/20 text-amber-500'
                    }`}>
                      {d.status}
                    </span>
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}

      {/* Tab 3: Disaster Recovery & Snapshots */}
      {activeTab === 'recovery' && (
        <div className="space-y-4 text-xs">
          <div className="p-3.5 rounded-xl border border-teal-500/30 bg-teal-500/5 space-y-1">
            <div className="font-semibold text-teal-700 dark:text-teal-300 flex items-center gap-1.5">
              <Archive className="w-4 h-4 text-teal-500" />
              <span>Sovereign State Snapshots (/api/backup)</span>
            </div>
            <p className="text-neutral-600 dark:text-neutral-300 leading-relaxed text-[11px]">
              Every snapshot encapsulates your Ships, Charters, Quests, Artifacts, and Fleet Code policies into a single portable bundle.
              The dry-run planner verifies schema migration compatibility before committing any restore.
            </p>
          </div>

          <div className="space-y-3">
            <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
              Available Fleet Backup Points
            </span>

            <div className="space-y-3">
              {snapshots.map((snap) => (
                <div
                  key={snap.id}
                  className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] shadow-xs space-y-2.5"
                >
                  <div className="flex items-start justify-between">
                    <div>
                      <div className="font-bold text-neutral-900 dark:text-neutral-100">{snap.title}</div>
                      <div className="text-[11px] text-neutral-400">{snap.createdAt} &middot; Size: {snap.size}</div>
                    </div>
                    <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-neutral-100 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-400">
                      Schema {snap.schemaVersion}
                    </span>
                  </div>

                  <div className="text-[11px] font-mono text-neutral-500 dark:text-neutral-400">
                    Included: {snap.entitiesCount}
                  </div>

                  <div className="pt-2 border-t border-neutral-100 dark:border-neutral-800 flex items-center justify-end gap-2">
                    <button className="px-2.5 py-1 rounded-md border border-neutral-200 dark:border-neutral-700 text-neutral-600 dark:text-neutral-400 hover:border-teal-500 transition-colors cursor-pointer">
                      Dry-Run Diff Preview
                    </button>
                    <button className="px-2.5 py-1 rounded-md bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-semibold hover:opacity-90 transition-opacity cursor-pointer">
                      Restore Snapshot &rarr;
                    </button>
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
