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
import { PageHeader } from '../common/PageHeader';
import { PageStickyNav } from '../common/PageStickyNav';
import { SubMenuScroller } from '../common/SubMenuScroller';

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

  const severities = [
    { id: 'all', label: 'All Severities' },
    { id: 'info', label: 'Info' },
    { id: 'warning', label: 'Warning' },
    { id: 'alert', label: 'Alert' },
    { id: 'success', label: 'Success' }
  ];

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto p-4 sm:p-6 space-y-4 max-w-4xl mx-auto w-full animate-view-fade-in scrollbar-none">
      {/* Reusable Standard Header */}
      <PageHeader
        icon={<BookOpen className="w-4 h-4 text-teal-500 shrink-0" />}
        title="Logbook"
        badge={
          <span className="text-xs font-mono text-neutral-400">
            ({filtered.length} Recorded Entries)
          </span>
        }
        description="Audit & Activity History. Chronological event log with cryptographic correlation IDs and actor attribution."
        search={{
          value: search,
          onChange: setSearch,
          placeholder: 'Search traces, actors, correlation IDs...'
        }}
        actions={
          <button
            onClick={handleExportJSON}
            className="flex items-center gap-1 sm:gap-1.5 px-2.5 sm:px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-neutral-700 dark:text-neutral-300 text-xs font-medium hover:bg-neutral-50 dark:hover:bg-neutral-900 cursor-pointer shadow-2xs"
          >
            <Download className="w-3.5 h-3.5" />
            <span className="hidden sm:inline">Export JSON</span>
            <span className="sm:hidden">Export</span>
          </button>
        }
      />

      {/* Floating Sticky Sub-Tabs with Navigation Arrows (< >) */}
      <PageStickyNav>
        <SubMenuScroller className="gap-2" containerClassName="w-full">
          {severities.map((sev) => {
            const isSelected = filterSeverity === sev.id;
            const count = sev.id === 'all'
              ? logbook.length
              : logbook.filter((e) => e.severity === sev.id).length;

            return (
              <button
                key={sev.id}
                onClick={() => setFilterSeverity(sev.id)}
                className={`flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs font-medium transition-all shrink-0 cursor-pointer ${
                  isSelected
                    ? 'bg-neutral-200 dark:bg-neutral-800 text-neutral-900 dark:text-white font-semibold shadow-2xs'
                    : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200'
                }`}
              >
                <span>{sev.label}</span>
                <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-neutral-200/80 dark:bg-neutral-700/80 text-neutral-700 dark:text-neutral-300">
                  {count}
                </span>
              </button>
            );
          })}
        </SubMenuScroller>
      </PageStickyNav>

      {/* Timeline List */}
      <div className="space-y-3">
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
  );
};
