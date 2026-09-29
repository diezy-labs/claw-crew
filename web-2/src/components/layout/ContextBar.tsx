import React, { useState, useRef, useEffect } from 'react';
import {
  Search,
  Moon,
  Sun,
  Bell,
  ChevronRight,
  ChevronDown,
  CheckCheck,
  User,
  Shield,
  Layers,
  Sparkles,
  ExternalLink,
  Keyboard,
  HelpCircle,
  MessageSquare,
  LogOut,
  Palette,
  ShieldAlert,
  ArrowRight,
  Menu,
  Activity
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';
import { SettingsCategory, NavigationTab } from '../../types';
import { SubMenuScroller } from '../common/SubMenuScroller';

export const ContextBar: React.FC = () => {
  const {
    activeTab,
    realmName,
    fleetName,
    workspaces,
    projects,
    selectedWorkspace,
    selectedProject,
    setSelectedWorkspace,
    setSelectedProject,
    theme,
    toggleTheme,
    setCommandPaletteOpen,
    notifications,
    markNotificationRead,
    markAllNotificationsRead,
    setActiveTab,
    setActiveSettingsCategory,
    toggleSidebarCollapsed,
    setMobileSidebarOpen,
    isFleetPulseOpen,
    toggleFleetPulse
  } = useFleetStore();

  const [isNotifOpen, setIsNotifOpen] = useState(false);
  const [isAccountMenuOpen, setIsAccountMenuOpen] = useState(false);
  const [showSignOutModal, setShowSignOutModal] = useState(false);
  const [showFeedbackModal, setShowFeedbackModal] = useState(false);
  const [feedbackText, setFeedbackText] = useState('');
  const [feedbackSent, setFeedbackSent] = useState(false);

  const accountMenuRef = useRef<HTMLDivElement>(null);
  const unreadCount = notifications.filter((n) => !n.read).length;

  const subMenuSections: {
    category: string;
    items: { id: NavigationTab; label: string }[];
  }[] = [
    {
      category: 'COMMAND',
      items: [
        { id: 'quarterdeck', label: 'Quarterdeck' },
        { id: 'realm', label: 'Realm' },
        { id: 'flag-bridge', label: 'Flag Bridge' },
        { id: 'quests', label: 'Quests' },
        { id: 'captains-journal', label: "Captain's Journal" }
      ]
    },
    {
      category: 'FLEET',
      items: [
        { id: 'mission-board', label: 'Mission Board' },
        { id: 'ships', label: 'Ships' },
        { id: 'crew', label: 'Crew' },
        { id: 'artifacts', label: 'Artifacts' },
        { id: 'approvals', label: 'Approvals' }
      ]
    },
    {
      category: 'OPERATIONS',
      items: [
        { id: 'treasury', label: 'Treasury' },
        { id: 'logbook', label: 'Logbook' },
        { id: 'harbor', label: 'Harbor' }
      ]
    },
    {
      category: 'CONTROL',
      items: [
        { id: 'fleet-code', label: 'Fleet Code' },
        { id: 'crows-nest', label: "Crow's Nest" },
        { id: 'shipyard', label: 'Shipyard' },
        { id: 'settings', label: 'Settings' }
      ]
    }
  ];

  const currentSection = subMenuSections.find((sec) =>
    sec.items.some((item) => item.id === activeTab || (item.id === 'quarterdeck' && activeTab === 'quartermaster'))
  ) || subMenuSections[0];

  // Close menus on click outside or escape key
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setIsAccountMenuOpen(false);
        setIsNotifOpen(false);
      }
    };

    const handleClickOutside = (e: MouseEvent) => {
      if (accountMenuRef.current && !accountMenuRef.current.contains(e.target as Node)) {
        setIsAccountMenuOpen(false);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    document.addEventListener('mousedown', handleClickOutside);
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
      document.removeEventListener('mousedown', handleClickOutside);
    };
  }, []);

  const handleNavigateToSetting = (category: SettingsCategory) => {
    setActiveSettingsCategory(category);
    setActiveTab('settings');
    setIsAccountMenuOpen(false);
  };

  const handleFeedbackSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!feedbackText.trim()) return;
    setFeedbackSent(true);
    setTimeout(() => {
      setShowFeedbackModal(false);
      setFeedbackSent(false);
      setFeedbackText('');
    }, 1200);
  };

  return (
    <header className="h-14 border-b border-neutral-200 dark:border-neutral-800 bg-white/95 dark:bg-[#141619]/95 backdrop-blur-md px-3 sm:px-4 flex items-center justify-between sticky top-0 z-30 select-none">
      {/* Left: Sidebar Toggle & Search Fleet Bar right beside it */}
      <div className="flex items-center gap-2 sm:gap-3 min-w-0">
        {/* Sidebar Toggle Button */}
        <button
          onClick={() => {
            if (window.innerWidth < 768) {
              setMobileSidebarOpen(true);
            } else {
              toggleSidebarCollapsed();
            }
          }}
          className="p-1.5 text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200 hover:bg-neutral-100 dark:hover:bg-neutral-800/80 rounded transition-colors shrink-0 cursor-pointer"
          title="Toggle navigation sidebar (⌘B)"
          aria-label="Toggle navigation sidebar"
        >
          <Menu className="w-4 h-4" />
        </button>

        {/* Command palette search trigger (next to sidebar toggle, compact size) */}
        <div className="w-52 sm:w-72 md:w-80">
          <button
            onClick={() => setCommandPaletteOpen(true)}
            className="w-full flex items-center justify-between px-3 py-1.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/80 dark:bg-neutral-900/60 text-neutral-500 dark:text-neutral-400 text-xs hover:border-neutral-300 dark:hover:border-neutral-700 hover:text-neutral-700 dark:hover:text-neutral-200 transition-all cursor-pointer shadow-2xs group"
            title="Open Command Palette (⌘K)"
          >
            <div className="flex items-center gap-2 min-w-0">
              <Search className="w-3.5 h-3.5 group-hover:text-teal-500 transition-colors shrink-0" />
              <span className="truncate hidden sm:inline">Search Fleet...</span>
              <span className="truncate sm:hidden">Search...</span>
            </div>
            <kbd className="hidden sm:inline-flex items-center px-1.5 py-0.5 text-[10px] font-mono bg-neutral-200 dark:bg-neutral-800 rounded text-neutral-600 dark:text-neutral-400 shrink-0">
              ⌘K
            </kbd>
          </button>
        </div>
      </div>

      {/* Middle: Sub-Menu Scroller with '<' and '>' arrows that auto-hide at boundaries */}
      <div className="flex-1 max-w-xl mx-2 min-w-0 hidden md:block">
        <SubMenuScroller className="px-1 gap-1">
          {currentSection.items.map((item) => {
            const isActive = activeTab === item.id || (item.id === 'quarterdeck' && activeTab === 'quartermaster');
            return (
              <button
                key={item.id}
                onClick={() => setActiveTab(item.id)}
                className={`px-2.5 py-1 rounded-lg text-xs whitespace-nowrap transition-all cursor-pointer shrink-0 ${
                  isActive
                    ? 'bg-teal-500/15 text-teal-700 dark:text-teal-300 font-bold border border-teal-500/30 shadow-2xs'
                    : 'text-neutral-500 dark:text-neutral-400 hover:text-neutral-900 dark:hover:text-neutral-100 hover:bg-neutral-100 dark:hover:bg-neutral-800/60'
                }`}
              >
                {item.label}
              </button>
            );
          })}
        </SubMenuScroller>
      </div>

      {/* Right controls */}
      <div className="flex items-center gap-1.5 sm:gap-2 shrink-0">

        {/* Real-time Notifications Bell */}
        <div className="relative">
          <button
            onClick={() => setIsNotifOpen(!isNotifOpen)}
            className="p-1.5 rounded-lg text-neutral-600 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors relative cursor-pointer"
            title="Real-time notifications"
          >
            <Bell className="w-4 h-4" />
            {unreadCount > 0 && (
              <span className="absolute top-1 right-1 w-2 h-2 rounded-full bg-teal-500 ring-2 ring-white dark:ring-neutral-950" />
            )}
          </button>

          {/* Notifications Dropdown Popover */}
          {isNotifOpen && (
            <div className="absolute right-0 mt-2 w-80 sm:w-96 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] shadow-2xl z-50 p-3 space-y-2 animate-in fade-in duration-100">
              <div className="flex items-center justify-between pb-2 border-b border-neutral-100 dark:border-neutral-800">
                <div className="flex items-center gap-2">
                  <span className="text-xs font-semibold text-neutral-900 dark:text-neutral-100">
                    Real-time Signals
                  </span>
                  {unreadCount > 0 && (
                    <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-teal-500/20 text-teal-600 dark:text-teal-400">
                      {unreadCount} new
                    </span>
                  )}
                </div>
                <button
                  onClick={markAllNotificationsRead}
                  className="text-[11px] text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200 flex items-center gap-1 cursor-pointer"
                >
                  <CheckCheck className="w-3.5 h-3.5" />
                  Mark all read
                </button>
              </div>

              <div className="max-h-72 overflow-y-auto space-y-2 py-1">
                {notifications.length === 0 ? (
                  <div className="text-xs text-neutral-400 text-center py-4">No recent signals.</div>
                ) : (
                  notifications.map((notif) => (
                    <div
                      key={notif.id}
                      onClick={() => {
                        markNotificationRead(notif.id);
                        if (notif.actionLinkTab) {
                          setActiveTab(notif.actionLinkTab);
                          setIsNotifOpen(false);
                        }
                      }}
                      className={`p-2.5 rounded-lg text-xs transition-colors cursor-pointer border ${
                        notif.read
                          ? 'border-transparent bg-neutral-50/60 dark:bg-neutral-900/40 text-neutral-600 dark:text-neutral-400'
                          : 'border-teal-500/20 bg-teal-50/50 dark:bg-teal-950/20 text-neutral-900 dark:text-neutral-100'
                      }`}
                    >
                      <div className="flex items-center justify-between font-medium">
                        <span className="truncate">{notif.title}</span>
                        <span className="text-[10px] text-neutral-400 font-mono shrink-0 ml-2">
                          {notif.createdAt}
                        </span>
                      </div>
                      <p className="text-[11px] text-neutral-500 dark:text-neutral-400 mt-1 line-clamp-2">
                        {notif.description}
                      </p>
                    </div>
                  ))
                )}
              </div>
            </div>
          )}
        </div>

        {/* Fleet Pulse Tray / Sidebar Toggle */}
        <button
          onClick={toggleFleetPulse}
          className={`flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-xs transition-colors cursor-pointer border ${
            isFleetPulseOpen
              ? 'border-teal-500/50 bg-teal-500/10 text-teal-600 dark:text-teal-400 font-semibold ring-1 ring-teal-500/30'
              : 'border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-900/60 text-neutral-600 dark:text-neutral-300 hover:text-teal-600 dark:hover:text-teal-400 hover:border-neutral-300 dark:hover:border-neutral-700'
          }`}
          title={isFleetPulseOpen ? "Hide Fleet Pulse tray" : "Show Fleet Pulse tray"}
          aria-label="Toggle Fleet Pulse tray"
        >
          <span className="relative flex h-2 w-2">
            <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-teal-400 opacity-75"></span>
            <span className="relative inline-flex rounded-full h-2 w-2 bg-teal-500"></span>
          </span>
          <Activity className="w-3.5 h-3.5" />
          <span className="font-mono text-[11px] font-medium hidden sm:inline">Fleet Pulse</span>
        </button>

        <div className="h-4 w-[1px] bg-neutral-200 dark:border-neutral-800 mx-0.5" />

        {/* Persistent Top-Right Owner Account Trigger & Menu */}
        <div className="relative" ref={accountMenuRef}>
          <button
            onClick={() => setIsAccountMenuOpen(!isAccountMenuOpen)}
            aria-label="Open Pirate King account menu"
            title="Pirate King account"
            className="flex items-center gap-2 p-1 sm:px-2 sm:py-1 rounded-lg hover:bg-neutral-100 dark:hover:bg-neutral-800/80 transition-colors cursor-pointer group"
          >
            {/* Avatar / Initials: 28px in compact, 32px standard */}
            <div className="w-7 h-7 sm:w-8 sm:h-8 rounded-full bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-bold text-xs flex items-center justify-center shadow-xs shrink-0 ring-1 ring-neutral-300 dark:ring-neutral-700">
              AA
            </div>

            {/* Optional full trigger on wide desktop */}
            <div className="hidden lg:block text-left min-w-0">
              <div className="text-xs font-semibold text-neutral-900 dark:text-neutral-100 leading-none truncate">
                Adiet Alimudin
              </div>
              <div className="text-[10px] text-teal-600 dark:text-teal-400 font-mono leading-none mt-0.5">
                Pirate King
              </div>
            </div>

            <ChevronDown className="w-3.5 h-3.5 text-neutral-400 group-hover:text-neutral-600 dark:group-hover:text-neutral-200 transition-colors" />
          </button>

          {/* Account Dropdown Menu (Section 3.3) */}
          {isAccountMenuOpen && (
            <div className="absolute right-0 mt-2 w-72 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#181a1d] shadow-2xl z-50 py-2 space-y-1 animate-in fade-in duration-120">
              {/* Identity Header */}
              <div className="px-4 py-3 border-b border-neutral-100 dark:border-neutral-800/80 flex items-center gap-3">
                <div className="w-10 h-10 rounded-full bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-bold text-sm flex items-center justify-center shrink-0">
                  AA
                </div>
                <div className="min-w-0">
                  <div className="text-xs font-bold text-neutral-900 dark:text-neutral-100 truncate">
                    Adiet Alimudin
                  </div>
                  <div className="text-[11px] text-teal-600 dark:text-teal-400 font-medium">
                    Pirate King
                  </div>
                  <div className="text-[10px] text-neutral-400 font-mono truncate">
                    {realmName} &middot; {fleetName}
                  </div>
                </div>
              </div>

              {/* Group 1: Identity & Account */}
              <div className="py-1">
                <button
                  onClick={() => handleNavigateToSetting('profile')}
                  className="w-full px-4 py-2 text-left text-xs text-neutral-700 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800/80 flex items-center justify-between transition-colors cursor-pointer"
                >
                  <span className="flex items-center gap-2">
                    <User className="w-3.5 h-3.5 text-teal-500" />
                    <span>Account &amp; Profile</span>
                  </span>
                  <ChevronRight className="w-3.5 h-3.5 text-neutral-400" />
                </button>

                <button
                  onClick={() => {
                    setIsAccountMenuOpen(false);
                    // Open Realm switcher or settings defaults
                    handleNavigateToSetting('defaults');
                  }}
                  className="w-full px-4 py-2 text-left text-xs text-neutral-700 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800/80 flex items-center justify-between transition-colors cursor-pointer"
                >
                  <span className="flex items-center gap-2">
                    <Layers className="w-3.5 h-3.5 text-teal-500" />
                    <span>Realm &amp; Workspace</span>
                  </span>
                  <ChevronRight className="w-3.5 h-3.5 text-neutral-400" />
                </button>

                <button
                  onClick={() => {
                    setIsAccountMenuOpen(false);
                    setActiveTab('shipyard');
                  }}
                  className="w-full px-4 py-2 text-left text-xs text-neutral-700 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800/80 flex items-center justify-between transition-colors cursor-pointer"
                >
                  <span className="flex items-center gap-2">
                    <Sparkles className="w-3.5 h-3.5 text-amber-500" />
                    <span>Plan &amp; Billing</span>
                  </span>
                  <ChevronRight className="w-3.5 h-3.5 text-neutral-400" />
                </button>

                <button
                  onClick={() => handleNavigateToSetting('privacy')}
                  className="w-full px-4 py-2 text-left text-xs text-neutral-700 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800/80 flex items-center justify-between transition-colors cursor-pointer"
                >
                  <span className="flex items-center gap-2">
                    <Shield className="w-3.5 h-3.5 text-teal-500" />
                    <span>Security &amp; Devices</span>
                  </span>
                  <ChevronRight className="w-3.5 h-3.5 text-neutral-400" />
                </button>
              </div>

              <div className="h-px bg-neutral-100 dark:bg-neutral-800/80 my-1" />

              {/* Group 2: Preferences Shortcuts */}
              <div className="py-1">
                <button
                  onClick={() => handleNavigateToSetting('appearance')}
                  className="w-full px-4 py-1.5 text-left text-xs text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800/80 flex items-center justify-between transition-colors cursor-pointer"
                >
                  <span className="flex items-center gap-2">
                    <Palette className="w-3.5 h-3.5 text-neutral-400" />
                    <span>Appearance &amp; Language</span>
                  </span>
                  <ChevronRight className="w-3.5 h-3.5 text-neutral-400" />
                </button>

                <button
                  onClick={toggleTheme}
                  className="w-full px-4 py-1.5 text-left text-xs text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800/80 flex items-center justify-between transition-colors cursor-pointer"
                >
                  <span className="flex items-center gap-2">
                    {theme === 'dark' ? <Sun className="w-3.5 h-3.5 text-amber-400" /> : <Moon className="w-3.5 h-3.5 text-neutral-700" />}
                    <span>Theme: {theme === 'dark' ? 'Dark' : 'Light'} Mode</span>
                  </span>
                  <span className="text-[10px] font-mono text-neutral-400">Toggle</span>
                </button>

                <button
                  onClick={() => handleNavigateToSetting('notifications')}
                  className="w-full px-4 py-1.5 text-left text-xs text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800/80 flex items-center justify-between transition-colors cursor-pointer"
                >
                  <span className="flex items-center gap-2">
                    <Bell className="w-3.5 h-3.5 text-neutral-400" />
                    <span>Notification Preferences</span>
                  </span>
                  <ChevronRight className="w-3.5 h-3.5 text-neutral-400" />
                </button>

                <button
                  onClick={() => handleNavigateToSetting('accessibility')}
                  className="w-full px-4 py-1.5 text-left text-xs text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800/80 flex items-center justify-between transition-colors cursor-pointer"
                >
                  <span className="flex items-center gap-2">
                    <Keyboard className="w-3.5 h-3.5 text-neutral-400" />
                    <span>Keyboard Shortcuts</span>
                  </span>
                  <span className="text-[10px] font-mono text-neutral-400">⌘,</span>
                </button>
              </div>

              <div className="h-px bg-neutral-100 dark:bg-neutral-800/80 my-1" />

              {/* Group 3: Help & Feedback */}
              <div className="py-1">
                <button
                  onClick={() => handleNavigateToSetting('about')}
                  className="w-full px-4 py-1.5 text-left text-xs text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800/80 flex items-center gap-2 transition-colors cursor-pointer"
                >
                  <HelpCircle className="w-3.5 h-3.5 text-neutral-400" />
                  <span>Help &amp; Documentation</span>
                </button>

                <button
                  onClick={() => {
                    setIsAccountMenuOpen(false);
                    setShowFeedbackModal(true);
                  }}
                  className="w-full px-4 py-1.5 text-left text-xs text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800/80 flex items-center gap-2 transition-colors cursor-pointer"
                >
                  <MessageSquare className="w-3.5 h-3.5 text-neutral-400" />
                  <span>Send Feedback</span>
                </button>
              </div>

              <div className="h-px bg-neutral-100 dark:bg-neutral-800/80 my-1" />

              {/* Group 4: Sign out */}
              <div className="py-1">
                <button
                  onClick={() => {
                    setIsAccountMenuOpen(false);
                    setShowSignOutModal(true);
                  }}
                  className="w-full px-4 py-2 text-left text-xs text-rose-600 dark:text-rose-400 hover:bg-rose-50 dark:hover:bg-rose-950/30 flex items-center gap-2 font-medium transition-colors cursor-pointer"
                >
                  <LogOut className="w-3.5 h-3.5" />
                  <span>Sign out</span>
                </button>
              </div>
            </div>
          )}
        </div>
      </div>

      {/* Sign Out Confirmation Modal */}
      {showSignOutModal && (
        <div
          onClick={() => setShowSignOutModal(false)}
          className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-xs animate-in fade-in duration-150 cursor-pointer"
        >
          <div
            onClick={(e) => e.stopPropagation()}
            className="w-full max-w-sm rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#181a1d] shadow-2xl p-5 space-y-4 cursor-default"
          >
            <div className="flex items-center gap-3">
              <div className="p-2.5 rounded-lg bg-rose-500/10 text-rose-500">
                <LogOut className="w-5 h-5" />
              </div>
              <div>
                <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100">
                  Sign out of Fleet AI?
                </h3>
                <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
                  Your local sessions and persistent Ship memory will remain securely encrypted on this machine.
                </p>
              </div>
            </div>
            <div className="flex items-center justify-end gap-2 pt-2 border-t border-neutral-100 dark:border-neutral-800">
              <button
                onClick={() => setShowSignOutModal(false)}
                className="px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 text-xs font-medium text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800 cursor-pointer"
              >
                Cancel
              </button>
              <button
                onClick={() => {
                  setShowSignOutModal(false);
                  setActiveTab('quarterdeck');
                }}
                className="px-3.5 py-1.5 rounded-lg bg-rose-600 text-white font-bold text-xs hover:bg-rose-700 cursor-pointer"
              >
                Sign out
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Send Feedback Modal */}
      {showFeedbackModal && (
        <div
          onClick={() => setShowFeedbackModal(false)}
          className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-xs animate-in fade-in duration-150 cursor-pointer"
        >
          <div
            onClick={(e) => e.stopPropagation()}
            className="w-full max-w-md rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#181a1d] shadow-2xl p-5 space-y-4 cursor-default"
          >
            <div className="flex items-center justify-between border-b border-neutral-100 dark:border-neutral-800 pb-2">
              <div className="flex items-center gap-2">
                <MessageSquare className="w-4 h-4 text-teal-500" />
                <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100">
                  Send Fleet Feedback
                </h3>
              </div>
            </div>

            {feedbackSent ? (
              <div className="py-6 text-center text-xs text-emerald-600 dark:text-emerald-400 font-semibold space-y-1">
                <div className="w-8 h-8 rounded-full bg-emerald-500/10 text-emerald-500 flex items-center justify-center mx-auto mb-2">
                  ✓
                </div>
                Thank you, Pirate King! Your feedback was logged.
              </div>
            ) : (
              <form onSubmit={handleFeedbackSubmit} className="space-y-3">
                <p className="text-xs text-neutral-500 dark:text-neutral-400">
                  Help us refine autonomous fleet orchestration, BYOK provider efficiency, and control room responsiveness.
                </p>
                <textarea
                  rows={4}
                  value={feedbackText}
                  onChange={(e) => setFeedbackText(e.target.value)}
                  placeholder="What would make Fleet AI work better for your team?"
                  className="w-full p-2.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-teal-500"
                />
                <div className="flex justify-end gap-2">
                  <button
                    type="button"
                    onClick={() => setShowFeedbackModal(false)}
                    className="px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 text-xs font-medium text-neutral-600 dark:text-neutral-400"
                  >
                    Cancel
                  </button>
                  <button
                    type="submit"
                    disabled={!feedbackText.trim()}
                    className="px-3.5 py-1.5 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-bold text-xs hover:opacity-90 disabled:opacity-40 cursor-pointer"
                  >
                    Submit Feedback
                  </button>
                </div>
              </form>
            )}
          </div>
        </div>
      )}
    </header>
  );
};
