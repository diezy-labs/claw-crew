import React, { useState, useEffect } from 'react';
import {
  Search,
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
  Sparkles,
  OctagonAlert,
  Moon,
  Sun,
  Settings,
  X
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';
import { NavigationTab } from '../../types';

export const CommandPalette: React.FC = () => {
  const {
    isCommandPaletteOpen,
    setCommandPaletteOpen,
    setActiveTab,
    toggleTheme,
    createQuest
  } = useFleetStore();

  const [query, setQuery] = useState('');

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault();
        setCommandPaletteOpen(!isCommandPaletteOpen);
      }
      if (e.key === 'Escape' && isCommandPaletteOpen) {
        setCommandPaletteOpen(false);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isCommandPaletteOpen, setCommandPaletteOpen]);

  if (!isCommandPaletteOpen) return null;

  const actions = [
    {
      id: 'qd',
      label: 'Quarterdeck (AI Chat Hub)',
      category: 'Command',
      icon: Compass,
      onSelect: () => setActiveTab('quarterdeck')
    },
    {
      id: 'flag-bridge',
      label: 'Flag Bridge (Quartermaster Control Room)',
      category: 'Command',
      icon: Compass,
      onSelect: () => setActiveTab('flag-bridge')
    },
    {
      id: 'quests',
      label: 'Quests (Workspace & Project Workflows)',
      category: 'Command',
      icon: LayoutGrid,
      onSelect: () => setActiveTab('quests')
    },
    {
      id: 'journal',
      label: 'Captain’s Journal (Private Conversations & Sessions)',
      category: 'Command',
      icon: FileText,
      onSelect: () => setActiveTab('captains-journal')
    },
    {
      id: 'mb',
      label: 'Open Mission Board (Global Work Queue)',
      category: 'Fleet',
      icon: LayoutGrid,
      onSelect: () => setActiveTab('mission-board')
    },
    {
      id: 'ships',
      label: 'Inspect Ships & Teams',
      category: 'Fleet',
      icon: Ship,
      onSelect: () => setActiveTab('ships')
    },
    {
      id: 'crew',
      label: 'Manage Crew Specialists',
      category: 'Fleet',
      icon: Users,
      onSelect: () => setActiveTab('crew')
    },
    {
      id: 'art',
      label: 'Browse Artifacts & Discoveries',
      category: 'Fleet',
      icon: FileText,
      onSelect: () => setActiveTab('artifacts')
    },
    {
      id: 'appr',
      label: 'Review Captain’s Approvals',
      category: 'Fleet',
      icon: ShieldAlert,
      onSelect: () => setActiveTab('approvals')
    },
    {
      id: 'squad-wizard',
      label: 'Make Me a Squad (Blueprint Wizard)',
      category: 'Fleet',
      icon: Sparkles,
      onSelect: () => setActiveTab('crew')
    },
    {
      id: 'treasury',
      label: 'Open Treasury (BYOK Costs & Budget)',
      category: 'Operations',
      icon: Coins,
      onSelect: () => setActiveTab('treasury')
    },
    {
      id: 'logbook',
      label: 'Audit Logbook & Activity Traces',
      category: 'Operations',
      icon: BookOpen,
      onSelect: () => setActiveTab('logbook')
    },
    {
      id: 'harbor',
      label: 'Configure Harbor Models & Integrations',
      category: 'Operations',
      icon: Anchor,
      onSelect: () => setActiveTab('harbor')
    },
    {
      id: 'fleet-code',
      label: 'Inspect Fleet Code Policies',
      category: 'Control',
      icon: Shield,
      onSelect: () => setActiveTab('fleet-code')
    },
    {
      id: 'crows-nest',
      label: 'Crow’s Nest Telemetry & Health',
      category: 'Control',
      icon: Activity,
      onSelect: () => setActiveTab('crows-nest')
    },
    {
      id: 'shipyard',
      label: 'Shipyard Capacity & Upgrades',
      category: 'Control',
      icon: Layers,
      onSelect: () => setActiveTab('shipyard')
    },
    {
      id: 'new-quest',
      label: 'Launch New Quest: Immediate Action',
      category: 'Actions',
      icon: LayoutGrid,
      onSelect: () => {
        createQuest({ title: 'Ad-hoc Mission Quest' });
        setActiveTab('mission-board');
      }
    },
    {
      id: 'settings',
      label: 'Open Settings & Preferences (⌘,)',
      category: 'Preferences',
      icon: Settings,
      onSelect: () => setActiveTab('settings')
    },
    {
      id: 'toggle-sidebar',
      label: 'Toggle Navigation Sidebar (⌘B)',
      category: 'Preferences',
      icon: LayoutGrid,
      onSelect: () => useFleetStore.getState().toggleSidebarCollapsed()
    },
    {
      id: 'toggle-theme',
      label: 'Toggle Dark / Light Mode',
      category: 'Preferences',
      icon: Sun,
      onSelect: () => toggleTheme()
    }
  ];

  const filtered = actions.filter(
    (a) =>
      a.label.toLowerCase().includes(query.toLowerCase()) ||
      a.category.toLowerCase().includes(query.toLowerCase())
  );

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center pt-20 px-4 bg-black/60 backdrop-blur-xs animate-in fade-in duration-150">
      <div className="w-full max-w-xl rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] shadow-2xl overflow-hidden">
        {/* Search header */}
        <div className="flex items-center px-4 py-3 border-b border-neutral-200 dark:border-neutral-800 gap-3">
          <Search className="w-4 h-4 text-neutral-400 shrink-0" />
          <input
            autoFocus
            type="text"
            placeholder="Type a command, screen, or action..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            className="w-full bg-transparent text-sm text-neutral-900 dark:text-neutral-100 placeholder:text-neutral-400 focus:outline-none"
          />
          <button
            onClick={() => setCommandPaletteOpen(false)}
            className="p-1 text-neutral-400 hover:text-neutral-600 dark:hover:text-neutral-200 rounded"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Results list */}
        <div className="max-h-80 overflow-y-auto p-2 space-y-1">
          {filtered.length === 0 ? (
            <div className="p-4 text-center text-xs text-neutral-400">
              No matching commands found.
            </div>
          ) : (
            filtered.map((action) => {
              const Icon = action.icon;
              return (
                <button
                  key={action.id}
                  onClick={() => {
                    action.onSelect();
                    setCommandPaletteOpen(false);
                  }}
                  className="w-full flex items-center justify-between px-3 py-2 rounded-lg text-xs hover:bg-neutral-100 dark:hover:bg-neutral-800/80 transition-colors text-left group"
                >
                  <div className="flex items-center gap-2.5">
                    <Icon className="w-4 h-4 text-neutral-400 group-hover:text-teal-500 transition-colors shrink-0" />
                    <span className="font-medium text-neutral-800 dark:text-neutral-200 group-hover:text-neutral-950 dark:group-hover:text-white">
                      {action.label}
                    </span>
                  </div>
                  <span className="text-[10px] font-mono text-neutral-400 uppercase tracking-wider">
                    {action.category}
                  </span>
                </button>
              );
            })
          )}
        </div>

        {/* Footer shortcuts */}
        <div className="px-4 py-2 border-t border-neutral-100 dark:border-neutral-800/80 bg-neutral-50/50 dark:bg-neutral-900/30 text-[11px] text-neutral-500 flex items-center justify-between">
          <div className="flex items-center gap-3">
            <span>
              <kbd className="px-1 py-0.5 rounded bg-neutral-200 dark:bg-neutral-800 font-mono text-[10px]">
                ↑↓
              </kbd>{' '}
              Navigate
            </span>
            <span>
              <kbd className="px-1 py-0.5 rounded bg-neutral-200 dark:bg-neutral-800 font-mono text-[10px]">
                ↵
              </kbd>{' '}
              Select
            </span>
            <span>
              <kbd className="px-1 py-0.5 rounded bg-neutral-200 dark:bg-neutral-800 font-mono text-[10px]">
                esc
              </kbd>{' '}
              Close
            </span>
          </div>
          <span className="font-mono text-[10px] text-teal-600 dark:text-teal-400">
            Fleet AI
          </span>
        </div>
      </div>
    </div>
  );
};
