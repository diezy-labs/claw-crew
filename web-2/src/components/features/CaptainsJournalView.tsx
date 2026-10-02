import React, { useState, useRef } from 'react';
import {
  BookMarked,
  Search,
  PlusCircle,
  Pin,
  Archive,
  Lock,
  Send,
  Sparkles,
  FileText,
  Map,
  CheckCircle2,
  Trash2,
  HelpCircle,
  Folder,
  ChevronLeft,
  ChevronRight
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';
import { JournalSession } from '../../types';
import { PageHeader } from '../common/PageHeader';
import { Button } from '../common/Button';

export const CaptainsJournalView: React.FC = () => {
  const {
    journalSessions,
    selectedJournalSessionId,
    setSelectedJournalSessionId,
    createJournalSession,
    sendJournalMessage,
    togglePinJournalSession,
    archiveJournalSession,
    convertJournalToArtifact,
    convertJournalToQuest,
    selectedWorkspace
  } = useFleetStore();

  const [search, setSearch] = useState('');
  const [input, setInput] = useState('');
  const [newTitlePrompt, setNewTitlePrompt] = useState('');
  const [isCreating, setIsCreating] = useState(false);
  const [isSidebarOpen, setIsSidebarOpen] = useState(true);

  const activeSession =
    journalSessions.find((s) => s.id === selectedJournalSessionId) || journalSessions[0];

  const filteredSessions = journalSessions.filter((s) => {
    return (
      (s.title || '').toLowerCase().includes(search.toLowerCase()) ||
      (s.lastNote || '').toLowerCase().includes(search.toLowerCase())
    );
  });

  const pinnedSessions = filteredSessions.filter((s) => s.isPinned && !s.isArchived);
  const unpinnedSessions = filteredSessions.filter((s) => !s.isPinned && !s.isArchived);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [activeSession?.messages?.length, activeSession?.id]);

  const handleSend = (e?: React.FormEvent) => {
    if (e) e.preventDefault();
    if (!input.trim() || !activeSession) return;
    sendJournalMessage(activeSession.id, input.trim());
    setInput('');
  };

  const handleCreateNew = (e: React.FormEvent) => {
    e.preventDefault();
    if (!newTitlePrompt.trim()) return;
    createJournalSession(newTitlePrompt.trim(), selectedWorkspace);
    setNewTitlePrompt('');
    setIsCreating(false);
  };

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-hidden animate-view-fade-in">
      {/* Standard Reusable PageHeader - Solid & Non-looping */}
      <div className="px-4 py-3 sm:px-6 sm:py-3.5 border-b border-neutral-200 dark:border-neutral-800 bg-white/60 dark:bg-[#141619]/60 shrink-0">
        <PageHeader
          icon={<BookMarked className="w-4 h-4 text-teal-500 shrink-0" />}
          title="Captain’s Journal"
          badge={
            <span className="hidden sm:inline-flex text-xs font-mono text-teal-600 dark:text-teal-400 px-2 py-0.5 rounded bg-teal-500/10 items-center gap-1">
              <Lock className="w-3 h-3" />
              Private &amp; Exploratory
            </span>
          }
          description="Your private conversations, notes, exploratory thinking, and ongoing working sessions with Quartermaster."
          search={{
            value: search,
            onChange: setSearch,
            placeholder: 'Search journal entries...'
          }}
          actions={
            <Button
              variant="primary"
              size="sm"
              icon={<PlusCircle className="w-3.5 h-3.5" />}
              shortLabel="Entry"
              onClick={() => setIsCreating(true)}
            >
              + New Entry
            </Button>
          }
        />
      </div>

      {/* Main Container: Sidebar + Active Canvas */}
      <div className="flex-1 flex overflow-hidden">
        {/* Collapsed Minimalist Strip (< > arrow) */}
        {!isSidebarOpen && (
          <div className="w-9 shrink-0 border-r border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-[#121315]/50 flex flex-col items-center py-3 select-none">
            <button
              onClick={() => setIsSidebarOpen(true)}
              className="p-1.5 rounded-lg text-neutral-500 hover:text-teal-600 dark:hover:text-teal-400 hover:bg-neutral-200/60 dark:hover:bg-neutral-800 transition-colors cursor-pointer"
              title="Show Discussions (>)"
              aria-label="Show discussions"
            >
              <ChevronRight className="w-4 h-4" />
            </button>
          </div>
        )}

        {/* Left Sessions Sidebar */}
        {isSidebarOpen && (
          <div className="w-72 shrink-0 border-r border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-[#121315]/50 flex flex-col justify-between overflow-hidden select-none">
            {/* Header with Hide Button (<) */}
            <div className="px-3 py-2 border-b border-neutral-200/60 dark:border-neutral-800/60 flex items-center justify-between">
              <span className="text-[11px] font-semibold text-neutral-700 dark:text-neutral-300">
                Discussions
              </span>
              <button
                onClick={() => setIsSidebarOpen(false)}
                className="p-1 rounded-md text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200 hover:bg-neutral-200/60 dark:hover:bg-neutral-800 transition-colors cursor-pointer flex items-center gap-1 text-[10px]"
                title="Hide Discussions (<)"
                aria-label="Hide discussions"
              >
                <span>Hide</span>
                <ChevronLeft className="w-3.5 h-3.5" />
              </button>
            </div>

            <div className="flex-1 overflow-y-auto p-3 space-y-4">
            {isCreating && (
              <form
                onSubmit={handleCreateNew}
                className="p-3 rounded-lg border border-teal-500 bg-white dark:bg-neutral-900 space-y-2 text-xs"
              >
                <div className="font-semibold text-neutral-900 dark:text-neutral-100">
                  New Journal Topic
                </div>
                <input
                  type="text"
                  autoFocus
                  placeholder="e.g. Model benchmarks & release plan"
                  value={newTitlePrompt}
                  onChange={(e) => setNewTitlePrompt(e.target.value)}
                  className="w-full px-2 py-1.5 rounded border border-neutral-200 dark:border-neutral-700 bg-white dark:bg-neutral-950 text-xs focus:outline-none"
                />
                <div className="flex justify-end gap-1.5">
                  <button
                    type="button"
                    onClick={() => setIsCreating(false)}
                    className="px-2 py-1 text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200"
                  >
                    Cancel
                  </button>
                  <button
                    type="submit"
                    className="px-2.5 py-1 rounded bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-bold"
                  >
                    Open
                  </button>
                </div>
              </form>
            )}

            {/* Pinned Entries */}
            {pinnedSessions.length > 0 && (
              <div className="space-y-1">
                <span className="text-[10px] font-semibold text-neutral-400 uppercase tracking-wider px-1 flex items-center gap-1">
                  <Pin className="w-3 h-3 text-amber-500" />
                  Pinned Discussions
                </span>
                <div className="space-y-1">
                  {pinnedSessions.map((session) => {
                    const isSelected = activeSession?.id === session.id;
                    return (
                      <div
                        key={session.id}
                        onClick={() => setSelectedJournalSessionId(session.id)}
                        className={`p-2.5 rounded-lg border transition-all cursor-pointer text-xs space-y-1 ${
                          isSelected
                            ? 'border-teal-500 bg-white dark:bg-neutral-900 ring-1 ring-teal-500/20 shadow-xs'
                            : 'border-transparent hover:bg-neutral-100 dark:hover:bg-neutral-900/60'
                        }`}
                      >
                        <div className="flex items-center justify-between font-semibold text-neutral-900 dark:text-neutral-100">
                          <span className="truncate">{session.title}</span>
                          <span className="text-[10px] font-mono text-neutral-400 shrink-0 ml-1">
                            {(session.messages || []).length}
                          </span>
                        </div>
                        <p className="text-[11px] text-neutral-500 dark:text-neutral-400 truncate">
                          {session.lastNote || 'No notes yet'}
                        </p>
                      </div>
                    );
                  })}
                </div>
              </div>
            )}

            {/* Recent Working Sessions */}
            <div className="space-y-1">
              <span className="text-[10px] font-semibold text-neutral-400 uppercase tracking-wider px-1">
                Recent Sessions
              </span>
              <div className="space-y-1">
                {unpinnedSessions.map((session) => {
                  const isSelected = activeSession?.id === session.id;
                  return (
                    <div
                      key={session.id}
                      onClick={() => setSelectedJournalSessionId(session.id)}
                      className={`p-2.5 rounded-lg border transition-all cursor-pointer text-xs space-y-1 ${
                        isSelected
                          ? 'border-teal-500 bg-white dark:bg-neutral-900 ring-1 ring-teal-500/20 shadow-xs'
                          : 'border-transparent hover:bg-neutral-100 dark:hover:bg-neutral-900/60'
                      }`}
                    >
                      <div className="flex items-center justify-between font-semibold text-neutral-900 dark:text-neutral-100">
                        <span className="truncate">{session.title}</span>
                        <span className="text-[10px] font-mono text-neutral-400 shrink-0 ml-1">
                          {session.updatedAt || 'Recently'}
                        </span>
                      </div>
                      <p className="text-[11px] text-neutral-500 dark:text-neutral-400 truncate">
                        {session.lastNote || 'No notes yet'}
                      </p>
                    </div>
                  );
                })}
              </div>
            </div>
          </div>

          {/* Privacy Note Footer */}
          <div className="p-3 border-t border-neutral-200 dark:border-neutral-800 text-[11px] text-neutral-500 dark:text-neutral-400 bg-neutral-100/50 dark:bg-neutral-900/30">
            <span className="flex items-center gap-1 font-medium text-neutral-700 dark:text-neutral-300 mb-0.5">
              <Lock className="w-3 h-3 text-teal-500" />
              Journal vs. Logbook
            </span>
            Journal is private exploratory thinking with Quartermaster; Logbook is the official audit trail of fleet actions.
          </div>
        </div>
        )}

        {/* Right Active Journal Canvas */}
        {activeSession ? (
          <div className="flex-1 flex flex-col h-full overflow-hidden bg-white dark:bg-[#191b1f]">
            {/* Session Top Bar (Non-static, mobile responsive) */}
            <div className="px-3 sm:px-5 py-2.5 sm:py-3 border-b border-neutral-200 dark:border-neutral-800 flex flex-col sm:flex-row sm:items-center justify-between gap-2.5 shrink-0 bg-neutral-50/50 dark:bg-neutral-900/20">
              <div className="flex items-center gap-2 min-w-0">
                {!isSidebarOpen && (
                  <button
                    onClick={() => setIsSidebarOpen(true)}
                    className="p-1 sm:p-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 text-neutral-500 hover:text-teal-600 dark:hover:text-teal-400 hover:bg-neutral-100 dark:hover:bg-neutral-800 shrink-0 cursor-pointer"
                    title="Show Discussions (>)"
                  >
                    <ChevronRight className="w-3.5 h-3.5" />
                  </button>
                )}
                <div className="space-y-0.5 min-w-0">
                  <div className="flex items-center gap-2 flex-wrap">
                    <h2 className="text-xs sm:text-sm font-bold text-neutral-900 dark:text-neutral-100 truncate">
                      {activeSession.title}
                    </h2>
                    <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-neutral-100 dark:bg-neutral-800 text-neutral-500 shrink-0">
                      Workspace: {activeSession.workspaceId}
                    </span>
                  </div>
                  <div className="text-[10px] sm:text-[11px] text-neutral-400">
                    {(activeSession.messages || []).length} exchanges · {activeSession.updatedAt || 'Recently'}
                  </div>
                </div>
              </div>

              {/* Conversion and Pin actions */}
              <div className="flex items-center gap-1.5 sm:gap-2 flex-wrap self-end sm:self-auto">
                <Button
                  variant="outline"
                  size="xs"
                  icon={<FileText className="w-3.5 h-3.5 text-teal-500" />}
                  shortLabel="Artifact"
                  onClick={() => convertJournalToArtifact(activeSession.id)}
                  title="Promote these notes to a durable Artifact"
                >
                  Save Artifact
                </Button>

                <Button
                  variant="outline"
                  size="xs"
                  icon={<Map className="w-3.5 h-3.5 text-teal-500" />}
                  shortLabel="Quest"
                  onClick={() => convertJournalToQuest(activeSession.id)}
                  title="Turn this discussion into an operational Quest"
                >
                  Draft Quest
                </Button>

                <button
                  onClick={() => togglePinJournalSession(activeSession.id)}
                  className={`p-1.5 rounded text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200 cursor-pointer ${
                    activeSession.isPinned ? 'text-amber-500' : ''
                  }`}
                  title={activeSession.isPinned ? 'Unpin Entry' : 'Pin Entry'}
                >
                  <Pin className="w-4 h-4" />
                </button>
              </div>
            </div>

            {/* Conversation Messages */}
            <div className="flex-1 overflow-y-auto p-4 sm:p-5 space-y-4">
              {(activeSession.messages || []).map((msg) => {
                const isQM = msg.sender === 'quartermaster';
                return (
                  <div
                    key={msg.id}
                    className={`flex gap-3 text-xs leading-relaxed ${isQM ? '' : 'justify-end'}`}
                  >
                    {isQM && (
                      <div className="w-7 h-7 rounded-lg bg-teal-600/10 dark:bg-teal-500/20 text-teal-600 dark:text-teal-400 flex items-center justify-center shrink-0 font-bold font-mono text-xs">
                        QM
                      </div>
                    )}

                    <div className={`space-y-1 max-w-xl ${isQM ? '' : 'text-right'}`}>
                      <div
                        className={`inline-block p-3.5 rounded-xl text-left ${
                          isQM
                            ? 'bg-neutral-100/90 dark:bg-neutral-800/80 text-neutral-900 dark:text-neutral-100 border border-neutral-200 dark:border-neutral-700/60'
                            : 'bg-teal-600 text-white dark:bg-teal-500 dark:text-neutral-950 font-medium'
                        }`}
                      >
                        <div className="whitespace-pre-wrap">{msg.content}</div>
                      </div>
                      <div className="text-[10px] text-neutral-400 font-mono">
                        {msg.timestamp}
                      </div>
                    </div>

                    {!isQM && (
                      <div className="w-7 h-7 rounded-lg bg-neutral-200 dark:bg-neutral-800 text-neutral-700 dark:text-neutral-300 flex items-center justify-center shrink-0 font-bold font-mono text-xs">
                        PK
                      </div>
                    )}
                  </div>
                );
              })}
              <div ref={messagesEndRef} />
            </div>

            {/* Journal Composer */}
            <form
              onSubmit={handleSend}
              className="p-3 border-t border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-900/30 flex items-center gap-2 shrink-0"
            >
              <input
                type="text"
                value={input}
                onChange={(e) => setInput(e.target.value)}
                placeholder="Write private notes or consult with Quartermaster..."
                className="flex-1 bg-white dark:bg-neutral-950 border border-neutral-200 dark:border-neutral-800 rounded-lg px-3 py-2 text-xs text-neutral-900 dark:text-neutral-100 placeholder:text-neutral-400 focus:outline-none focus:ring-1 focus:ring-teal-500"
              />
              <button
                type="submit"
                disabled={!input.trim()}
                className="px-4 py-2 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 text-xs font-semibold hover:opacity-90 disabled:opacity-40 transition-opacity flex items-center gap-1.5"
              >
                <Send className="w-3.5 h-3.5" />
                <span>Note</span>
              </button>
            </form>
          </div>
        ) : (
          <div className="flex-1 flex items-center justify-center text-xs text-neutral-400">
            Select or create a Captain’s Journal session.
          </div>
        )}
      </div>
    </div>
  );
};
