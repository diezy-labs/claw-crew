import React, { useState, useRef } from 'react';
import {
  Map,
  Plus,
  PlusCircle,
  Search,
  Filter,
  Ship,
  Sparkles,
  Play,
  CheckCircle2,
  AlertTriangle,
  Folder,
  Layers,
  ChevronLeft,
  ChevronRight,
  SlidersHorizontal,
  Workflow,
  ShieldCheck
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';
import { Quest, QuestTab } from '../../types';
import { PageHeader } from '../common/PageHeader';
import { PageStickyNav } from '../common/PageStickyNav';
import { SubMenuScroller } from '../common/SubMenuScroller';

export const QuestsView: React.FC = () => {
  const {
    quests,
    createQuest,
    runQuestVoyage,
    ships,
    crew,
    workspaces,
    projects,
    selectedWorkspace,
    selectedProject,
    setSelectedWorkspace,
    setSelectedProject,
    selectedQuestId,
    setSelectedQuestId,
    setActiveTab
  } = useFleetStore();

  const [activeTabFilter, setActiveTabFilter] = useState<QuestTab>('active');
  const [mapStudioMode, setMapStudioMode] = useState<'guided' | 'advanced'>('guided');
  const [search, setSearch] = useState('');
  const [isMobileSearchOpen, setIsMobileSearchOpen] = useState(false);
  const [isNewQuestModalOpen, setIsNewQuestModalOpen] = useState(false);
  const [isHeaderVisible, setIsHeaderVisible] = useState(true);
  const lastScrollTop = useRef(0);

  const handleViewportScroll = (e: React.UIEvent<HTMLDivElement>) => {
    const currentScrollTop = e.currentTarget.scrollTop;
    if (typeof window !== 'undefined' && window.innerWidth < 640) {
      if (currentScrollTop > 15) {
        if (currentScrollTop > lastScrollTop.current + 6) {
          // Scrolling down: collapse header so subtabs attach directly under navbar
          setIsHeaderVisible(false);
        } else if (currentScrollTop < lastScrollTop.current - 8) {
          // Scrolling up: reveal header
          setIsHeaderVisible(true);
        }
      } else {
        setIsHeaderVisible(true);
      }
    }
    lastScrollTop.current = currentScrollTop;
  };

  // New Quest Form state
  const [newTitle, setNewTitle] = useState('');
  const [newObjective, setNewObjective] = useState('');
  const [newShipId, setNewShipId] = useState('ship-dev');
  const [newPriority, setNewPriority] = useState<'low' | 'medium' | 'high' | 'urgent'>('high');
  const [newBudget, setNewBudget] = useState(2.00);

  const questTabs: { id: QuestTab; label: string }[] = [
    { id: 'active', label: 'Active Quests' },
    { id: 'planned', label: 'Planned / Drafts' },
    { id: 'recurring', label: 'Recurring Schedules' },
    { id: 'completed', label: 'Completed' },
    { id: 'treasures', label: 'Treasures' },
    { id: 'maps', label: 'Reusable Maps (SOPs)' }
  ];

  const filteredQuests = quests.filter((q) => {
    const matchSearch =
      (q.title || '').toLowerCase().includes(search.toLowerCase()) ||
      (q.objective || '').toLowerCase().includes(search.toLowerCase());

    if (!matchSearch) return false;

    if (activeTabFilter === 'active') {
      return q.status === 'ready' || q.status === 'underway' || q.status === 'awaiting_captain' || q.status === 'review';
    }
    if (activeTabFilter === 'planned') {
      return q.status === 'backlog';
    }
    if (activeTabFilter === 'completed') {
      return q.status === 'completed';
    }
    if (activeTabFilter === 'treasures') {
      return q.status === 'treasured';
    }
    return true;
  });

  const selectedQuest = quests.find((q) => q.id === selectedQuestId) || quests[0];

  const handleCreateQuestSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!newTitle.trim()) return;

    createQuest({
      title: newTitle.trim(),
      objective: newObjective.trim() || 'Execute bounded team objectives according to Charter.',
      workspaceId: selectedWorkspace,
      projectId: selectedProject,
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
<<<<<<< HEAD
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto p-4 sm:p-6 space-y-4 max-w-6xl mx-auto w-full animate-view-fade-in scrollbar-none">
      {/* Reusable Standard Header */}
      <PageHeader
        icon={<Map className="w-4 h-4 text-teal-500 shrink-0" />}
        title="Quests"
        badge={
          <span className="hidden sm:inline-flex text-xs font-mono text-teal-600 dark:text-teal-400 px-2 py-0.5 rounded bg-teal-500/10">
            {quests.length} Active Missions
          </span>
        }
        description={`What work are we planning, running, and improving for ${selectedWorkspace} / ${selectedProject}?`}
        search={{
          value: search,
          onChange: setSearch,
          placeholder: 'Search quests, objectives, keys...'
        }}
        actions={
=======
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-hidden animate-view-fade-in">
      {/* Top Header - Auto-collapses on mobile when scrolled down so content sits directly below navbar */}
      <div
        className={`transition-all duration-300 shrink-0 ${
          isHeaderVisible
            ? 'max-h-24 px-4 py-3 sm:p-5 border-b border-neutral-200 dark:border-neutral-800 bg-white/40 dark:bg-[#141619]/40 backdrop-blur-xs opacity-100'
            : 'max-h-0 py-0 px-4 border-b-0 opacity-0 overflow-hidden pointer-events-none sm:max-h-none sm:p-5 sm:border-b sm:border-neutral-200 sm:dark:border-neutral-800 sm:bg-white/40 sm:dark:bg-[#141619]/40 sm:opacity-100 sm:pointer-events-auto'
        } flex items-center justify-between gap-3`}
      >
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <h1 className="text-xl sm:text-2xl font-bold tracking-tight text-neutral-900 dark:text-neutral-100">
              Quests
            </h1>
            <span className="hidden sm:inline-block text-xs font-mono text-teal-600 dark:text-teal-400 px-2 py-0.5 rounded bg-teal-500/10">
              Workspace &amp; Project SOPs
            </span>
          </div>
          <p className="hidden sm:block text-xs text-neutral-500 dark:text-neutral-400 mt-0.5 truncate">
            What work are we planning, running, and improving for {selectedWorkspace} / {selectedProject}?
          </p>
        </div>

        <div className="flex items-center gap-2 shrink-0">
          {/* Mobile search: icon toggle */}
          <div className="sm:hidden relative">
            {isMobileSearchOpen ? (
              <div className="flex items-center gap-1.5 animate-in fade-in duration-100">
                <input
                  type="text"
                  autoFocus
                  placeholder="Search..."
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                  className="pl-2.5 pr-2 py-1 text-xs rounded-lg border border-teal-500 bg-white dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 w-28 focus:outline-none"
                />
                <button
                  type="button"
                  onClick={() => {
                    setIsMobileSearchOpen(false);
                    setSearch('');
                  }}
                  className="p-1 text-neutral-400 hover:text-neutral-600 dark:hover:text-neutral-200 text-xs cursor-pointer"
                  title="Close search"
                >
                  ✕
                </button>
              </div>
            ) : (
              <button
                type="button"
                onClick={() => setIsMobileSearchOpen(true)}
                className="p-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-neutral-600 dark:text-neutral-300 hover:border-teal-500/50 transition-colors cursor-pointer shadow-xs"
                title="Search quests"
                aria-label="Search quests"
              >
                <Search className="w-3.5 h-3.5" />
              </button>
            )}
          </div>

          {/* Desktop search bar */}
          <div className="hidden sm:block relative">
            <Search className="w-3.5 h-3.5 absolute left-2.5 top-1/2 -translate-y-1/2 text-neutral-400" />
            <input
              type="text"
              placeholder="Search quests..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="pl-8 pr-3 py-1.5 text-xs rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-teal-500 w-44"
            />
          </div>

          {/* Quest Button */}
>>>>>>> 2e922019e513ae8198c4bf6e1addb10f2c027867
          <button
            onClick={() => setIsNewQuestModalOpen(true)}
            className="flex items-center gap-1 sm:gap-1.5 px-2.5 sm:px-3.5 py-1.5 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 text-xs font-semibold hover:opacity-90 active:scale-[0.98] transition-all cursor-pointer shadow-xs shrink-0"
          >
            <Plus className="w-3.5 h-3.5" />
            <span className="hidden sm:inline">+ Quest</span>
            <span className="sm:hidden">Quest</span>
          </button>
        }
      />

      {/* Floating Sticky Sub-Tabs with Navigation Arrows & Guided Map Button */}
      <PageStickyNav
        rightContent={
          <button
            type="button"
            onClick={() => setMapStudioMode((prev) => (prev === 'guided' ? 'advanced' : 'guided'))}
            className="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white/90 dark:bg-neutral-900/90 text-[11px] font-medium text-neutral-700 dark:text-neutral-200 hover:border-teal-500/40 hover:text-teal-600 dark:hover:text-teal-400 transition-all cursor-pointer shadow-2xs group shrink-0"
            title={`Switch to ${mapStudioMode === 'guided' ? 'Advanced Studio (Nodes)' : 'Guided Map (Steps)'}`}
          >
            <Workflow className="w-3.5 h-3.5 text-teal-500 shrink-0" />
            <span className="font-semibold text-neutral-900 dark:text-neutral-100">
              {mapStudioMode === 'guided' ? 'Guided Map' : 'Advanced Studio'}
            </span>
            <div className="flex items-center text-neutral-400 group-hover:text-teal-500 group-hover:translate-x-0.5 transition-all">
              <ChevronRight className="w-3.5 h-3.5" />
            </div>
          </button>
        }
      >
        <SubMenuScroller className="gap-1.5" containerClassName="w-full">
          {questTabs.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTabFilter(tab.id)}
              className={`px-3 py-1.5 rounded-lg text-xs font-medium whitespace-nowrap transition-colors cursor-pointer shrink-0 ${
                activeTabFilter === tab.id
                  ? 'bg-neutral-200 dark:bg-neutral-800 text-neutral-900 dark:text-white font-semibold'
                  : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200'
              }`}
            >
              {tab.label}
            </button>
          ))}
        </SubMenuScroller>
      </PageStickyNav>

      {/* Main Content Layout */}
      <div className="flex gap-6 items-start">
        {/* Left Desktop Sidebar: Workspaces & Projects */}
        <div className="w-52 shrink-0 border border-neutral-200 dark:border-neutral-800 rounded-xl bg-neutral-50/50 dark:bg-[#121315]/50 p-3 space-y-4 hidden lg:block select-none text-xs">
          {/* Workspaces */}
          <div className="space-y-1">
            <span className="text-[10px] font-semibold text-neutral-400 uppercase tracking-wider px-1">
              Workspaces
            </span>
            <div className="space-y-0.5">
              {workspaces.map((ws) => (
                <button
                  key={ws}
                  onClick={() => setSelectedWorkspace(ws)}
                  className={`w-full text-left px-2 py-1.5 rounded-md text-xs transition-colors flex items-center gap-2 ${
                    selectedWorkspace === ws
                      ? 'bg-neutral-200/80 dark:bg-neutral-800 text-teal-700 dark:text-teal-300 font-semibold'
                      : 'text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-900'
                  }`}
                >
                  <Folder className="w-3.5 h-3.5 shrink-0 opacity-60" />
                  <span className="truncate">{ws}</span>
                </button>
              ))}
            </div>
          </div>

          {/* Projects */}
          <div className="space-y-1">
            <span className="text-[10px] font-semibold text-neutral-400 uppercase tracking-wider px-1">
              Initiative Projects
            </span>
            <div className="space-y-0.5">
              {projects.map((proj) => (
                <button
                  key={proj}
                  onClick={() => setSelectedProject(proj)}
                  className={`w-full text-left px-2 py-1.5 rounded-md text-xs transition-colors flex items-center gap-2 ${
                    selectedProject === proj
                      ? 'bg-neutral-200/80 dark:bg-neutral-800 text-teal-700 dark:text-teal-300 font-semibold'
                      : 'text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-900'
                  }`}
                >
                  <Layers className="w-3.5 h-3.5 shrink-0 opacity-60" />
                  <span className="truncate">{proj}</span>
                </button>
              ))}
            </div>
          </div>

          {/* Reusable Maps Shortcut */}
          <div className="pt-2 border-t border-neutral-200 dark:border-neutral-800 space-y-1">
            <span className="text-[10px] font-semibold text-neutral-400 uppercase tracking-wider px-1">
              Saved SOP Maps
            </span>
            <div className="space-y-1 text-[11px] text-neutral-500 dark:text-neutral-400">
              <div className="px-2 py-1 rounded hover:bg-neutral-100 dark:hover:bg-neutral-900 cursor-pointer flex items-center justify-between">
                <span>Release Readiness</span>
                <span className="font-mono text-[9px]">v1.4</span>
              </div>
              <div className="px-2 py-1 rounded hover:bg-neutral-100 dark:hover:bg-neutral-900 cursor-pointer flex items-center justify-between">
                <span>CI Triage Protocol</span>
                <span className="font-mono text-[9px]">SOP</span>
              </div>
              <div className="px-2 py-1 rounded hover:bg-neutral-100 dark:hover:bg-neutral-900 cursor-pointer flex items-center justify-between">
                <span>Repo Health Audit</span>
                <span className="font-mono text-[9px]">Daily</span>
              </div>
            </div>
          </div>
        </div>

