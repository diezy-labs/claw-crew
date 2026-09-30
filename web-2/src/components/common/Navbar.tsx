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
  Keyboard,
  HelpCircle,
  MessageSquare,
  LogOut,
  Palette,
  Menu,
  Activity,
  QrCode,
  Monitor
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';
import { SettingsCategory } from '../../types';
import { PWAInstallButton } from './PWAInstallButton';
import { TauriDesktopModal } from './TauriDesktopModal';
import { Modal } from './Modal';
import { Button } from './Button';

export interface NavbarProps {
  children?: React.ReactNode;
  showSidebarToggle?: boolean;
  showSearch?: boolean;
  showNotifications?: boolean;
  showRemoteAccess?: boolean;
  showFleetPulse?: boolean;
  showAccountMenu?: boolean;
  className?: string;
}

export const Navbar: React.FC<NavbarProps> = ({
  children,
  showSidebarToggle = true,
  showSearch = true,
  showNotifications = true,
  showRemoteAccess = true,
  showFleetPulse = true,
  showAccountMenu = true,
  className = ''
}) => {
  const {
    realmName,
    fleetName,
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
    toggleFleetPulse,
    setRemoteAccessModalOpen
  } = useFleetStore();

  const [isNotifOpen, setIsNotifOpen] = useState(false);
  const [isAccountMenuOpen, setIsAccountMenuOpen] = useState(false);
  const [isTauriModalOpen, setIsTauriModalOpen] = useState(false);
  const [showSignOutModal, setShowSignOutModal] = useState(false);
  const [showFeedbackModal, setShowFeedbackModal] = useState(false);
  const [feedbackText, setFeedbackText] = useState('');
  const [feedbackSent, setFeedbackSent] = useState(false);

  const accountMenuRef = useRef<HTMLDivElement>(null);
  const notifMenuRef = useRef<HTMLDivElement>(null);
  const unreadCount = notifications.filter((n) => !n.read).length;

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
      if (notifMenuRef.current && !notifMenuRef.current.contains(e.target as Node)) {
        setIsNotifOpen(false);
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
    }, 1500);
  };

  return (
    <header
      className={`h-14 border-b border-neutral-200 dark:border-neutral-800 bg-white/95 dark:bg-[#141619]/95 backdrop-blur-xs flex items-center justify-between px-3 sm:px-4 z-40 shrink-0 select-none ${className}`}
    >
      {/* Left side: Sidebar Toggle & Search Command Trigger */}
      <div className="flex items-center gap-2 sm:gap-3 min-w-0">
        {showSidebarToggle && (
          <button
            type="button"
            onClick={() => {
              if (window.innerWidth < 768) {
                setMobileSidebarOpen(true);
              } else {
                toggleSidebarCollapsed();
              }
            }}
            className="p-1.5 text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200 hover:bg-neutral-100 dark:hover:bg-neutral-800/80 rounded-lg transition-colors shrink-0 cursor-pointer"
            title="Toggle navigation sidebar (⌘B)"
            aria-label="Toggle navigation sidebar"
          >
            <Menu className="w-4 h-4" />
          </button>
        )}

        {showSearch && (
          <div className="w-48 sm:w-64 md:w-72">
            <button
              type="button"
              onClick={() => setCommandPaletteOpen(true)}
              className="w-full flex items-center justify-between px-3 py-1.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/80 dark:bg-neutral-900/60 text-neutral-500 dark:text-neutral-400 text-xs hover:border-teal-500/40 hover:text-neutral-700 dark:hover:text-neutral-200 transition-all cursor-pointer shadow-2xs group"
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
        )}
      </div>

      {/* Middle Slot: Clean & open (redundant sub-navigation removed, accepts optional custom children) */}
      {children && <div className="flex-1 mx-3 min-w-0">{children}</div>}

      {/* Right controls */}
      <div className="flex items-center gap-1.5 sm:gap-2 shrink-0">
        {/* Real-time Notifications Bell */}
        {showNotifications && (
          <div className="relative" ref={notifMenuRef}>
            <button
              type="button"
              onClick={() => setIsNotifOpen(!isNotifOpen)}
              className="p-1.5 rounded-lg text-neutral-600 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors relative cursor-pointer"
              title="Real-time notifications"
              aria-label="Real-time notifications"
            >
              <Bell className="w-4 h-4" />
              {unreadCount > 0 && (
                <span className="absolute top-1 right-1 w-2 h-2 rounded-full bg-teal-500 ring-2 ring-white dark:ring-neutral-950" />
              )}
            </button>

            {isNotifOpen && (
              <div className="fixed inset-x-3 top-14 sm:absolute sm:inset-auto sm:right-0 sm:top-full sm:mt-2 sm:w-96 rounded-2xl sm:rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] shadow-2xl z-50 p-3 sm:p-3.5 space-y-2 max-h-[calc(100vh-4.5rem)] flex flex-col animate-in fade-in zoom-in-95 duration-100">
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
                    type="button"
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
        )}

        {/* Remote Access QR Hub */}
        {showRemoteAccess && (
          <button
            type="button"
            onClick={() => setRemoteAccessModalOpen(true)}
            className="flex items-center gap-1.5 px-2 py-1.5 rounded-lg border border-teal-500/30 bg-teal-500/10 text-teal-700 dark:text-teal-300 hover:bg-teal-500/20 text-xs font-medium transition-colors cursor-pointer shadow-2xs"
            title="Remote Access QR Code & Multi-Platform Hub"
          >
            <QrCode className="w-3.5 h-3.5 text-teal-600 dark:text-teal-400" />
            <span className="hidden lg:inline font-mono text-[11px] font-semibold text-teal-600 dark:text-teal-300">
              QR Access
            </span>
          </button>
        )}

        {/* PWA In-App Install Prompt */}
        <PWAInstallButton />

        {/* Desktop Tauri v2 Preview Button */}
        <button
          type="button"
          onClick={() => setIsTauriModalOpen(true)}
          className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-900/60 text-neutral-600 dark:text-neutral-300 hover:text-teal-600 dark:hover:text-teal-400 hover:border-teal-500/40 text-xs font-medium transition-colors cursor-pointer shadow-2xs"
          title="Tauri v2 Desktop App Simulator & Project Scaffolding"
        >
          <Monitor className="w-3.5 h-3.5 text-teal-500" />
          <span className="hidden xl:inline font-mono text-[11px]">Desktop (Tauri v2)</span>
        </button>

        {/* Fleet Pulse Tray / Sidebar Toggle */}
        {showFleetPulse && (
          <button
            type="button"
            onClick={toggleFleetPulse}
            className={`flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-xs transition-colors cursor-pointer border ${
              isFleetPulseOpen
                ? 'border-teal-500/50 bg-teal-500/10 text-teal-600 dark:text-teal-400 font-semibold ring-1 ring-teal-500/30'
                : 'border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-900/60 text-neutral-600 dark:text-neutral-300 hover:text-teal-600 dark:hover:text-teal-400 hover:border-neutral-300 dark:hover:border-neutral-700'
            }`}
            title={isFleetPulseOpen ? 'Hide Fleet Pulse tray' : 'Show Fleet Pulse tray'}
            aria-label="Toggle Fleet Pulse tray"
          >
            <span className="relative flex h-2 w-2">
              <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-teal-400 opacity-75"></span>
              <span className="relative inline-flex rounded-full h-2 w-2 bg-teal-500"></span>
            </span>
            <Activity className="w-3.5 h-3.5" />
            <span className="font-mono text-[11px] font-medium hidden sm:inline">Fleet Pulse</span>
          </button>
        )}

        <div className="h-4 w-[1px] bg-neutral-200 dark:bg-neutral-800 mx-0.5" />

        {/* Persistent Top-Right Owner Account Trigger & Menu */}
        {showAccountMenu && (
          <div className="relative" ref={accountMenuRef}>
            <button
              type="button"
              onClick={() => setIsAccountMenuOpen(!isAccountMenuOpen)}
              aria-label="Open Pirate King account menu"
              title="Pirate King account"
              className="flex items-center gap-2 p-1 sm:px-2 sm:py-1 rounded-lg hover:bg-neutral-100 dark:hover:bg-neutral-800/80 transition-colors cursor-pointer group"
            >
              <div className="w-7 h-7 sm:w-8 sm:h-8 rounded-full bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-bold text-xs flex items-center justify-center shadow-xs shrink-0 ring-1 ring-neutral-300 dark:ring-neutral-700">
                AA
              </div>

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
                    type="button"
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
                    type="button"
                    onClick={() => {
                      setIsAccountMenuOpen(false);
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
                    type="button"
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
                    type="button"
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
                    type="button"
                    onClick={() => {
                      setIsAccountMenuOpen(false);
                      setIsTauriModalOpen(true);
                    }}
                    className="w-full px-4 py-1.5 text-left text-xs text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800/80 flex items-center justify-between transition-colors cursor-pointer"
                  >
                    <span className="flex items-center gap-2">
                      <Monitor className="w-3.5 h-3.5 text-teal-500" />
                      <span>Desktop App (Tauri v2)</span>
                    </span>
                    <span className="text-[10px] font-mono text-teal-600 dark:text-teal-400">Simulator</span>
                  </button>

                  <button
                    type="button"
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
                    type="button"
                    onClick={toggleTheme}
                    className="w-full px-4 py-1.5 text-left text-xs text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800/80 flex items-center justify-between transition-colors cursor-pointer"
                  >
                    <span className="flex items-center gap-2">
                      {theme === 'dark' ? (
                        <Sun className="w-3.5 h-3.5 text-amber-400" />
                      ) : (
                        <Moon className="w-3.5 h-3.5 text-neutral-700" />
                      )}
                      <span>Theme: {theme === 'dark' ? 'Dark' : 'Light'} Mode</span>
                    </span>
                    <span className="text-[10px] font-mono text-neutral-400">Toggle</span>
                  </button>

                  <button
                    type="button"
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
                    type="button"
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
                    type="button"
                    onClick={() => handleNavigateToSetting('about')}
                    className="w-full px-4 py-1.5 text-left text-xs text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800/80 flex items-center gap-2 transition-colors cursor-pointer"
                  >
                    <HelpCircle className="w-3.5 h-3.5 text-neutral-400" />
                    <span>Help &amp; Documentation</span>
                  </button>

                  <button
                    type="button"
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
                    type="button"
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
        )}
      </div>

      {/* Sign Out Confirmation Modal */}
      <Modal
        isOpen={showSignOutModal}
        onClose={() => setShowSignOutModal(false)}
        maxWidth="sm"
        title="Sign Out"
        footer={
          <div className="flex items-center justify-end gap-2 w-full">
            <Button
              variant="outline"
              size="sm"
              onClick={() => setShowSignOutModal(false)}
            >
              Cancel
            </Button>
            <Button
              variant="danger"
              size="sm"
              onClick={() => {
                setShowSignOutModal(false);
                setActiveTab('quarterdeck');
              }}
            >
              Sign Out
            </Button>
          </div>
        }
      >
        <div className="flex items-start gap-3 py-1">
          <div className="p-2.5 rounded-lg bg-rose-500/10 text-rose-500 shrink-0 mt-0.5">
            <LogOut className="w-5 h-5" />
          </div>
          <div>
            <h3 className="text-sm font-semibold text-neutral-900 dark:text-neutral-100">
              Sign out of Galleon Fleet?
            </h3>
            <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-1 leading-relaxed">
              Your local sessions and persistent Ship memory will remain securely encrypted on this machine.
            </p>
          </div>
        </div>
      </Modal>

      {/* Send Feedback Modal */}
      <Modal
        isOpen={showFeedbackModal}
        onClose={() => setShowFeedbackModal(false)}
        maxWidth="md"
        title={
          <div className="flex items-center gap-2">
            <MessageSquare className="w-4 h-4 text-teal-500" />
            <span>Send Feedback</span>
          </div>
        }
      >
        {feedbackSent ? (
          <div className="py-6 text-center text-xs text-emerald-600 dark:text-emerald-400 font-semibold space-y-1">
            <div className="w-8 h-8 rounded-full bg-emerald-500/10 text-emerald-500 flex items-center justify-center mx-auto mb-2">
              ✓
            </div>
            Thank you, Pirate King! Your feedback was logged.
          </div>
        ) : (
          <form onSubmit={handleFeedbackSubmit} className="space-y-3 pt-1">
            <p className="text-xs text-neutral-500 dark:text-neutral-400">
              Help us refine autonomous fleet orchestration, BYOK provider efficiency, and control room responsiveness.
            </p>
            <textarea
              rows={4}
              value={feedbackText}
              onChange={(e) => setFeedbackText(e.target.value)}
              placeholder="What would make Galleon Fleet work better for your team?"
              className="w-full p-2.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-teal-500"
            />
            <div className="flex justify-end gap-2 pt-2 border-t border-neutral-100 dark:border-neutral-800">
              <Button
                variant="outline"
                size="sm"
                onClick={() => setShowFeedbackModal(false)}
              >
                Cancel
              </Button>
              <Button
                variant="primary"
                size="sm"
                type="submit"
                disabled={!feedbackText.trim()}
              >
                Submit Feedback
              </Button>
            </div>
          </form>
        )}
      </Modal>

      {/* Tauri v2 Desktop Simulator & Scaffolding Modal */}
      <TauriDesktopModal
        isOpen={isTauriModalOpen}
        onClose={() => setIsTauriModalOpen(false)}
      />
    </header>
  );
};
