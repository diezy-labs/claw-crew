import React, { useState } from 'react';
import {
  Compass,
  LayoutGrid,
  FileText,
  ShieldAlert,
  Ship,
  Users,
  Coins,
  BookOpen,
  Anchor,
  Shield,
  Activity,
  Layers,
  OctagonAlert,
  Map,
  BookMarked,
  Settings,
  ChevronLeft,
  ChevronRight,
  ChevronDown,
  X,
  Play,
  CheckCircle2,
  AlertTriangle,
  User,
  Flag,
  Mic,
  Terminal,
  GraduationCap,
  ShieldCheck
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';
import { NavigationTab } from '../../types';
import { GalleonLogo } from '../common/GalleonLogo';

export const AppSidebar: React.FC = () => {
  const {
    activeTab,
    setActiveTab,
    approvals,
    isSidebarCollapsed,
    toggleSidebarCollapsed,
    isMobileSidebarOpen,
    setMobileSidebarOpen,
    isAnchorDropped,
    toggleAnchor
  } = useFleetStore();

  const [showAnchorModal, setShowAnchorModal] = useState(false);
  const [collapsedSections, setCollapsedSections] = useState<Record<string, boolean>>({});

  const toggleSectionCollapse = (sectionTitle: string) => {
    setCollapsedSections((prev) => ({
      ...prev,
      [sectionTitle]: !prev[sectionTitle]
    }));
  };

  const pendingApprovalsCount = approvals.filter((a) => a.status === 'pending').length;

  const navSections: {
    title: string;
    items: {
      id: NavigationTab;
      label: string;
      icon: React.ComponentType<{ className?: string }>;
      badge?: number;
      badgeColor?: string;
      hint?: string;
    }[];
  }[] = [
    {
      title: 'COMMAND',
      items: [
        { id: 'quarterdeck', label: 'Quarterdeck', icon: Compass, hint: 'AI Chat Hub · chat intake & delegation' },
        { id: 'realm', label: 'Realm', icon: Mic, hint: 'Voice Mode · 8-bit Quarterdeck conversation' },
        { id: 'flag-bridge', label: 'Flag Bridge', icon: Flag, hint: 'Quartermaster Control Room · see, steer & decide' },
        { id: 'quests', label: 'Quests', icon: Map, hint: 'Living SOPs & workflow maps' },
        { id: 'captains-journal', label: 'Captain’s Journal', icon: BookMarked, hint: 'Private working sessions & notes' }
      ]
    },
    {
      title: 'FLEET',
      items: [
        { id: 'mission-board', label: 'Mission Board', icon: LayoutGrid, hint: 'Global Kanban & work routing' },
        { id: 'ships', label: 'Ships', icon: Ship, hint: 'Department fleet vessels' },
        { id: 'squads', label: 'Squad', icon: ShieldCheck, hint: 'Cross-functional teams & squads' },
        { id: 'crew', label: 'Crew Members', icon: Users, hint: 'Specialist AI roster & squad mapping' },
        { id: 'artifacts', label: 'Artifacts', icon: FileText, hint: 'Reviewable deliverables & treasures' },
        {
          id: 'approvals',
          label: 'Captain’s Approval',
          icon: ShieldAlert,
          badge: pendingApprovalsCount,
          badgeColor: 'bg-amber-500/20 text-amber-500 dark:text-amber-400 border border-amber-500/30',
          hint: 'High-impact action review gate'
        }
      ]
    },
    {
      title: 'OPERATIONS',
      items: [
        { id: 'treasury', label: 'Treasury', icon: Coins, hint: 'BYOK cost tracking & token ledger' },
        { id: 'logbook', label: 'Logbook', icon: BookOpen, hint: 'Official immutable audit record' },
        { id: 'harbor', label: 'Harbor', icon: Anchor, hint: 'Model providers & connectors' },
        { id: 'training-officer', label: 'Training Officer', icon: GraduationCap, hint: 'Skills, steering directives & hooks' }
      ]
    },
    {
      title: 'CONTROL',
      items: [
        { id: 'fleet-code', label: 'Fleet Code', icon: Shield, hint: 'Policy engine & risk tiers' },
        { id: 'crows-nest', label: 'Crow’s Nest', icon: Activity, hint: 'Observability & gateway health' },
        { id: 'engine-room', label: 'Engine Room', icon: Terminal, hint: 'Local terminal, process monitor & services' },
        { id: 'shipyard', label: 'Shipyard', icon: Layers, hint: 'Fleet capacity & upgrades' }
      ]
    }
  ];

  const handleTabClick = (tabId: NavigationTab) => {
    setActiveTab(tabId);
    if (isMobileSidebarOpen) {
      setMobileSidebarOpen(false);
    }
  };

  const handleAnchorClick = () => {
    setShowAnchorModal(true);
  };

  const confirmToggleAnchor = () => {
    toggleAnchor();
    setShowAnchorModal(false);
  };

  return (
    <>
      {/* Mobile Backdrop Overlay */}
      {isMobileSidebarOpen && (
        <div
          className="fixed inset-0 bg-black/60 backdrop-blur-xs z-40 md:hidden"
          onClick={() => setMobileSidebarOpen(false)}
        />
      )}

      {/* Main Sidebar Container */}
      <aside
        className={`fixed inset-y-0 left-0 z-50 md:static flex flex-col justify-between select-none h-screen bg-white dark:bg-[#141619] border-r border-neutral-200 dark:border-neutral-800 transition-all duration-200 ease-in-out ${
          isMobileSidebarOpen ? 'translate-x-0 w-64 shadow-2xl' : '-translate-x-full md:translate-x-0'
        } ${isSidebarCollapsed ? 'md:w-16 overflow-x-hidden' : 'md:w-64'}`}
      >
        <div className="flex flex-col h-full overflow-hidden">
          {/* Brand header */}
          <div className="h-14 border-b border-neutral-200 dark:border-neutral-800 flex items-center justify-between px-3">
            <div className={`flex items-center gap-2.5 overflow-hidden ${isSidebarCollapsed ? 'mx-auto' : ''}`}>
              <div
                onClick={() => handleTabClick('quarterdeck')}
                className="w-8 h-8 rounded-lg overflow-hidden shrink-0 cursor-pointer hover:opacity-90 active:scale-95 transition-all shadow-xs"
                title="Galleon Fleet — Return to Quarterdeck"
              >
                <GalleonLogo className="w-full h-full" />
              </div>
              {!isSidebarCollapsed && (
                <div className="min-w-0">
                  <div className="text-sm font-semibold tracking-tight text-neutral-900 dark:text-neutral-100 flex items-center gap-1.5">
                    <span className="truncate">Galleon Fleet</span>
                    <span className="text-[10px] text-teal-600 dark:text-teal-400 font-mono px-1 py-0.2 rounded bg-teal-500/10 shrink-0">
                      v1.4
                    </span>
                  </div>
                  <div className="text-[11px] text-neutral-500 dark:text-neutral-400 truncate">
                    Agent Orchestration
                  </div>
                </div>
              )}
            </div>

            {/* Mobile Close Button only */}
            <div className="flex items-center gap-1 md:hidden">
              <button
                onClick={() => setMobileSidebarOpen(false)}
                className="p-1.5 text-neutral-400 hover:text-neutral-600 dark:hover:text-neutral-200 rounded"
                aria-label="Close sidebar"
              >
                <X className="w-4 h-4" />
              </button>
            </div>
          </div>

          {/* Navigation list (scrollbar hidden for clean visual) */}
          <div className="flex-1 overflow-y-auto overflow-x-hidden px-2 py-3 space-y-4 scrollbar-none">
            {navSections.map((section) => {
              const isCollapsible = section.title !== 'COMMAND';
              const isSectionCollapsed = isCollapsible && Boolean(collapsedSections[section.title]);

              return (
                <div key={section.title} className="space-y-1">
                  {!isSidebarCollapsed && (
                    <>
                      {isCollapsible ? (
                        <button
                          type="button"
                          onClick={() => toggleSectionCollapse(section.title)}
                          className="w-full flex items-center justify-between px-2 pb-1 text-[10px] font-semibold tracking-wider text-neutral-400 dark:text-neutral-500 uppercase hover:text-neutral-700 dark:hover:text-neutral-300 transition-colors cursor-pointer select-none group"
                          title={`Toggle ${section.title} section`}
                        >
                          <span>{section.title}</span>
                          <ChevronDown
                            className={`w-3 h-3 text-neutral-400 group-hover:text-neutral-600 dark:group-hover:text-neutral-300 transition-transform duration-200 ${
                              isSectionCollapsed ? '-rotate-90' : 'rotate-0'
                            }`}
                          />
                        </button>
                      ) : (
                        <div className="px-2 pb-1 text-[10px] font-semibold tracking-wider text-neutral-400 dark:text-neutral-500 uppercase">
                          {section.title}
                        </div>
                      )}
                    </>
                  )}
                  {isSidebarCollapsed && (
                    <div className="h-px bg-neutral-200 dark:bg-neutral-800 my-2 mx-1" />
                  )}
                  {(!isSectionCollapsed || isSidebarCollapsed) && (
                    <div className="space-y-0.5 animate-in fade-in duration-150">
                      {section.items.map((item) => {
                        const Icon = item.icon;
                        const isActive = activeTab === item.id || (item.id === 'quarterdeck' && activeTab === 'quartermaster');
                        return (
                          <button
                            key={item.id}
                            onClick={() => handleTabClick(item.id)}
                            title={isSidebarCollapsed ? `${item.label} — ${item.hint || ''}` : undefined}
                            style={
                              isActive
                                ? {
                                    backgroundColor: 'rgba(var(--brand-primary-rgb, 13, 148, 136), 0.16)',
                                    color: 'var(--brand-primary, #2dd4bf)'
                                  }
                                : undefined
                            }
                            className={`w-full flex items-center ${
                              isSidebarCollapsed ? 'justify-center px-0 py-2' : 'justify-between px-2.5 py-1.5'
                            } rounded-md text-xs font-medium transition-colors relative group ${
                              isActive
                                ? 'font-semibold'
                                : 'text-neutral-700 dark:text-neutral-300 hover:bg-neutral-50 dark:hover:bg-neutral-900 hover:text-neutral-950 dark:hover:text-white'
                            }`}
                          >
                            <div className={`flex items-center gap-2.5 ${isSidebarCollapsed ? 'justify-center' : 'truncate'}`}>
                              <span
                                style={isActive ? { color: 'var(--brand-primary, #2dd4bf)' } : undefined}
                                className="shrink-0 flex items-center justify-center"
                              >
                                <Icon
                                  className={`w-4 h-4 shrink-0 transition-colors ${
                                    isActive
                                      ? 'text-teal-600 dark:text-teal-400'
                                      : 'text-neutral-400 dark:text-neutral-500 group-hover:text-neutral-700 dark:group-hover:text-neutral-300'
                                  }`}
                                />
                              </span>
                              {!isSidebarCollapsed && <span className="truncate">{item.label}</span>}
                            </div>

                            {/* Badges */}
                            {item.badge !== undefined && item.badge > 0 && (
                              <>
                                {!isSidebarCollapsed ? (
                                  <span
                                    className={`text-[10px] px-1.5 py-0.2 rounded-full font-mono font-bold ${
                                      item.badgeColor || 'bg-neutral-200 dark:bg-neutral-700 text-neutral-800 dark:text-neutral-200'
                                    }`}
                                  >
                                    {item.badge}
                                  </span>
                                ) : (
                                  <span className="absolute top-1 right-1 w-2 h-2 rounded-full bg-amber-500 ring-2 ring-white dark:ring-[#141619]" />
                                )}
                              </>
                            )}
                          </button>
                        );
                      })}
                    </div>
                  )}
                </div>
              );
            })}
          </div>

          {/* Bottom Settings Link */}
          <div className="px-2 py-2 border-t border-neutral-200 dark:border-neutral-800">
            <button
              onClick={() => handleTabClick('settings')}
              title={isSidebarCollapsed ? 'Settings & Preferences (⌘,)' : undefined}
              className={`w-full flex items-center ${
                isSidebarCollapsed ? 'justify-center px-0 py-2' : 'justify-between px-2.5 py-1.5'
              } rounded-md text-xs font-medium transition-colors relative group ${
                activeTab === 'settings'
                  ? 'bg-neutral-100 dark:bg-neutral-800 text-teal-700 dark:text-teal-300 font-semibold'
                  : 'text-neutral-700 dark:text-neutral-300 hover:bg-neutral-50 dark:hover:bg-neutral-900 hover:text-neutral-950 dark:hover:text-white'
              }`}
            >
              <div className="flex items-center gap-2.5">
                <Settings
                  className={`w-4 h-4 shrink-0 ${
                    activeTab === 'settings'
                      ? 'text-teal-600 dark:text-teal-400'
                      : 'text-neutral-400 dark:text-neutral-500 group-hover:text-neutral-700 dark:group-hover:text-neutral-300'
                  }`}
                />
                {!isSidebarCollapsed && <span>Settings</span>}
              </div>
              {!isSidebarCollapsed && <span className="text-[10px] font-mono text-neutral-400">⌘,</span>}
            </button>
          </div>

          {/* Capacity status card & Drop Anchor */}
          <div className="p-2.5 border-t border-neutral-200 dark:border-neutral-800 bg-neutral-50/70 dark:bg-[#111214] space-y-2">
            {!isSidebarCollapsed ? (
              <>
                <div className="text-[11px] font-medium text-neutral-600 dark:text-neutral-300 flex items-center justify-between">
                  <span className="flex items-center gap-1.5">
                    <span
                      className={`w-1.5 h-1.5 rounded-full ${
                        isAnchorDropped ? 'bg-amber-500' : 'bg-emerald-500 animate-pulse'
                      }`}
                    />
                    <span>{isAnchorDropped ? 'Anchored (Paused)' : 'Community Tier'}</span>
                  </span>
                  <span className="font-mono text-[10px] text-neutral-500">1 / 3 Ships</span>
                </div>

                <div className="w-full bg-neutral-200 dark:bg-neutral-800 h-1.5 rounded-full overflow-hidden">
                  <div
                    style={!isAnchorDropped ? { backgroundColor: 'var(--brand-primary, #0d9488)' } : undefined}
                    className={`h-full rounded-full transition-all duration-300 ${
                      isAnchorDropped ? 'bg-amber-500 w-full' : 'w-1/3 shadow-2xs'
                    }`}
                  />
                </div>

                <div className="flex items-center justify-between text-[10px] text-neutral-500 dark:text-neutral-400 font-mono">
                  <span>Crew: 6/15</span>
                  <span>Voyages: {isAnchorDropped ? '0' : '2'}/8</span>
                </div>

                <button
                  onClick={handleAnchorClick}
                  className={`w-full flex items-center justify-center gap-1.5 py-1.5 px-2 rounded border text-[11px] font-medium transition-colors ${
                    isAnchorDropped
                      ? 'border-emerald-300 dark:border-emerald-900/50 bg-emerald-50 dark:bg-emerald-950/40 text-emerald-700 dark:text-emerald-400 hover:bg-emerald-100 dark:hover:bg-emerald-900/50'
                      : 'border-rose-300 dark:border-rose-900/50 bg-rose-50 dark:bg-rose-950/30 text-rose-700 dark:text-rose-400 hover:bg-rose-100 dark:hover:bg-rose-900/40'
                  }`}
                  title={
                    isAnchorDropped
                      ? 'Resume all background autonomous voyages'
                      : 'Emergency halt for all active background agent voyages'
                  }
                >
                  {isAnchorDropped ? (
                    <>
                      <Play className="w-3.5 h-3.5 fill-current" />
                      <span>Weigh Anchor (Resume)</span>
                    </>
                  ) : (
                    <>
                      <OctagonAlert className="w-3.5 h-3.5" />
                      <span>Drop Anchor (Pause All)</span>
                    </>
                  )}
                </button>
              </>
            ) : (
              <div className="flex flex-col items-center gap-2">
                <button
                  onClick={handleAnchorClick}
                  title={
                    isAnchorDropped
                      ? 'Weigh Anchor (Resume All Runs)'
                      : 'Drop Anchor (Emergency Pause All)'
                  }
                  className={`p-2 rounded-md transition-colors ${
                    isAnchorDropped
                      ? 'bg-emerald-500/10 text-emerald-500 hover:bg-emerald-500/20'
                      : 'bg-rose-500/10 text-rose-500 hover:bg-rose-500/20'
                  }`}
                >
                  {isAnchorDropped ? (
                    <Play className="w-4 h-4 fill-current" />
                  ) : (
                    <OctagonAlert className="w-4 h-4" />
                  )}
                </button>
              </div>
            )}
          </div>
        </div>
      </aside>

      {/* Drop Anchor Confirmation Modal (Pure in-app UI, no window.alert) */}
      {showAnchorModal && (
        <div
          onClick={() => setShowAnchorModal(false)}
          className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-xs animate-in fade-in duration-150 cursor-pointer"
        >
          <div
            onClick={(e) => e.stopPropagation()}
            className="w-full max-w-md rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#181a1d] shadow-2xl p-5 space-y-4 cursor-default"
          >
            <div className="flex items-start gap-3">
              <div
                className={`p-2.5 rounded-lg shrink-0 ${
                  isAnchorDropped
                    ? 'bg-emerald-500/10 text-emerald-500'
                    : 'bg-rose-500/10 text-rose-500'
                }`}
              >
                {isAnchorDropped ? (
                  <Play className="w-5 h-5 fill-current" />
                ) : (
                  <OctagonAlert className="w-5 h-5" />
                )}
              </div>
              <div className="space-y-1">
                <h3 className="text-sm font-semibold text-neutral-900 dark:text-neutral-100">
                  {isAnchorDropped ? 'Weigh Anchor (Resume Voyages)?' : 'Drop Anchor (Emergency Pause All)?'}
                </h3>
                <p className="text-xs text-neutral-500 dark:text-neutral-400 leading-relaxed">
                  {isAnchorDropped
                    ? 'This will resume background autonomous agent execution loops across all Ships. Active Quests will continue advancing their Map steps.'
                    : 'This will immediately halt all autonomous agent runs, cognitive worker goroutines, and scheduled SOP executions across your entire Fleet. No further model API calls will be dispatched.'}
                </p>
              </div>
            </div>

            <div className="p-3 rounded-lg bg-neutral-50 dark:bg-neutral-900/60 border border-neutral-200/80 dark:border-neutral-800 text-[11px] text-neutral-600 dark:text-neutral-400 space-y-1 font-mono">
              <div className="flex justify-between">
                <span>Active Ships:</span>
                <span className="font-semibold text-neutral-800 dark:text-neutral-200">1 / 3 Active</span>
              </div>
              <div className="flex justify-between">
                <span>In-flight Voyages:</span>
                <span className="font-semibold text-neutral-800 dark:text-neutral-200">2 voyages affected</span>
              </div>
              <div className="flex justify-between">
                <span>Audit Logbook:</span>
                <span className="text-teal-600 dark:text-teal-400">Action will be recorded with correlation ID</span>
              </div>
            </div>

            <div className="flex items-center justify-end gap-2 pt-2">
              <button
                onClick={() => setShowAnchorModal(false)}
                className="px-3 py-1.5 rounded-lg text-xs font-medium text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={confirmToggleAnchor}
                className={`px-4 py-1.5 rounded-lg text-xs font-semibold text-white shadow-sm transition-colors ${
                  isAnchorDropped
                    ? 'bg-emerald-600 hover:bg-emerald-500'
                    : 'bg-rose-600 hover:bg-rose-500'
                }`}
              >
                {isAnchorDropped ? 'Confirm Resume Operations' : 'Confirm Emergency Halt'}
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
};