<<<<<<< HEAD
        {/* Center / Main Content Area */}
        <div className="flex-1 min-w-0 space-y-6">
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
=======
        {/* Center Canvas */}
        <div className="flex-1 flex flex-col overflow-hidden">
          {/* Sub-Tabs with < and > arrows */}
          <div className="px-3 sm:px-4 py-2 border-b border-neutral-200 dark:border-neutral-800 bg-white/60 dark:bg-[#141619]/60 flex items-center justify-between gap-2 overflow-x-auto shrink-0 scrollbar-none">
            <div className="flex items-center gap-1.5 min-w-0">
              {/* Prev tab arrow button (<) */}
              <button
                type="button"
                onClick={() => {
                  const currIdx = questTabs.findIndex((t) => t.id === activeTabFilter);
                  const prevIdx = (currIdx - 1 + questTabs.length) % questTabs.length;
                  setActiveTabFilter(questTabs[prevIdx].id);
                }}
                className="p-1 rounded-lg border border-neutral-200 dark:border-neutral-800 hover:border-teal-500/50 bg-white dark:bg-neutral-900 text-neutral-600 dark:text-neutral-400 hover:text-teal-600 dark:hover:text-teal-400 transition-colors cursor-pointer shrink-0"
                title="Previous Tab (<)"
                aria-label="Previous tab"
              >
                <ChevronLeft className="w-3.5 h-3.5" />
              </button>

              <div className="flex items-center gap-1 overflow-x-auto scrollbar-none">
                {questTabs.map((tab) => (
                  <button
                    key={tab.id}
                    onClick={() => setActiveTabFilter(tab.id)}
                    className={`px-2.5 sm:px-3 py-1.5 rounded-lg text-xs font-medium whitespace-nowrap transition-colors cursor-pointer shrink-0 ${
                      activeTabFilter === tab.id
                        ? 'bg-neutral-200 dark:bg-neutral-800 text-neutral-900 dark:text-white font-semibold'
                        : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200'
                    }`}
                  >
                    {tab.label}
                  </button>
                ))}
              </div>

              {/* Next tab arrow button (>) */}
              <button
                type="button"
                onClick={() => {
                  const currIdx = questTabs.findIndex((t) => t.id === activeTabFilter);
                  const nextIdx = (currIdx + 1) % questTabs.length;
                  setActiveTabFilter(questTabs[nextIdx].id);
                }}
                className="p-1 rounded-lg border border-neutral-200 dark:border-neutral-800 hover:border-teal-500/50 bg-white dark:bg-neutral-900 text-neutral-600 dark:text-neutral-400 hover:text-teal-600 dark:hover:text-teal-400 transition-colors cursor-pointer shrink-0"
                title="Next Tab (>)"
                aria-label="Next tab"
              >
                <ChevronRight className="w-3.5 h-3.5" />
              </button>
            </div>

            {/* Unified Map Studio Navigation Arrow (Modern & Minimalist) */}
            <button
              type="button"
              onClick={() => setMapStudioMode((prev) => (prev === 'guided' ? 'advanced' : 'guided'))}
              className="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white/90 dark:bg-neutral-900/90 text-[11px] font-medium text-neutral-700 dark:text-neutral-200 hover:border-teal-500/40 hover:text-teal-600 dark:hover:text-teal-400 transition-all cursor-pointer shadow-2xs group shrink-0"
              title={`Switch to ${mapStudioMode === 'guided' ? 'Advanced Studio (Nodes)' : 'Guided Map (Steps)'}`}
            >
              <Workflow className="w-3.5 h-3.5 text-teal-500 shrink-0" />
              <span className="font-semibold text-neutral-900 dark:text-neutral-100">
                {mapStudioMode === 'guided' ? 'Guided Map' : 'Advanced Studio'}
              </span>
              <div className="flex items-center text-neutral-400 group-hover:text-teal-500 group-hover:translate-x-0.5 transition-all">
                <ChevronRight className="w-3.5 h-3.5" />
              </div>
            </button>
          </div>

          {/* Quests Viewport */}
          <div
            onScroll={handleViewportScroll}
            className="flex-1 overflow-y-auto p-4 sm:p-6 space-y-4"
          >
            <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
