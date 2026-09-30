import React, { useState } from 'react';
import {
  BookOpen,
  Search,
  Filter,
  CheckCircle2,
  AlertTriangle,
  Info,
  ShieldAlert,
  Download,
  Clock
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';

export const LogbookView: React.FC = () => {
  const { logbook } = useFleetStore();
  const [search, setSearch] = useState('');
  const [filterSeverity, setFilterSeverity] = useState('all');

  const filtered = logbook.filter((entry) => {
    const matchSearch =
      entry.action.toLowerCase().includes(search.toLowerCase()) ||
      entry.actorName.toLowerCase().includes(search.toLowerCase()) ||
      entry.correlationId.toLowerCase().includes(search.toLowerCase());
    const matchSeverity = filterSeverity === 'all' || entry.severity === filterSeverity;
    return matchSearch && matchSeverity;
  });

  const handleExportJSON = () => {
    const blob = new Blob([JSON.stringify(logbook, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `fleet_logbook_${Date.now()}.json`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-hidden animate-view-fade-in">
      {/* Top Header */}
      <div className="p-4 sm:p-6 border-b border-neutral-200 dark:border-neutral-800 bg-white/40 dark:bg-[#141619]/40 backdrop-blur-xs flex flex-col sm:flex-row sm:items-center justify-between gap-4 shrink-0">
        <div>
          <div className="flex items-center gap-2">
            <h1 className="text-xl sm:text-2xl font-bold tracking-tight text-neutral-900 dark:text-neutral-100">
              Logbook
            </h1>
            <span className="text-xs font-mono text-neutral-400">
              ({filtered.length} Recorded Entries)
            </span>
          </div>
          <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
            Audit &amp; Activity History. Chronological event log with cryptographic correlation IDs and actor attribution.
          </p>
        </div>

        <div className="flex items-center gap-2.5">
          <div className="relative">
            <Search className="w-3.5 h-3.5 absolute left-2.5 top-1/2 -translate-y-1/2 text-neutral-400" />
            <input
              type="text"
              placeholder="Search traces, actors, correlation IDs..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="pl-8 pr-3 py-1.5 text-xs rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-teal-500 w-64"
            />
          </div>

          <button
            onClick={handleExportJSON}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-neutral-700 dark:text-neutral-300 text-xs font-medium hover:bg-neutral-50 dark:hover:bg-neutral-900"
          >
            <Download className="w-3.5 h-3.5" />
            <span>Export JSON</span>
          </button>
        </div>
      </div>

      {/* Timeline List */}
      <div className="flex-1 overflow-y-auto p-4 sm:p-6">
        <div className="max-w-4xl mx-auto space-y-3">
          {filtered.map((entry) => (
            <div
              key={entry.id}
              className="p-3.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] flex items-start justify-between gap-3 text-xs shadow-xs"
            >
              <div className="flex items-start gap-3">
                <div className="mt-0.5 shrink-0">
                  {entry.severity === 'success' && (
                    <CheckCircle2 className="w-4 h-4 text-emerald-500" />
                  )}
                  {entry.severity === 'warning' && (
                    <AlertTriangle className="w-4 h-4 text-amber-500" />
                  )}
                  {entry.severity === 'info' && (
                    <Info className="w-4 h-4 text-teal-500" />
                  )}
                  {entry.severity === 'alert' && (
                    <ShieldAlert className="w-4 h-4 text-rose-500" />
                  )}
                </div>

                <div className="space-y-1">
                  <div className="flex items-center gap-2">
                    <span className="font-semibold text-neutral-900 dark:text-neutral-100">
                      {entry.actorName}
                    </span>
                    <span className="text-[10px] font-mono px-1 py-0.2 rounded bg-neutral-100 dark:bg-neutral-800 text-neutral-500 uppercase">
                      {entry.actorType}
                    </span>
                  </div>
                  <p className="text-neutral-700 dark:text-neutral-300 font-medium">
                    {entry.action}
                  </p>
                  <div className="text-[10px] font-mono text-neutral-400">
                    Trace ID: {entry.correlationId}
                  </div>
                </div>
              </div>

              <div className="text-[11px] font-mono text-neutral-400 shrink-0">
                {entry.timestamp}
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
};
