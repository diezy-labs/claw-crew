import React, { useState, useRef } from 'react';
import {
  FileText,
  Search,
  Sparkles,
  Ship,
  Coins,
  CheckCircle2,
  AlertTriangle,
  X,
  Download,
  Share2,
  BookmarkCheck,
  Compass
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';
import { Artifact } from '../../types';
import { PageHeader } from '../common/PageHeader';
import { PageStickyNav } from '../common/PageStickyNav';
import { SubMenuScroller } from '../common/SubMenuScroller';
import { ItemCard } from '../common/ItemCard';
import { CardPopover } from '../common/CardPopover';

export const ArtifactsView: React.FC = () => {
  const {
    artifacts,
    promoteArtifactToTreasure,
    selectedArtifactId,
    setSelectedArtifactId,
    ships,
    crew
  } = useFleetStore();

  const [search, setSearch] = useState('');
  const [filterType, setFilterType] = useState('all');

  const filtered = artifacts.filter((a) => {
    const matchSearch =
      a.title.toLowerCase().includes(search.toLowerCase()) ||
      a.summary.toLowerCase().includes(search.toLowerCase());
    const matchType = filterType === 'all' || a.type === filterType;
    return matchSearch && matchType;
  });

  const selectedArtifact = artifacts.find((a) => a.id === selectedArtifactId);

  const handleExport = (art: Artifact) => {
    const blob = new Blob([art.content], { type: 'text/markdown' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${art.title.replace(/\s+/g, '_')}.md`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const artifactTypes = [
    { id: 'all', label: 'All Types' },
    { id: 'health-brief', label: 'Health Brief' },
    { id: 'ci-triage', label: 'CI Triage' },
    { id: 'readiness-checklist', label: 'Readiness Checklist' }
  ];

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto p-4 sm:p-6 space-y-4 max-w-6xl mx-auto w-full animate-view-fade-in scrollbar-none">
      {/* Reusable Standard Header */}
      <PageHeader
        icon={<FileText className="w-4 h-4 text-teal-500 shrink-0" />}
        title="Artifact Gallery"
        badge={
          <span className="hidden sm:inline-flex text-xs font-mono text-teal-600 dark:text-teal-400 px-2 py-0.5 rounded bg-teal-500/10">
            {artifacts.length} Deliverables
          </span>
        }
        description="Artifacts are durable, evidence-backed deliverables produced by Crew Voyages—not transient chat transcripts."
        search={{
          value: search,
          onChange: setSearch,
          placeholder: 'Search artifacts & findings...'
        }}
      />

      {/* Floating Sticky Sub-Tabs with Navigation Arrows (< >) */}
      <PageStickyNav>
        <SubMenuScroller className="gap-2" containerClassName="w-full">
          {artifactTypes.map((type) => {
            const isSelected = filterType === type.id;
            const count = type.id === 'all'
              ? artifacts.length
              : artifacts.filter((a) => a.type === type.id).length;

            return (
              <button
                key={type.id}
                onClick={() => setFilterType(type.id)}
                className={`flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs font-medium transition-all shrink-0 cursor-pointer ${
                  isSelected
                    ? 'bg-neutral-200 dark:bg-neutral-800 text-neutral-900 dark:text-white font-semibold shadow-2xs'
                    : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200'
                }`}
              >
                <span>{type.label}</span>
                <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-neutral-200/80 dark:bg-neutral-700/80 text-neutral-700 dark:text-neutral-300">
                  {count}
                </span>
              </button>
            );
          })}
        </SubMenuScroller>
      </PageStickyNav>

      {/* Grid of Artifacts */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {filtered.map((art) => {
          const ship = ships.find((s) => s.id === art.shipId);
          const producer = crew.find((c) => c.id === art.producerCrewId);

          return (
            <ItemCard
              key={art.id}
              selected={selectedArtifactId === art.id}
              onClick={() => setSelectedArtifactId(art.id)}
              accentColor={art.status === 'treasure' ? 'amber' : 'teal'}
              icon={<FileText className="w-4 h-4 text-teal-500" />}
              title={art.title}
              subtitle={`${producer?.name || 'Specialist'} · ${ship?.name.replace(' Ship', '') || 'Fleet'}`}
              badge={
                art.status === 'treasure' ? (
                  <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-amber-500/20 text-amber-500 font-semibold shrink-0">
                    Treasure
                  </span>
                ) : (
                  <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-blue-500/20 text-blue-500 font-semibold shrink-0">
                    Needs Review
                  </span>
                )
              }
              description={art.summary}
              children={
                art.discoveries.length > 0 ? (
                  <div className="text-[11px] text-neutral-500 flex items-center gap-1.5 font-medium pt-0.5">
                    <Sparkles className="w-3.5 h-3.5 text-amber-500 shrink-0" />
                    <span className="truncate">{art.discoveries[0].title}</span>
                  </div>
                ) : null
              }
              footer={
                <div className="flex items-center justify-between text-[10px] font-mono text-neutral-400">
                  <span>{art.evidenceCount} Citations</span>
                  <span className="text-teal-600 dark:text-teal-400 font-semibold">
                    ${art.voyageCostUSD.toFixed(2)} Voyage Cost
                  </span>
                </div>
              }
            />
          );
        })}
      </div>

      {/* Artifact Detail Inspector Popover / Drawer */}
      <CardPopover
        isOpen={Boolean(selectedArtifact)}
        onClose={() => setSelectedArtifactId(null)}
        variant="sheet-right"
        drawerWidth="sm:w-[560px]"
        icon={<FileText className="w-4 h-4 text-teal-500" />}
        title={selectedArtifact?.title}
        subtitle={
          selectedArtifact && (
            <span>
              Produced by {crew.find((c) => c.id === selectedArtifact.producerCrewId)?.name || 'Specialist'} ·{' '}
              {ships.find((s) => s.id === selectedArtifact.shipId)?.name}
            </span>
          )
        }
        badge={
          selectedArtifact?.status === 'treasure' ? (
            <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-amber-500/20 text-amber-500 font-bold">
              VALIDATED TREASURE
            </span>
          ) : (
            <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-blue-500/20 text-blue-500 font-semibold">
              AWAITING VALIDATION
            </span>
          )
        }
        footer={
          selectedArtifact && (
            <div className="flex items-center justify-between w-full">
              <button
                type="button"
                onClick={() => handleExport(selectedArtifact)}
                className="px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 text-neutral-700 dark:text-neutral-300 text-xs font-medium hover:bg-neutral-100 dark:hover:bg-neutral-800 flex items-center gap-1.5 cursor-pointer"
              >
                <Download className="w-3.5 h-3.5" />
                <span>Export (.md)</span>
              </button>

              {selectedArtifact.status !== 'treasure' ? (
                <button
                  type="button"
                  onClick={() => promoteArtifactToTreasure(selectedArtifact.id)}
                  className="px-4 py-1.5 rounded-lg bg-amber-500 hover:bg-amber-400 text-neutral-950 font-semibold text-xs flex items-center gap-1.5 cursor-pointer transition-colors"
                >
                  <BookmarkCheck className="w-4 h-4" />
                  <span className="hidden sm:inline">Mark as Treasure</span>
                  <span className="sm:hidden">Treasure</span>
                </button>
              ) : (
                <span className="text-xs font-mono text-amber-500 flex items-center gap-1">
                  <CheckCircle2 className="w-4 h-4 text-emerald-500" />
                  Treasure Verified
                </span>
              )}
            </div>
          )
        }
      >
        {selectedArtifact && (
          <div className="space-y-6">
            <div className="p-3.5 rounded-xl bg-neutral-50 dark:bg-neutral-900/50 border border-neutral-200 dark:border-neutral-800">
              <div className="flex items-center justify-between text-xs text-neutral-500 font-mono">
                <span>Evidence Citations: {selectedArtifact.evidenceCount}</span>
                <span className="text-teal-600 dark:text-teal-400 font-semibold">
                  Voyage Cost: ${selectedArtifact.voyageCostUSD.toFixed(2)}
                </span>
              </div>
            </div>

            {/* Discoveries Box */}
            {selectedArtifact.discoveries.length > 0 && (
              <div className="space-y-2">
                <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400 flex items-center gap-1.5">
                  <Sparkles className="w-3.5 h-3.5 text-amber-500" />
                  Key Discoveries &amp; Insights
                </span>
                <div className="space-y-2">
                  {selectedArtifact.discoveries.map((disc) => (
                    <div
                      key={disc.id}
                      className="p-3 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-neutral-50/70 dark:bg-neutral-900/40 text-xs space-y-1"
                    >
                      <div className="flex items-center justify-between font-semibold">
                        <span className="text-neutral-900 dark:text-neutral-100">
                          {disc.title}
                        </span>
                        <span
                          className={`text-[9px] font-mono px-1 py-0.2 rounded uppercase ${
                            disc.type === 'risk'
                              ? 'bg-rose-500/20 text-rose-500'
                              : 'bg-teal-500/20 text-teal-600 dark:text-teal-400'
                          }`}
                        >
                          {disc.type}
                        </span>
                      </div>
                      <p className="text-neutral-600 dark:text-neutral-400">
                        {disc.detail}
                      </p>
                      <div className="text-[10px] font-mono text-neutral-400 pt-0.5">
                        Evidence link: {disc.evidenceSource}
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {/* Rendered Content */}
            <div className="space-y-2">
              <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
                Deliverable Document
              </span>
              <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/40 dark:bg-neutral-950/40 text-xs leading-relaxed font-mono whitespace-pre-wrap text-neutral-800 dark:text-neutral-200">
                {selectedArtifact.content}
              </div>
            </div>
          </div>
        )}
      </CardPopover>
    </div>
  );
};