>>>>>>> 2e922019e513ae8198c4bf6e1addb10f2c027867
              {filteredQuests.map((quest) => {
                const assignedShip = ships.find(
                  (s) => s.id === (quest.assignedShipId || quest.suggestedShipId)
                );
                const isSelected = selectedQuest?.id === quest.id;

                return (
                  <div
                    key={quest.id}
                    onClick={() => setSelectedQuestId(quest.id)}
                    className={`p-4 rounded-xl border bg-white dark:bg-[#191b1f] hover:border-teal-500/50 cursor-pointer transition-all space-y-3 shadow-xs ${
                      isSelected
                        ? 'border-teal-500 ring-1 ring-teal-500/30'
                        : 'border-neutral-200 dark:border-neutral-800'
                    }`}
                  >
                    <div className="flex items-start justify-between gap-2">
                      <div>
                        <h3 className="font-bold text-neutral-900 dark:text-neutral-100 text-sm">
                          {quest.title}
                        </h3>
                        <div className="text-[11px] text-neutral-400 font-mono mt-0.5">
                          {assignedShip?.name || 'Developer Ship'} · Priority: {(quest.priority || 'medium').toUpperCase()}
                        </div>
                      </div>
                      <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-neutral-100 dark:bg-neutral-800 uppercase font-semibold text-neutral-600 dark:text-neutral-300">
                        {(quest.status || 'ready').replace('_', ' ')}
                      </span>
                    </div>

                    <p className="text-xs text-neutral-600 dark:text-neutral-400 line-clamp-2 leading-relaxed">
                      {quest.objective}
                    </p>

                    {/* Map Progress Bar */}
                    <div className="space-y-1">
                      <div className="flex items-center justify-between text-[10px] font-mono text-neutral-400">
                        <span>Map Step Progression</span>
                        <span className="text-teal-600 dark:text-teal-400 font-bold">
                          {quest.activeVoyageProgress || 0}%
                        </span>
                      </div>
                      <div className="w-full bg-neutral-100 dark:bg-neutral-800 h-1.5 rounded-full overflow-hidden">
                        <div
                          className="bg-teal-500 h-full rounded-full transition-all duration-300"
                          style={{ width: `${quest.activeVoyageProgress || 0}%` }}
                        />
                      </div>
                    </div>

                    {/* Metadata & Actions */}
                    <div className="pt-2 border-t border-neutral-100 dark:border-neutral-800 flex items-center justify-between text-[11px] font-mono">
                      <span className="text-neutral-400">
                        {(quest.requiredArtifacts || []).length} Artifacts Required
                      </span>

                      {quest.status === 'ready' ? (
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            runQuestVoyage(quest.id);
                          }}
                          className="px-2.5 py-1 rounded bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-bold text-xs flex items-center gap-1 hover:opacity-90"
                        >
                          <Play className="w-3 h-3" />
                          <span>Set Sail</span>
                        </button>
                      ) : (
                        <span className="text-teal-600 dark:text-teal-400 font-medium">
                          ${quest.budgetLimitUSD.toFixed(2)} cap
                        </span>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>

            {/* Selected Quest Living Map Preview */}
            {selectedQuest && (
              <div className="mt-6 p-5 rounded-2xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-4">
                <div className="flex items-center justify-between border-b border-neutral-100 dark:border-neutral-800 pb-3">
                  <div>
                    <span className="text-xs font-semibold text-teal-600 dark:text-teal-400 uppercase tracking-wider font-mono">
                      {mapStudioMode === 'guided' ? 'Guided Map Steps' : 'Advanced Map Studio Graph'}
                    </span>
                    <h2 className="text-base font-bold text-neutral-900 dark:text-neutral-100 mt-0.5">
                      {selectedQuest.title}
                    </h2>
                  </div>
                  <div className="text-right font-mono text-xs text-neutral-400">
                    <span>Budget: ${(selectedQuest.budgetLimitUSD ?? 2.0).toFixed(2)}</span>
                  </div>
                </div>

                {mapStudioMode === 'guided' ? (
                  <div className="grid grid-cols-1 md:grid-cols-4 gap-3 text-xs">
                    {(selectedQuest.mapSteps || []).map((step) => (
                      <div
                        key={step.stepNumber}
                        className="p-3 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/60 dark:bg-neutral-900/40 space-y-2"
                      >
                        <div className="flex items-center justify-between">
                          <span className="font-bold text-teal-600 font-mono">
                            Step {step.stepNumber}
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
                        <p className="font-medium text-neutral-800 dark:text-neutral-200">
                          {step.title}
                        </p>
                      </div>
                    ))}
                  </div>
                ) : (
                  <div className="p-6 rounded-xl border border-dashed border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-950 text-center space-y-2">
                    <Workflow className="w-8 h-8 text-teal-500 mx-auto" />
                    <div className="text-xs font-bold text-neutral-900 dark:text-neutral-100">
                      Advanced Graph Canvas Mode
                    </div>
                    <p className="text-[11px] text-neutral-400 max-w-md mx-auto">
                      Visualizing conditional retry gates, AST parsing nodes, and cryptographic ActionDigest checkpoints.
                    </p>
                  </div>
                )}
              </div>
            )}
          </div>
        </div>

      {/* New Quest Modal */}
      {isNewQuestModalOpen && (
        <div
          onClick={() => setIsNewQuestModalOpen(false)}
          className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-xs cursor-pointer"
        >
          <div
            onClick={(e) => e.stopPropagation()}
            className="w-full max-w-lg rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] shadow-2xl p-5 space-y-4 cursor-default"
          >
            <div className="flex items-center justify-between border-b border-neutral-200 dark:border-neutral-800 pb-3">
              <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100">
                Create a Quest — Step-by-Step
              </h3>
            </div>

            <form onSubmit={handleCreateQuestSubmit} className="space-y-3.5 text-xs">
              <div>
                <label className="block text-neutral-700 dark:text-neutral-300 font-semibold mb-1">
                  1. Objective / Title
                </label>
                <input
                  type="text"
                  required
                  placeholder="e.g. Prepare Release v1.4 Package"
                  value={newTitle}
                  onChange={(e) => setNewTitle(e.target.value)}
                  className="w-full px-3 py-2 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-teal-500"
                />
              </div>

              <div>
                <label className="block text-neutral-700 dark:text-neutral-300 font-semibold mb-1">
                  2. Detailed Scope &amp; Target Deliverables
                </label>
                <textarea
                  rows={2}
                  placeholder="Describe required Artifacts, tests, and boundaries..."
                  value={newObjective}
                  onChange={(e) => setNewObjective(e.target.value)}
                  className="w-full px-3 py-2 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-teal-500"
                />
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="block text-neutral-700 dark:text-neutral-300 font-semibold mb-1">
                    3. Assigned Specialist Ship
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
                  4. Voyage Budget Hard Cap (USD)
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
                  className="px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 text-neutral-600 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800"
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  className="px-4 py-1.5 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-semibold hover:opacity-90"
                >
                  Confirm &amp; Set Sail
                </button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  );
};
