import React, { useState, useRef } from 'react';
import {
  LayoutGrid,
  PlusCircle,
  Search,
  Filter,
  Ship,
  Clock,
  Play,
  CheckCircle2,
  AlertCircle,
  ShieldAlert,
  Sparkles,
  ChevronRight,
  ArrowRight,
  FileCheck,
  Coins
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';
import { Quest, QuestStatus } from '../../types';
import { PageHeader } from '../common/PageHeader';
import { PageStickyNav } from '../common/PageStickyNav';
import { SubMenuScroller } from '../common/SubMenuScroller';
import { ItemCard } from '../common/ItemCard';
import { CardPopover } from '../common/CardPopover';
import { Modal } from '../common/Modal';

export const MissionBoard: React.FC = () => {
  const {
    quests,
    createQuest,
    updateQuestStatus,
    runQuestVoyage,
    ships,
    crew,
    selectedQuestId,
    setSelectedQuestId,
    setActiveTab
  } = useFleetStore();

  const [search, setSearch] = useState('');
  const [selectedShipFilter, setSelectedShipFilter] = useState('all');
  const [isNewQuestModalOpen, setIsNewQuestModalOpen] = useState(false);

  // Close quest drawer on Escape key press
  React.useEffect(() => {
    if (!selectedQuestId && !isNewQuestModalOpen) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setSelectedQuestId(null);
        setIsNewQuestModalOpen(false);
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [selectedQuestId, setSelectedQuestId, isNewQuestModalOpen]);

  // New Quest form state
  const [newTitle, setNewTitle] = useState('');
  const [newObjective, setNewObjective] = useState('');
  const [newShipId, setNewShipId] = useState('ship-dev');
  const [newPriority, setNewPriority] = useState<'low' | 'medium' | 'high' | 'urgent'>('medium');
  const [newBudget, setNewBudget] = useState(2.00);

  const columns: {
    status: QuestStatus;
    title: string;
    subtitle: string;
    badgeColor: string;
  }[] = [
    { status: 'backlog', title: 'Backlog', subtitle: 'Unprioritized work', badgeColor: 'bg-neutral-200 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-400' },
    { status: 'ready', title: 'Ready to Sail', subtitle: 'Map planned & assigned', badgeColor: 'bg-teal-500/20 text-teal-600 dark:text-teal-400' },
    { status: 'underway', title: 'Underway', subtitle: 'Voyage in progress', badgeColor: 'bg-blue-500/20 text-blue-600 dark:text-blue-400' },
    { status: 'awaiting_captain', title: 'Awaiting Captain', subtitle: 'Decision/Approval gate', badgeColor: 'bg-amber-500/20 text-amber-500' },
    { status: 'treasured', title: 'Treasures Claimed', subtitle: 'Validated value delivered', badgeColor: 'bg-amber-500/30 text-amber-600 dark:text-amber-400 font-bold' }
  ];

  const filteredQuests = quests.filter((q) => {
    const matchesSearch =
      q.title.toLowerCase().includes(search.toLowerCase()) ||
      q.objective.toLowerCase().includes(search.toLowerCase()) ||
      q.projectId.toLowerCase().includes(search.toLowerCase());
    const matchesShip =
      selectedShipFilter === 'all' || q.assignedShipId === selectedShipFilter || q.suggestedShipId === selectedShipFilter;
    return matchesSearch && matchesShip;
  });

  const selectedQuest = quests.find((q) => q.id === selectedQuestId);

  const handleCreateQuestSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!newTitle.trim()) return;

    createQuest({
      title: newTitle.trim(),
      objective: newObjective.trim() || 'Execute bounded team objectives according to Charter.',
      suggestedShipId: newShipId,
      assignedShipId: newShipId,
      priority: newPriority,
      budgetLimitUSD: Number(newBudget)
    });

    setNewTitle('');
    setNewObjective('');
    setIsNewQuestModalOpen(false);
  };

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto p-4 sm:p-6 space-y-4 max-w-7xl mx-auto w-full animate-view-fade-in scrollbar-none">
      {/* Reusable Standard Header */}
      <PageHeader
        icon={<LayoutGrid className="w-4 h-4 text-teal-500 shrink-0" />}
        title="Mission Board"
        badge={
          <span className="text-xs font-mono text-neutral-400">
            ({filteredQuests.length} Quests)
          </span>
        }
        description="Fleet-wide work intake, priority, Map progression, and routing to Ships."
        search={{
          value: search,
          onChange: setSearch,
          placeholder: 'Filter quests...'
        }}
        actions={
          <button
            onClick={() => setIsNewQuestModalOpen(true)}
            className="flex items-center gap-1 sm:gap-1.5 px-2.5 sm:px-3.5 py-1.5 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 text-xs font-semibold hover:opacity-90 active:scale-[0.98] transition-all cursor-pointer shadow-xs shrink-0"
          >
            <PlusCircle className="w-3.5 h-3.5" />
            <span className="hidden sm:inline">New Quest</span>
            <span className="sm:hidden">Quest</span>
          </button>
        }
      />

      {/* Floating Sticky Sub-Tabs with Navigation Arrows (< >) */}
      <PageStickyNav>
        <SubMenuScroller className="gap-2" containerClassName="w-full">
          <button
            onClick={() => setSelectedShipFilter('all')}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs font-medium transition-all shrink-0 cursor-pointer ${
              selectedShipFilter === 'all'
                ? 'bg-neutral-200 dark:bg-neutral-800 text-neutral-900 dark:text-white font-semibold shadow-2xs'
                : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200'
            }`}
          >
            <span>All Ships</span>
            <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-neutral-200/80 dark:bg-neutral-700/80 text-neutral-700 dark:text-neutral-300">
              {quests.length}
            </span>
          </button>
          {ships.map((ship) => {
            const shipQuestsCount = quests.filter(
              (q) => q.assignedShipId === ship.id || q.suggestedShipId === ship.id
            ).length;
            const isSelected = selectedShipFilter === ship.id;

            return (
              <button
                key={ship.id}
                onClick={() => setSelectedShipFilter(ship.id)}
                className={`flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs font-medium transition-all shrink-0 cursor-pointer ${
                  isSelected
                    ? 'bg-teal-500/10 dark:bg-teal-500/15 text-teal-700 dark:text-teal-300 font-semibold border border-teal-500/30'
                    : 'text-neutral-600 dark:text-neutral-400 hover:text-neutral-900 dark:hover:text-white border border-neutral-200 dark:border-neutral-800 bg-white/60 dark:bg-[#181a1d]'
                }`}
              >
                <Ship className="w-3.5 h-3.5 text-neutral-400" />
                <span className="whitespace-nowrap">{ship.name}</span>
                <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-neutral-100 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-400">
                  {shipQuestsCount}
                </span>
              </button>
            );
          })}
        </SubMenuScroller>
      </PageStickyNav>

      {/* Kan-ban Columns Grid */}
      <div className="flex-1 overflow-x-auto pb-4">
        <div className="flex gap-4 h-full min-w-[1100px]">
          {columns.map((col) => {
            const colQuests = filteredQuests.filter((q) => {
              if (col.status === 'ready') return q.status === 'ready';
              if (col.status === 'treasured') return q.status === 'treasured' || q.status === 'completed';
              return q.status === col.status;
            });

            return (
              <div
                key={col.status}
                className="flex-1 flex flex-col rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/70 dark:bg-[#15171a] p-3 overflow-hidden min-w-[220px]"
              >
                {/* Column header */}
                <div className="pb-3 border-b border-neutral-200 dark:border-neutral-800/80 mb-3 shrink-0">
                  <div className="flex items-center justify-between">
                    <span className="text-xs font-semibold text-neutral-900 dark:text-neutral-100">
                      {col.title}
                    </span>
                    <span className={`text-[10px] font-mono px-1.5 py-0.2 rounded-full ${col.badgeColor}`}>
                      {colQuests.length}
                    </span>
                  </div>
                  <div className="text-[11px] text-neutral-400 mt-0.5 truncate">
                    {col.subtitle}
                  </div>
                </div>

                {/* Cards Container */}
                <div
                  className="flex-1 overflow-y-auto space-y-2.5 pr-1"
                >
                  {colQuests.length === 0 ? (
                    <div className="p-4 rounded-lg border border-dashed border-neutral-200 dark:border-neutral-800 text-center text-xs text-neutral-400">
                      No quests in {col.title.toLowerCase()}
                    </div>
                  ) : (
                    colQuests.map((quest) => {
                      const assignedShip = ships.find(
                        (s) => s.id === (quest.assignedShipId || quest.suggestedShipId)
                      );

                      return (
                        <ItemCard
                          key={quest.id}
                          compact
                          selected={selectedQuestId === quest.id}
                          onClick={() => setSelectedQuestId(quest.id)}
                          accentColor={
                            quest.priority === 'urgent'
                              ? 'rose'
                              : quest.priority === 'high'
                              ? 'amber'
                              : 'teal'
                          }
                          title={quest.title}
                          badge={
                            <span
                              className={`text-[9px] font-mono px-1 py-0.2 rounded uppercase shrink-0 font-medium ${
                                quest.priority === 'urgent'
                                  ? 'bg-rose-500/20 text-rose-500'
                                  : quest.priority === 'high'
                                  ? 'bg-amber-500/20 text-amber-500'
                                  : 'bg-neutral-200 dark:bg-neutral-800 text-neutral-400'
                              }`}
                            >
                              {quest.priority}
                            </span>
                          }
                          description={quest.objective}
                          descriptionClamp={2}
                          children={
                            quest.status === 'underway' ? (
                              <div className="space-y-1 pt-1">
                                <div className="flex items-center justify-between text-[10px] font-mono text-neutral-400">
                                  <span>Voyage Progress</span>
                                  <span className="text-teal-600 dark:text-teal-400 font-semibold">
                                    {quest.activeVoyageProgress}%
                                  </span>
                                </div>
                                <div className="w-full bg-neutral-100 dark:bg-neutral-800 h-1.5 rounded-full overflow-hidden">
                                  <div
                                    className="bg-teal-500 h-full rounded-full transition-all duration-300"
                                    style={{ width: `${quest.activeVoyageProgress}%` }}
                                  />
                                </div>
                              </div>
                            ) : null
                          }
                          footer={
                            <div className="space-y-1.5">
                              <div className="flex items-center justify-between text-[10px] text-neutral-400 font-mono">
                                <span className="truncate max-w-[120px]">
                                  {assignedShip?.name || 'Developer Ship'}
                                </span>
                                <span>${quest.budgetLimitUSD.toFixed(2)} cap</span>
                              </div>
                              {quest.status === 'ready' && (
                                <button
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    runQuestVoyage(quest.id);
                                  }}
                                  className="w-full py-1 rounded bg-teal-600/10 dark:bg-teal-500/15 text-teal-700 dark:text-teal-300 hover:bg-teal-600/20 text-[11px] font-medium flex items-center justify-center gap-1 transition-colors cursor-pointer"
                                >
                                  <Play className="w-3 h-3 text-teal-500" />
                                  <span>Set Sail</span>
                                </button>
                              )}
                            </div>
                          }
                        />
                      );
                    })
                  )}
                </div>
              </div>
            );
          })}
        </div>
      </div>

      {/* Quest Detail Popover / Drawer */}
      <CardPopover
        isOpen={Boolean(selectedQuest)}
        onClose={() => setSelectedQuestId(null)}
        variant="sheet-right"
        drawerWidth="sm:w-[500px]"
        icon={<LayoutGrid className="w-4 h-4 text-teal-500" />}
        title={selectedQuest?.title}
        subtitle={selectedQuest?.objective}
        badge={
          selectedQuest && (
            <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-teal-500/20 text-teal-600 dark:text-teal-400 font-semibold">
              {selectedQuest.status.toUpperCase()}
            </span>
          )
        }
        footer={
          selectedQuest && (
            <div className="flex items-center gap-2 w-full">
              {selectedQuest.status === 'ready' && (
                <button
                  type="button"
                  onClick={() => runQuestVoyage(selectedQuest.id)}
                  className="flex-1 py-2 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-semibold text-xs hover:opacity-90 flex items-center justify-center gap-1.5 cursor-pointer shadow-xs"
                >
                  <Play className="w-3.5 h-3.5" />
                  <span className="hidden sm:inline">Set Sail (Start Voyage)</span>
                  <span className="sm:hidden">Set Sail</span>
                </button>
              )}

              {selectedQuest.status === 'awaiting_captain' && (
                <button
                  type="button"
                  onClick={() => setActiveTab('approvals')}
                  className="flex-1 py-2 rounded-lg bg-amber-500 text-neutral-950 font-semibold text-xs hover:opacity-90 flex items-center justify-center gap-1.5 cursor-pointer shadow-xs"
                >
                  <ShieldAlert className="w-3.5 h-3.5" />
                  <span className="hidden sm:inline">Review Approval</span>
                  <span className="sm:hidden">Approval</span>
                </button>
              )}

              {selectedQuest.status === 'underway' && (
                <button
                  type="button"
                  onClick={() => updateQuestStatus(selectedQuest.id, 'review')}
                  className="flex-1 py-2 rounded-lg bg-blue-600 text-white font-semibold text-xs hover:opacity-90 flex items-center justify-center gap-1.5 cursor-pointer shadow-xs"
                >
                  <CheckCircle2 className="w-3.5 h-3.5" />
                  <span className="hidden sm:inline">Advance to Review</span>
                  <span className="sm:hidden">To Review</span>
                </button>
              )}

              <button
                type="button"
                onClick={() => setSelectedQuestId(null)}
                className="px-3.5 py-2 rounded-lg border border-neutral-200 dark:border-neutral-700 text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800 text-xs font-medium cursor-pointer"
              >
                Close
              </button>
            </div>
          )
        }
      >
        {selectedQuest && (
          <div className="space-y-5">
            {/* Scope / Metadata */}
            <div className="grid grid-cols-2 gap-2 text-xs p-3 rounded-lg bg-neutral-50 dark:bg-neutral-900/60 border border-neutral-200 dark:border-neutral-800 font-mono">
              <div>
                <span className="text-[10px] text-neutral-400 block">Workspace</span>
                <span className="font-medium text-neutral-800 dark:text-neutral-200 truncate block">
                  {selectedQuest.workspaceId}
                </span>
              </div>
              <div>
                <span className="text-[10px] text-neutral-400 block">Project</span>
                <span className="font-medium text-neutral-800 dark:text-neutral-200 truncate block">
                  {selectedQuest.projectId}
                </span>
              </div>
              <div>
                <span className="text-[10px] text-neutral-400 block">Budget Cap</span>
                <span className="font-medium text-neutral-800 dark:text-neutral-200">
                  ${selectedQuest.budgetLimitUSD.toFixed(2)}
                </span>
              </div>
              <div>
                <span className="text-[10px] text-neutral-400 block">Est. Cost</span>
                <span className="font-medium text-teal-600 dark:text-teal-400">
                  ${selectedQuest.estimatedCostUSD.toFixed(2)}
                </span>
              </div>
            </div>

            {/* Map Execution Plan */}
            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
                  Living Map (SOP Steps)
                </span>
                <span className="text-[10px] font-mono text-neutral-400">
                  {selectedQuest.mapSteps.filter((s) => s.status === 'completed').length} / {selectedQuest.mapSteps.length} done
                </span>
              </div>

              <div className="space-y-2">
                {selectedQuest.mapSteps.map((step) => {
                  const assignedCrew = crew.find((c) => c.id === step.assignedCrewId);
                  return (
                    <div
                      key={step.stepNumber}
                      className="p-3 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-900/30 space-y-1 text-xs"
                    >
                      <div className="flex items-center justify-between">
                        <span className="font-semibold text-neutral-800 dark:text-neutral-200">
                          {step.stepNumber}. {step.title}
                        </span>
                        <span
                          className={`text-[9px] font-mono px-1 py-0.2 rounded uppercase ${
                            step.status === 'completed'
                              ? 'bg-emerald-500/20 text-emerald-500'
                              : step.status === 'in_progress'
                              ? 'bg-blue-500/20 text-blue-500'
                              : 'bg-neutral-200 dark:bg-neutral-800 text-neutral-400'
                          }`}
                        >
                          {step.status}
                        </span>
                      </div>
                      {assignedCrew && (
                        <div className="text-[11px] text-neutral-500 dark:text-neutral-400 flex items-center gap-1">
                          <span>Crew:</span>
                          <span className="text-teal-600 dark:text-teal-400 font-medium">
                            {assignedCrew.name}
                          </span>
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            </div>

            {/* Required Artifacts Contract */}
            <div className="space-y-1.5">
              <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
                Required Artifact Deliverables
              </span>
              <div className="flex flex-wrap gap-1.5">
                {selectedQuest.requiredArtifacts.map((artName, i) => (
                  <span
                    key={i}
                    className="text-xs px-2.5 py-1 rounded bg-neutral-100 dark:bg-neutral-800 text-neutral-700 dark:text-neutral-300 border border-neutral-200 dark:border-neutral-700 font-mono"
                  >
                    {artName}
                  </span>
                ))}
              </div>
            </div>
          </div>
        )}
      </CardPopover>

      {/* New Quest Modal */}
      <Modal
        isOpen={isNewQuestModalOpen}
        onClose={() => setIsNewQuestModalOpen(false)}
        maxWidth="lg"
        title="Launch New Quest"
      >

            <form onSubmit={handleCreateQuestSubmit} className="space-y-3.5 text-xs">
              <div>
                <label className="block text-neutral-700 dark:text-neutral-300 font-semibold mb-1">
                  Quest Objective / Title
                </label>
                <input
                  type="text"
                  required
                  placeholder="e.g. Audit Branch feat/enhance-agent-phase for Release"
                  value={newTitle}
                  onChange={(e) => setNewTitle(e.target.value)}
                  className="w-full px-3 py-2 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-teal-500"
                />
              </div>

              <div>
                <label className="block text-neutral-700 dark:text-neutral-300 font-semibold mb-1">
                  Detailed Scope &amp; Outcome
                </label>
                <textarea
                  rows={3}
                  placeholder="Describe the desired Artifacts, evidence expectations, and boundaries..."
                  value={newObjective}
                  onChange={(e) => setNewObjective(e.target.value)}
                  className="w-full px-3 py-2 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-teal-500"
                />
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="block text-neutral-700 dark:text-neutral-300 font-semibold mb-1">
                    Assigned Ship
                  </label>
                  <select
                    value={newShipId}
                    onChange={(e) => setNewShipId(e.target.value)}
                    className="w-full px-3 py-2 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none"
                  >
                    {ships.map((s) => (
                      <option key={s.id} value={s.id}>
                        {s.name}
                      </option>
                    ))}
                  </select>
                </div>

                <div>
                  <label className="block text-neutral-700 dark:text-neutral-300 font-semibold mb-1">
                    Priority
                  </label>
                  <select
                    value={newPriority}
                    onChange={(e) => setNewPriority(e.target.value as any)}
                    className="w-full px-3 py-2 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none"
                  >
                    <option value="low">Low</option>
                    <option value="medium">Medium</option>
                    <option value="high">High</option>
                    <option value="urgent">Urgent</option>
                  </select>
                </div>
              </div>

              <div>
                <label className="block text-neutral-700 dark:text-neutral-300 font-semibold mb-1">
                  Voyage Budget Hard Cap (USD)
                </label>
                <input
                  type="number"
                  step="0.50"
                  min="0.50"
                  max="10.00"
                  value={newBudget}
                  onChange={(e) => setNewBudget(Number(e.target.value))}
                  className="w-full px-3 py-2 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none"
                />
              </div>

              <div className="pt-2 flex items-center justify-end gap-2 border-t border-neutral-200 dark:border-neutral-800">
                <button
                  type="button"
                  onClick={() => setIsNewQuestModalOpen(false)}
                  className="px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 text-neutral-600 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800 cursor-pointer"
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  className="px-4 py-1.5 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-semibold hover:opacity-90 cursor-pointer text-xs"
                >
                  <span className="hidden sm:inline">Create Quest</span>
                  <span className="sm:hidden">Create</span>
                </button>
              </div>
            </form>
      </Modal>
    </div>
  );
};
