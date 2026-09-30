import React, { useState, useRef } from 'react';
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
import { PageHeaderNav } from '../common/PageHeaderNav';
import { Button } from '../common/Button';
import { ItemCard } from '../common/ItemCard';

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
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto px-4 sm:px-6 pt-0 pb-6 space-y-3 sm:space-y-4 max-w-4xl mx-auto w-full animate-view-fade-in scrollbar-none">
      {/* Reusable General Header with Integrated Chips */}
      <PageHeaderNav
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
          <Button
            variant="secondary"
            size="sm"
            icon={<Download className="w-3.5 h-3.5" />}
            shortLabel="Export"
            onClick={handleExportJSON}
          >
            Export JSON
          </Button>
        }
        chips={{
          items: severities.map((sev) => ({
            id: sev.id,
            label: sev.label,
            count: sev.id === 'all' ? logbook.length : logbook.filter((e) => e.severity === sev.id).length
          })),
          selectedId: filterSeverity,
          onSelect: setFilterSeverity,
          variant: 'pills'
        }}
      />

      {/* Timeline List */}
      <div className="space-y-3">
        {filtered.map((entry) => (
          <ItemCard
            key={entry.id}
            compact
            icon={
              entry.severity === 'success' ? (
                <CheckCircle2 className="w-4 h-4 text-emerald-500" />
              ) : entry.severity === 'warning' ? (
                <AlertTriangle className="w-4 h-4 text-amber-500" />
              ) : entry.severity === 'alert' ? (
                <ShieldAlert className="w-4 h-4 text-rose-500" />
              ) : (
                <Info className="w-4 h-4 text-teal-500" />
              )
            }
            title={entry.actorName}
            badge={
              <div className="flex items-center gap-2">
                <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-neutral-100 dark:bg-neutral-800 text-neutral-500 uppercase">
                  {entry.actorType}
                </span>
                <span className="text-[11px] font-mono text-neutral-400">
                  {entry.timestamp}
                </span>
              </div>
            }
            description={entry.action}
            footer={
              <div className="text-[10px] font-mono text-neutral-400">
                Trace ID: {entry.correlationId}
              </div>
            }
          />
        ))}
      </div>
    </div>
  );
};
