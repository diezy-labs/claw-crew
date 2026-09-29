import React, { useState } from 'react';
import {
  Settings,
  User,
  Palette,
  Bell,
  BookMarked,
  Sliders,
  Shield,
  Archive,
  Monitor,
  RefreshCw,
  Accessibility,
  Code2,
  FlaskConical,
  Info,
  Search,
  CheckCircle2,
  ExternalLink,
  Download,
  Upload,
  AlertTriangle,
  RotateCcw,
  Save,
  Lock,
  ArrowUpRight,
  FolderOpen
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';
import { SettingsCategory, TerminologyMode, DensityMode, UpdateChannel } from '../../types';

export const SettingsView: React.FC = () => {
  const {
    settings,
    activeSettingsCategory,
    setActiveSettingsCategory,
    updateSettings,
    resetSettingsCategory,
    setActiveTab,
    setTheme
  } = useFleetStore();

  const [searchQuery, setSearchQuery] = useState('');
  const [savedSuccessMsg, setSavedSuccessMsg] = useState<string | null>(null);

  const categories: {
    group: string;
    items: {
      id: SettingsCategory;
      label: string;
      icon: React.ComponentType<{ className?: string }>;
      subtitle: string;
    }[];
  }[] = [
    {
      group: 'PERSONAL',
      items: [
        { id: 'profile', label: 'Pirate King Profile', icon: User, subtitle: 'Identity, display name, avatar, preferred terminology' },
        { id: 'appearance', label: 'Appearance & Language', icon: Palette, subtitle: 'Theme, density, terminology mode, language' },
        { id: 'notifications', label: 'Notifications', icon: Bell, subtitle: 'Alert rules, approval reminders, quiet hours' },
        { id: 'journal', label: 'Captain’s Journal', icon: BookMarked, subtitle: 'Session retention, privacy defaults, search' }
      ]
    },
    {
      group: 'REALM',
      items: [
        { id: 'defaults', label: 'Defaults & Preferences', icon: Sliders, subtitle: 'Default Workspace, Project, routing, and planning' },
        { id: 'privacy', label: 'Privacy & Data', icon: Shield, subtitle: 'Telemetry, retention, secret redaction, local directory' },
        { id: 'backup', label: 'Backup, Export & Recovery', icon: Archive, subtitle: 'Realm export, backup policies, disaster recovery' }
      ]
    },
    {
      group: 'APPLICATION',
      items: [
        { id: 'runtime', label: 'Desktop & Runtime', icon: Monitor, subtitle: 'Startup, daemon, gateway address, background mode' },
        { id: 'updates', label: 'Updates', icon: RefreshCw, subtitle: 'Update channel, auto-check, release notes' },
        { id: 'accessibility', label: 'Accessibility & Shortcuts', icon: Accessibility, subtitle: 'Keyboard, text scale, focus indicators' }
      ]
    },
    {
      group: 'ADVANCED',
      items: [
        { id: 'developer', label: 'Developer Preferences', icon: Code2, subtitle: 'API/CLI, verbose logs, internal entity IDs' },
        { id: 'experimental', label: 'Experimental Features', icon: FlaskConical, subtitle: 'Opt-in preview capabilities & flags' },
        { id: 'about', label: 'About & Diagnostics', icon: Info, subtitle: 'Version, licensing, diagnostic bundle' }
      ]
    }
  ];

  const showSaveNotice = (msg: string) => {
    setSavedSuccessMsg(msg);
    setTimeout(() => setSavedSuccessMsg(null), 3000);
  };

  // Filter categories by search
  const filteredCategories = categories.map((catGroup) => ({
    ...catGroup,
    items: catGroup.items.filter(
      (item) =>
        item.label.toLowerCase().includes(searchQuery.toLowerCase()) ||
        item.subtitle.toLowerCase().includes(searchQuery.toLowerCase()) ||
        catGroup.group.toLowerCase().includes(searchQuery.toLowerCase())
    )
  })).filter((group) => group.items.length > 0);

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-hidden animate-view-fade-in">
      {/* Top Header */}
      <div className="p-4 sm:p-5 border-b border-neutral-200 dark:border-neutral-800 bg-white/40 dark:bg-[#141619]/40 backdrop-blur-xs flex flex-col sm:flex-row sm:items-center justify-between gap-3 shrink-0">
        <div>
          <div className="flex items-center gap-2">
            <Settings className="w-5 h-5 text-teal-600 dark:text-teal-400" />
            <h1 className="text-xl sm:text-2xl font-bold tracking-tight text-neutral-900 dark:text-neutral-100">
              Settings
            </h1>
            <span className="text-xs font-mono text-neutral-400">
              Preferences &amp; Application Controls
            </span>
          </div>
          <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
            Manage your personal preferences, data location, desktop runtime, and advanced configuration.
          </p>
        </div>

        {/* Search settings input */}
        <div className="flex items-center gap-2">
          <div className="relative">
            <Search className="w-3.5 h-3.5 absolute left-2.5 top-1/2 -translate-y-1/2 text-neutral-400" />
            <input
              type="text"
              placeholder="Search settings (e.g. theme, memory, backup)..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="pl-8 pr-3 py-1.5 text-xs rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-teal-500 w-64"
            />
          </div>
        </div>
      </div>

      {/* Main Settings Body */}
      <div className="flex-1 flex overflow-hidden">
        {/* Left Categories Sidebar */}
        <div className="w-64 shrink-0 border-r border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-[#121315]/50 overflow-y-auto p-3 space-y-4 select-none text-xs">
          {filteredCategories.map((group) => (
            <div key={group.group} className="space-y-1">
              <span className="text-[10px] font-semibold text-neutral-400 uppercase tracking-wider px-2">
                {group.group}
              </span>
              <div className="space-y-0.5">
                {group.items.map((item) => {
                  const Icon = item.icon;
                  const isActive = activeSettingsCategory === item.id;
                  return (
                    <button
                      key={item.id}
                      onClick={() => setActiveSettingsCategory(item.id)}
                      className={`w-full text-left px-2.5 py-1.5 rounded-lg transition-colors flex items-center gap-2.5 ${
                        isActive
                          ? 'bg-neutral-200/90 dark:bg-neutral-800 text-teal-700 dark:text-teal-300 font-semibold'
                          : 'text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-900 hover:text-neutral-900 dark:hover:text-white'
                      }`}
                    >
                      <Icon className={`w-3.5 h-3.5 shrink-0 ${isActive ? 'text-teal-600 dark:text-teal-400' : 'text-neutral-400'}`} />
                      <span className="truncate">{item.label}</span>
                    </button>
                  );
                })}
              </div>
            </div>
          ))}

          {/* Operational Boundaries Notice */}
          <div className="p-3 rounded-lg border border-neutral-200 dark:border-neutral-800/80 bg-neutral-100/40 dark:bg-neutral-900/30 text-[11px] text-neutral-500 dark:text-neutral-400 space-y-1">
            <span className="font-semibold text-neutral-700 dark:text-neutral-300 block">
              Operational Navigation
            </span>
            <p className="leading-relaxed">
              Looking for providers? Visit <button onClick={() => setActiveTab('harbor')} className="text-teal-600 hover:underline">Harbor</button>. For budgets, see <button onClick={() => setActiveTab('treasury')} className="text-teal-600 hover:underline">Treasury</button>. For policies, check <button onClick={() => setActiveTab('fleet-code')} className="text-teal-600 hover:underline">Fleet Code</button>.
            </p>
          </div>
        </div>

        {/* Right Active Category Content */}
        <div className="flex-1 overflow-y-auto p-5 sm:p-7 max-w-4xl space-y-6">
          {savedSuccessMsg && (
            <div className="p-3 rounded-lg bg-teal-500/10 border border-teal-500/30 text-teal-700 dark:text-teal-300 text-xs flex items-center justify-between animate-in fade-in">
              <span className="flex items-center gap-1.5 font-medium">
                <CheckCircle2 className="w-4 h-4 text-teal-500" />
                {savedSuccessMsg}
              </span>
              <span className="text-[10px] font-mono text-neutral-400">Logged to Logbook</span>
            </div>
          )}

          {/* 1. Pirate King Profile */}
          {activeSettingsCategory === 'profile' && (
            <div className="space-y-5 text-xs">
              <div>
                <h2 className="text-base font-bold text-neutral-900 dark:text-neutral-100">
                  Pirate King Profile
                </h2>
                <p className="text-neutral-500 dark:text-neutral-400 mt-0.5">
                  How your Realm and Quartermaster recognize you.
                </p>
              </div>

              <div className="p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-4">
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
                  <div>
                    <label className="block font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                      Display Name
                    </label>
                    <input
                      type="text"
                      value={settings.profile.displayName}
                      onChange={(e) =>
                        updateSettings((s) => ({
                          ...s,
                          profile: { ...s.profile, displayName: e.target.value }
                        }))
                      }
                      className="w-full px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-950 focus:outline-none"
                    />
                  </div>

                  <div>
                    <label className="block font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                      Narrative Title
                    </label>
                    <input
                      type="text"
                      value={settings.profile.narrativeTitle}
                      onChange={(e) =>
                        updateSettings((s) => ({
                          ...s,
                          profile: { ...s.profile, narrativeTitle: e.target.value }
                        }))
                      }
                      className="w-full px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-950 focus:outline-none"
                    />
                  </div>
                </div>

                <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
                  <div>
                    <label className="block font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                      Communication Style
                    </label>
                    <select
                      value={settings.profile.communicationStyle}
                      onChange={(e) =>
                        updateSettings((s) => ({
                          ...s,
                          profile: { ...s.profile, communicationStyle: e.target.value }
                        }))
                      }
                      className="w-full px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-950 focus:outline-none"
                    >
                      <option value="Clear and concise">Clear and concise</option>
                      <option value="Executive & Brief">Executive &amp; Brief</option>
                      <option value="Detailed technical">Detailed technical</option>
                    </select>
                  </div>

                  <div>
                    <label className="block font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                      Working Hours
                    </label>
                    <input
                      type="text"
                      value={settings.profile.workingHours}
                      onChange={(e) =>
                        updateSettings((s) => ({
                          ...s,
                          profile: { ...s.profile, workingHours: e.target.value }
                        }))
                      }
                      className="w-full px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-950 focus:outline-none font-mono"
                    />
                  </div>
                </div>

                <div className="pt-2 flex justify-end">
                  <button
                    onClick={() => showSaveNotice('Profile preferences updated')}
                    className="px-4 py-1.5 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-bold hover:opacity-90 flex items-center gap-1.5"
                  >
                    <Save className="w-3.5 h-3.5" />
                    <span>Save Profile</span>
                  </button>
                </div>
              </div>
            </div>
          )}

          {/* 2. Appearance & Language */}
          {activeSettingsCategory === 'appearance' && (
            <div className="space-y-5 text-xs">
              <div>
                <h2 className="text-base font-bold text-neutral-900 dark:text-neutral-100">
                  Appearance &amp; Language
                </h2>
                <p className="text-neutral-500 dark:text-neutral-400 mt-0.5">
                  Personalize how Fleet AI looks and speaks without altering core fleet behavior.
                </p>
              </div>

              <div className="p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-5">
                {/* Theme */}
                <div>
                  <label className="block font-semibold text-neutral-800 dark:text-neutral-200 mb-2">
                    Theme
                  </label>
                  <div className="flex gap-2">
                    {(['dark', 'light'] as const).map((t) => (
                      <button
                        key={t}
                        onClick={() => {
                          setTheme(t);
                          updateSettings((s) => ({
                            ...s,
                            appearance: { ...s.appearance, theme: t }
                          }));
                        }}
                        className={`px-4 py-2 rounded-lg border font-semibold capitalize ${
                          settings.appearance.theme === t
                            ? 'border-teal-500 bg-teal-500/10 text-teal-600 dark:text-teal-400'
                            : 'border-neutral-200 dark:border-neutral-800 text-neutral-600 dark:text-neutral-400'
                        }`}
                      >
                        {t} Mode
                      </button>
                    ))}
                  </div>
                </div>

                {/* Terminology Mode */}
                <div>
                  <label className="block font-semibold text-neutral-800 dark:text-neutral-200 mb-1">
                    Terminology Style
                  </label>
                  <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 mt-2">
                    <div
                      onClick={() =>
                        updateSettings((s) => ({
                          ...s,
                          appearance: { ...s.appearance, terminology: 'adventure' }
                        }))
                      }
                      className={`p-3 rounded-lg border cursor-pointer ${
                        settings.appearance.terminology === 'adventure'
                          ? 'border-teal-500 bg-teal-500/10 text-teal-700 dark:text-teal-300'
                          : 'border-neutral-200 dark:border-neutral-800'
                      }`}
                    >
                      <div className="font-bold">Adventure Mode (Maritime / Fleet)</div>
                      <div className="text-[11px] text-neutral-500 mt-0.5">
                        Quarterdeck, Quests, Ships, Logbook, Artifacts, Treasures.
                      </div>
                    </div>

                    <div
                      onClick={() =>
                        updateSettings((s) => ({
                          ...s,
                          appearance: { ...s.appearance, terminology: 'professional' }
                        }))
                      }
                      className={`p-3 rounded-lg border cursor-pointer ${
                        settings.appearance.terminology === 'professional'
                          ? 'border-teal-500 bg-teal-500/10 text-teal-700 dark:text-teal-300'
                          : 'border-neutral-200 dark:border-neutral-800'
                      }`}
                    >
                      <div className="font-bold">Professional Mode (Enterprise)</div>
                      <div className="text-[11px] text-neutral-500 mt-0.5">
                        Command Console, Workflows, Teams, Audit Log, Deliverables.
                      </div>
                    </div>
                  </div>
                </div>

                {/* Density */}
                <div>
                  <label className="block font-semibold text-neutral-800 dark:text-neutral-200 mb-1">
                    Interface Density
                  </label>
                  <div className="flex gap-2">
                    {(['comfortable', 'compact'] as const).map((d) => (
                      <button
                        key={d}
                        onClick={() =>
                          updateSettings((s) => ({
                            ...s,
                            appearance: { ...s.appearance, density: d }
                          }))
                        }
                        className={`px-3 py-1.5 rounded-lg border capitalize font-medium ${
                          settings.appearance.density === d
                            ? 'border-teal-500 text-teal-600 bg-teal-500/10'
                            : 'border-neutral-200 dark:border-neutral-800 text-neutral-500'
                        }`}
                      >
                        {d}
                      </button>
                    ))}
                  </div>
                </div>
              </div>
            </div>
          )}

          {/* 3. Notifications */}
          {activeSettingsCategory === 'notifications' && (
            <div className="space-y-5 text-xs">
              <div>
                <h2 className="text-base font-bold text-neutral-900 dark:text-neutral-100">
                  Notification Rules
                </h2>
                <p className="text-neutral-500 dark:text-neutral-400 mt-0.5">
                  Choose what should notify you without agent noise or notification fatigue.
                </p>
              </div>

              <div className="p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-4">
                <div className="space-y-2">
                  <span className="font-bold text-neutral-800 dark:text-neutral-200 block uppercase tracking-wider text-[11px]">
                    Owner Decisions &amp; Blockers
                  </span>
                  <label className="flex items-center gap-2 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={settings.notifications.captainApprovalRequested}
                      onChange={(e) =>
                        updateSettings((s) => ({
                          ...s,
                          notifications: { ...s.notifications, captainApprovalRequested: e.target.checked }
                        }))
                      }
                      className="rounded text-teal-600 focus:ring-teal-500"
                    />
                    <span>Captain’s Approval requested for external actions</span>
                  </label>

                  <label className="flex items-center gap-2 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={settings.notifications.strategicDecisionRequested}
                      onChange={(e) =>
                        updateSettings((s) => ({
                          ...s,
                          notifications: { ...s.notifications, strategicDecisionRequested: e.target.checked }
                        }))
                      }
                      className="rounded text-teal-600 focus:ring-teal-500"
                    />
                    <span>Strategic decision requested by Quartermaster</span>
                  </label>
                </div>

                <div className="pt-2 border-t border-neutral-100 dark:border-neutral-800 space-y-2">
                  <span className="font-bold text-neutral-800 dark:text-neutral-200 block uppercase tracking-wider text-[11px]">
                    Work Deliverables &amp; Health
                  </span>
                  <label className="flex items-center gap-2 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={settings.notifications.questCompleted}
                      onChange={(e) =>
                        updateSettings((s) => ({
                          ...s,
                          notifications: { ...s.notifications, questCompleted: e.target.checked }
                        }))
                      }
                      className="rounded text-teal-600 focus:ring-teal-500"
                    />
                    <span>Quest completed with Artifact ready for review</span>
                  </label>

                  <label className="flex items-center gap-2 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={settings.notifications.budgetSoftLimitReached}
                      onChange={(e) =>
                        updateSettings((s) => ({
                          ...s,
                          notifications: { ...s.notifications, budgetSoftLimitReached: e.target.checked }
                        }))
                      }
                      className="rounded text-teal-600 focus:ring-teal-500"
                    />
                    <span>Budget warning (Soft threshold reached in Treasury)</span>
                  </label>
                </div>
              </div>
            </div>
          )}

          {/* 4. Captain's Journal Settings */}
          {activeSettingsCategory === 'journal' && (
            <div className="space-y-5 text-xs">
              <div>
                <h2 className="text-base font-bold text-neutral-900 dark:text-neutral-100">
                  Captain’s Journal Preferences
                </h2>
                <p className="text-neutral-500 dark:text-neutral-400 mt-0.5">
                  Control how private conversations, drafts, and working sessions are retained.
                </p>
              </div>

              <div className="p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-4">
                <div>
                  <label className="block font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                    Learning Mode from Private Sessions
                  </label>
                  <select
                    value={settings.journal.learningMode}
                    onChange={(e) =>
                      updateSettings((s) => ({
                        ...s,
                        journal: { ...s.journal, learningMode: e.target.value as any }
                      }))
                    }
                    className="w-full px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-950 focus:outline-none"
                  >
                    <option value="ask">Ask before remembering anything</option>
                    <option value="propose">Allow Quartermaster to propose rules after useful edits</option>
                    <option value="never">Never create memory proposals from Journal</option>
                  </select>
                </div>

                <div>
                  <label className="block font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                    Session Retention
                  </label>
                  <input
                    type="text"
                    value={settings.journal.retentionActive}
                    onChange={(e) =>
                      updateSettings((s) => ({
                        ...s,
                        journal: { ...s.journal, retentionActive: e.target.value }
                      }))
                    }
                    className="w-full px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-950 focus:outline-none font-mono"
                  />
                </div>
              </div>
            </div>
          )}

          {/* 5. Realm Defaults & Preferences */}
          {activeSettingsCategory === 'defaults' && (
            <div className="space-y-5 text-xs">
              <div>
                <h2 className="text-base font-bold text-neutral-900 dark:text-neutral-100">
                  Realm Defaults &amp; Preferences
                </h2>
                <p className="text-neutral-500 dark:text-neutral-400 mt-0.5">
                  Set recommended defaults for new work created in your Realm.
                </p>
              </div>

              <div className="p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-4">
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
                  <div>
                    <label className="block font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                      Default Fleet
                    </label>
                    <input
                      type="text"
                      value={settings.defaults.defaultFleet}
                      onChange={(e) =>
                        updateSettings((s) => ({
                          ...s,
                          defaults: { ...s.defaults, defaultFleet: e.target.value }
                        }))
                      }
                      className="w-full px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-950 focus:outline-none"
                    />
                  </div>

                  <div>
                    <label className="block font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                      Default Planning Mode
                    </label>
                    <select
                      value={settings.defaults.defaultPlanningMode}
                      onChange={(e) =>
                        updateSettings((s) => ({
                          ...s,
                          defaults: { ...s.defaults, defaultPlanningMode: e.target.value as any }
                        }))
                      }
                      className="w-full px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-950 focus:outline-none"
                    >
                      <option value="guided">Guided Map (Linear Steps)</option>
                      <option value="advanced">Advanced Studio (Nodes Graph)</option>
                    </select>
                  </div>
                </div>
              </div>
            </div>
          )}

          {/* 6. Privacy & Data */}
          {activeSettingsCategory === 'privacy' && (
            <div className="space-y-5 text-xs">
              <div>
                <h2 className="text-base font-bold text-neutral-900 dark:text-neutral-100">
                  Privacy &amp; Data Ownership
                </h2>
                <p className="text-neutral-500 dark:text-neutral-400 mt-0.5">
                  Your Realm data stays locally under your command.
                </p>
              </div>

              <div className="p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-4">
                <div>
                  <label className="block font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                    Application Data Directory
                  </label>
                  <div className="flex items-center gap-2">
                    <input
                      type="text"
                      readOnly
                      value={settings.privacy.dataDirectory}
                      className="flex-1 px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-neutral-100 dark:bg-neutral-900 text-neutral-600 dark:text-neutral-400 font-mono text-[11px]"
                    />
                    <button
                      onClick={() => alert(`Data directory: ${settings.privacy.dataDirectory}`)}
                      className="px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 hover:bg-neutral-100 dark:hover:bg-neutral-800 font-medium"
                    >
                      Inspect
                    </button>
                  </div>
                </div>

                <div className="pt-2 border-t border-neutral-100 dark:border-neutral-800 space-y-2">
                  <span className="font-bold text-neutral-800 dark:text-neutral-200 block">
                    Zero-Leakage Telemetry Guarantees
                  </span>
                  <p className="text-neutral-500 dark:text-neutral-400 leading-relaxed text-[11px]">
                    Fleet AI never transmits API keys, secret credentials, source code files, or Journal entries to external servers. Model prompts are routed strictly to your configured BYOK endpoints.
                  </p>
                </div>
              </div>
            </div>
          )}

          {/* 7. Backup, Export & Recovery */}
          {activeSettingsCategory === 'backup' && (
            <div className="space-y-5 text-xs">
              <div>
                <h2 className="text-base font-bold text-neutral-900 dark:text-neutral-100">
                  Backup, Export &amp; Recovery
                </h2>
                <p className="text-neutral-500 dark:text-neutral-400 mt-0.5">
                  Protect and move your Realm without losing command of your Fleet.
                </p>
              </div>

              <div className="p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-4">
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                  <button
                    onClick={() => {
                      const exportObj = { timestamp: new Date().toISOString(), realm: 'Adiet Realm', backup: 'full' };
                      const blob = new Blob([JSON.stringify(exportObj, null, 2)], { type: 'application/json' });
                      const url = URL.createObjectURL(blob);
                      const a = document.createElement('a');
                      a.href = url;
                      a.download = `realm_backup_${Date.now()}.json`;
                      a.click();
                      showSaveNotice('Full Realm exported successfully');
                    }}
                    className="p-3.5 rounded-lg border border-neutral-200 dark:border-neutral-800 hover:border-teal-500/50 text-left space-y-1 transition-all"
                  >
                    <div className="font-bold text-neutral-900 dark:text-neutral-100 flex items-center gap-1.5">
                      <Download className="w-4 h-4 text-teal-500" />
                      <span>Export Full Realm</span>
                    </div>
                    <p className="text-[11px] text-neutral-500">
                      Includes Charters, Maps, Quests, Artifacts, and memory rules.
                    </p>
                  </button>

                  <button
                    onClick={() => setActiveTab('crows-nest')}
                    className="p-3.5 rounded-lg border border-neutral-200 dark:border-neutral-800 hover:border-amber-500/50 text-left space-y-1 transition-all"
                  >
                    <div className="font-bold text-neutral-900 dark:text-neutral-100 flex items-center gap-1.5">
                      <RotateCcw className="w-4 h-4 text-amber-500" />
                      <span>Open Recovery Center</span>
                    </div>
                    <p className="text-[11px] text-neutral-500">
                      Run automated Doctor diagnostics and state repair actions.
                    </p>
                  </button>
                </div>
              </div>
            </div>
          )}

          {/* 8. Desktop & Runtime */}
          {activeSettingsCategory === 'runtime' && (
            <div className="space-y-5 text-xs">
              <div>
                <h2 className="text-base font-bold text-neutral-900 dark:text-neutral-100">
                  Desktop &amp; Runtime Settings
                </h2>
                <p className="text-neutral-500 dark:text-neutral-400 mt-0.5">
                  Control how Fleet AI runs on this device.
                </p>
              </div>

              <div className="p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-4">
                <div className="grid grid-cols-2 gap-3 font-mono text-[11px]">
                  <div className="p-3 rounded-lg bg-neutral-50 dark:bg-neutral-950 border border-neutral-200 dark:border-neutral-800">
                    <span className="text-neutral-400 block text-[10px]">DAEMON GATEWAY</span>
                    <span className="font-bold text-emerald-500">HEALTHY (Port 8080)</span>
                  </div>
                  <div className="p-3 rounded-lg bg-neutral-50 dark:bg-neutral-950 border border-neutral-200 dark:border-neutral-800">
                    <span className="text-neutral-400 block text-[10px]">HOST SANDBOX</span>
                    <span className="font-bold text-teal-500">Landlock / Tauri OS Active</span>
                  </div>
                </div>

                <label className="flex items-center gap-2 cursor-pointer pt-2">
                  <input
                    type="checkbox"
                    checked={settings.runtime.keepRuntimeActiveOnClose}
                    onChange={(e) =>
                      updateSettings((s) => ({
                        ...s,
                        runtime: { ...s.runtime, keepRuntimeActiveOnClose: e.target.checked }
                      }))
                    }
                    className="rounded text-teal-600 focus:ring-teal-500"
                  />
                  <span>Keep Fleet runtime active in background when window closes</span>
                </label>
              </div>
            </div>
          )}

          {/* 9. Updates */}
          {activeSettingsCategory === 'updates' && (
            <div className="space-y-5 text-xs">
              <div>
                <h2 className="text-base font-bold text-neutral-900 dark:text-neutral-100">
                  Updates &amp; Channels
                </h2>
                <p className="text-neutral-500 dark:text-neutral-400 mt-0.5">
                  Keep Fleet AI reliable and verified with latest engine releases.
                </p>
              </div>

              <div className="p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-4">
                <div className="flex items-center justify-between">
                  <div>
                    <span className="font-bold text-sm text-neutral-900 dark:text-neutral-100">
                      Fleet AI v1.4.0-phase2
                    </span>
                    <div className="text-[11px] text-neutral-400 font-mono mt-0.5">
                      Last checked: {settings.updates.lastChecked}
                    </div>
                  </div>
                  <button
                    onClick={() => showSaveNotice('Checking for updates: Already on the latest version.')}
                    className="px-3 py-1.5 rounded-lg bg-neutral-900 dark:bg-neutral-100 text-white dark:text-neutral-900 font-semibold"
                  >
                    Check for Updates
                  </button>
                </div>
              </div>
            </div>
          )}

          {/* 10. Accessibility & Shortcuts */}
          {activeSettingsCategory === 'accessibility' && (
            <div className="space-y-5 text-xs">
              <div>
                <h2 className="text-base font-bold text-neutral-900 dark:text-neutral-100">
                  Accessibility &amp; Shortcuts
                </h2>
                <p className="text-neutral-500 dark:text-neutral-400 mt-0.5">
                  Make the Fleet command center comfortable for every operator.
                </p>
              </div>

              <div className="p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-3">
                <div className="space-y-2">
                  <label className="flex items-center gap-2 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={settings.accessibility.alwaysShowFocus}
                      onChange={(e) =>
                        updateSettings((s) => ({
                          ...s,
                          accessibility: { ...s.accessibility, alwaysShowFocus: e.target.checked }
                        }))
                      }
                      className="rounded text-teal-600 focus:ring-teal-500"
                    />
                    <span>Always show high-contrast visible focus rings</span>
                  </label>

                  <label className="flex items-center gap-2 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={settings.accessibility.confirmDestructiveActions}
                      onChange={(e) =>
                        updateSettings((s) => ({
                          ...s,
                          accessibility: { ...s.accessibility, confirmDestructiveActions: e.target.checked }
                        }))
                      }
                      className="rounded text-teal-600 focus:ring-teal-500"
                    />
                    <span>Confirm high-impact actions (Drop Anchor, Data Reset)</span>
                  </label>
                </div>
              </div>
            </div>
          )}

          {/* 11. Developer Preferences */}
          {activeSettingsCategory === 'developer' && (
            <div className="space-y-5 text-xs">
              <div>
                <h2 className="text-base font-bold text-neutral-900 dark:text-neutral-100">
                  Developer Preferences
                </h2>
                <p className="text-neutral-500 dark:text-neutral-400 mt-0.5">
                  Advanced controls for building, extending, and diagnosing your Fleet.
                </p>
              </div>

              <div className="p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-4">
                <label className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={settings.developer.developerMode}
                    onChange={(e) =>
                      updateSettings((s) => ({
                        ...s,
                        developer: { ...s.developer, developerMode: e.target.checked }
                      }))
                    }
                    className="rounded text-teal-600 focus:ring-teal-500"
                  />
                  <span className="font-semibold text-neutral-800 dark:text-neutral-200">
                    Enable Developer Mode (Exposes raw traces and manifest tools)
                  </span>
                </label>

                <div className="p-3 rounded-lg bg-neutral-50 dark:bg-neutral-950 border border-neutral-200 dark:border-neutral-800 space-y-1.5 font-mono text-[11px]">
                  <div className="text-neutral-400">Local JSON-RPC Endpoint:</div>
                  <div className="text-neutral-800 dark:text-neutral-200 font-bold">
                    http://127.0.0.1:8080/rpc
                  </div>
                </div>
              </div>
            </div>
          )}

          {/* 12. Experimental Features */}
          {activeSettingsCategory === 'experimental' && (
            <div className="space-y-5 text-xs">
              <div>
                <h2 className="text-base font-bold text-neutral-900 dark:text-neutral-100">
                  Experimental Features
                </h2>
                <p className="text-neutral-500 dark:text-neutral-400 mt-0.5">
                  Try early Fleet capabilities. These may change or be revised in future releases.
                </p>
              </div>

              <div className="p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-3">
                <label className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={settings.experimental.multiShipPreview}
                    onChange={(e) =>
                      updateSettings((s) => ({
                        ...s,
                        experimental: { ...s.experimental, multiShipPreview: e.target.checked }
                      }))
                    }
                    className="rounded text-teal-600 focus:ring-teal-500"
                  />
                  <span>Multi-Ship Cross-Quest Preview</span>
                </label>

                <label className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={settings.experimental.localModelAutoRouting}
                    onChange={(e) =>
                      updateSettings((s) => ({
                        ...s,
                        experimental: { ...s.experimental, localModelAutoRouting: e.target.checked }
                      }))
                    }
                    className="rounded text-teal-600 focus:ring-teal-500"
                  />
                  <span>Automated Cost-Saving Local Model Routing (Ollama / DeepSeek)</span>
                </label>
              </div>
            </div>
          )}

          {/* 13. About & Diagnostics */}
          {activeSettingsCategory === 'about' && (
            <div className="space-y-5 text-xs">
              <div>
                <h2 className="text-base font-bold text-neutral-900 dark:text-neutral-100">
                  About Fleet AI
                </h2>
                <p className="text-neutral-500 dark:text-neutral-400 mt-0.5">
                  Autonomous work, under your command.
                </p>
              </div>

              <div className="p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-4">
                <div className="grid grid-cols-2 gap-3 font-mono text-[11px]">
                  <div>
                    <span className="text-neutral-400 block text-[10px]">VERSION</span>
                    <span className="font-bold text-neutral-800 dark:text-neutral-200">v1.4.0 (Community)</span>
                  </div>
                  <div>
                    <span className="text-neutral-400 block text-[10px]">COGNITIVE ENGINE</span>
                    <span className="font-bold text-teal-600">Go 1.24 + Rust Sandboxing</span>
                  </div>
                </div>

                <div className="pt-2 border-t border-neutral-100 dark:border-neutral-800 flex items-center gap-3">
                  <button
                    onClick={() => setActiveTab('crows-nest')}
                    className="text-teal-600 dark:text-teal-400 hover:underline flex items-center gap-1 font-medium"
                  >
                    Open Crow’s Nest Diagnostics →
                  </button>
                  <button
                    onClick={() => setActiveTab('shipyard')}
                    className="text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200 flex items-center gap-1 font-medium"
                  >
                    View Shipyard Capacity →
                  </button>
                </div>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
