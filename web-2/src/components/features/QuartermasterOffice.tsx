import React, { useState } from 'react';
import {
  Compass,
  Send,
  Sparkles,
  ShieldAlert,
  FileText,
  Ship,
  ArrowRight,
  PlusCircle,
  CheckCircle2,
  AlertTriangle,
  Coins,
  BookmarkPlus
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';
import { Artifact } from '../../types';

export const QuartermasterOffice: React.FC = () => {
  const {
    chatMessages,
    sendQuartermasterMessage,
    setActiveTab,
    createQuest,
    saveArtifact,
    artifacts,
    approvals,
    quests,
    setSelectedArtifactId
  } = useFleetStore();

  const [input, setInput] = useState('');

  const pendingApprovals = approvals.filter((a) => a.status === 'pending');
  const readyQuests = quests.filter((q) => q.status === 'ready' || q.status === 'underway');
  const recentArtifacts = artifacts.slice(0, 3);

  const handleSend = (e?: React.FormEvent) => {
    if (e) e.preventDefault();
    if (!input.trim()) return;
    sendQuartermasterMessage(input.trim());
    setInput('');
  };

  const handleQuickStart = (promptText: string) => {
    sendQuartermasterMessage(promptText);
  };

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto p-4 sm:p-6 space-y-6 max-w-6xl mx-auto w-full animate-view-fade-in">
      {/* Executive Welcome & Context */}
      <div className="space-y-1">
        <div className="flex items-center gap-2 text-xs font-mono text-teal-600 dark:text-teal-400">
          <Compass className="w-4 h-4 text-teal-500 animate-spin-slow" />
          <span>QUARTERMASTER OFFICE — EXECUTIVE DESK</span>
        </div>
        <h1 className="text-2xl sm:text-3xl font-bold tracking-tight text-neutral-900 dark:text-neutral-100">
          Good morning, Pirate King.
        </h1>
        <p className="text-sm text-neutral-500 dark:text-neutral-400 max-w-2xl">
          What should your Fleet accomplish next? I monitor all 3 Ships, track BYOK provider health, route Quests, and bring critical decisions directly to you.
        </p>
      </div>

      {/* Suggested Quick Starts */}
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
        <button
          onClick={() => handleQuickStart('Prepare release readiness package for v1.4')}
          className="p-3.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] hover:border-teal-500/50 hover:shadow-sm transition-all text-left group"
        >
          <div className="flex items-center justify-between text-xs font-semibold text-neutral-900 dark:text-neutral-100 group-hover:text-teal-600 dark:group-hover:text-teal-400">
            <span>Prepare Release v1.4</span>
            <ArrowRight className="w-3.5 h-3.5 opacity-60 group-hover:opacity-100 transition-opacity" />
          </div>
          <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-1 line-clamp-2">
            Audit PR changelog, verify test suite, and synthesize Go/No-Go brief.
          </p>
        </button>

        <button
          onClick={() => handleQuickStart('Triage the integration test timeout bug on the Developer Ship')}
          className="p-3.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] hover:border-teal-500/50 hover:shadow-sm transition-all text-left group"
        >
          <div className="flex items-center justify-between text-xs font-semibold text-neutral-900 dark:text-neutral-100 group-hover:text-teal-600 dark:group-hover:text-teal-400">
            <span>Triage CI Teardown Hang</span>
            <ArrowRight className="w-3.5 h-3.5 opacity-60 group-hover:opacity-100 transition-opacity" />
          </div>
          <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-1 line-clamp-2">
            Investigate goroutine leak in WebSocket listener and review GitHub draft.
          </p>
        </button>

        <button
          onClick={() => handleQuickStart('Run comprehensive repository health check across clean architecture')}
          className="p-3.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] hover:border-teal-500/50 hover:shadow-sm transition-all text-left group"
        >
          <div className="flex items-center justify-between text-xs font-semibold text-neutral-900 dark:text-neutral-100 group-hover:text-teal-600 dark:group-hover:text-teal-400">
            <span>Repository Health Audit</span>
            <ArrowRight className="w-3.5 h-3.5 opacity-60 group-hover:opacity-100 transition-opacity" />
          </div>
          <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-1 line-clamp-2">
            Check package boundaries, Tauri Landlock rules, and dependency drift.
          </p>
        </button>
      </div>

      {/* Executive Briefing Cards */}
      <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/70 dark:bg-neutral-900/40 space-y-3">
        <div className="flex items-center justify-between">
          <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
            Executive Briefing Summary
          </span>
          <span className="text-[11px] font-mono text-neutral-400">
            Updated just now
          </span>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
          <div className="flex items-start gap-2.5 p-3 rounded-lg bg-white dark:bg-[#191b1f] border border-neutral-200 dark:border-neutral-800">
            <AlertTriangle className="w-4 h-4 text-amber-500 shrink-0 mt-0.5" />
            <div>
              <div className="text-xs font-medium text-neutral-900 dark:text-neutral-100">
                1 Release Blocker
              </div>
              <div className="text-[11px] text-neutral-500 dark:text-neutral-400 mt-0.5">
                CI timeout recurred 3x. Draft GitHub issue awaiting approval.
              </div>
              <button
                onClick={() => setActiveTab('approvals')}
                className="mt-1.5 text-[11px] font-medium text-teal-600 dark:text-teal-400 hover:underline flex items-center gap-1"
              >
                Review Approval →
              </button>
            </div>
          </div>

          <div className="flex items-start gap-2.5 p-3 rounded-lg bg-white dark:bg-[#191b1f] border border-neutral-200 dark:border-neutral-800">
            <CheckCircle2 className="w-4 h-4 text-emerald-500 shrink-0 mt-0.5" />
            <div>
              <div className="text-xs font-medium text-neutral-900 dark:text-neutral-100">
                3 Artifacts Ready
              </div>
              <div className="text-[11px] text-neutral-500 dark:text-neutral-400 mt-0.5">
                Repository Health Brief was validated and promoted to Treasure.
              </div>
              <button
                onClick={() => setActiveTab('artifacts')}
                className="mt-1.5 text-[11px] font-medium text-teal-600 dark:text-teal-400 hover:underline flex items-center gap-1"
              >
                Inspect Artifacts →
              </button>
            </div>
          </div>

          <div className="flex items-start gap-2.5 p-3 rounded-lg bg-white dark:bg-[#191b1f] border border-neutral-200 dark:border-neutral-800">
            <Coins className="w-4 h-4 text-teal-500 shrink-0 mt-0.5" />
            <div>
              <div className="text-xs font-medium text-neutral-900 dark:text-neutral-100">
                Budget Within Limits
              </div>
              <div className="text-[11px] text-neutral-500 dark:text-neutral-400 mt-0.5">
                $4.10 spent of $25.00 monthly cap (16.4%). Model routing optimal.
              </div>
              <button
                onClick={() => setActiveTab('treasury')}
                className="mt-1.5 text-[11px] font-medium text-teal-600 dark:text-teal-400 hover:underline flex items-center gap-1"
              >
                Open Treasury →
              </button>
            </div>
          </div>
        </div>
      </div>

      {/* Main Conversation Canvas */}
      <div className="rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] flex flex-col h-[460px] shadow-xs">
        {/* Chat header */}
        <div className="px-4 py-2.5 border-b border-neutral-200 dark:border-neutral-800 flex items-center justify-between bg-neutral-50/50 dark:bg-neutral-900/30">
          <div className="flex items-center gap-2">
            <div className="w-2 h-2 rounded-full bg-teal-500 animate-pulse" />
            <span className="text-xs font-semibold text-neutral-800 dark:text-neutral-200">
              Quartermaster Console
            </span>
            <span className="text-[11px] text-neutral-400">
              · Fleet-level coordination
            </span>
          </div>
          <span className="text-[10px] font-mono text-neutral-400">
            Shortcuts: Type /quest or describe goals
          </span>
        </div>

        {/* Message stream */}
        <div className="flex-1 overflow-y-auto p-4 space-y-4">
          {chatMessages.map((msg) => {
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

                <div className={`space-y-2 max-w-xl ${isQM ? '' : 'text-right'}`}>
                  <div
                    className={`inline-block p-3.5 rounded-xl text-left ${
                      isQM
                        ? 'bg-neutral-100/90 dark:bg-neutral-800/80 text-neutral-900 dark:text-neutral-100 border border-neutral-200 dark:border-neutral-700/60'
                        : 'bg-teal-600 text-white dark:bg-teal-500 dark:text-neutral-950 font-medium'
                    }`}
                  >
                    <div className="whitespace-pre-wrap">{msg.content}</div>

                    {/* Generated Artifact Preview Card */}
                    {msg.generatedArtifactPreview && (
                      <div className="mt-3 p-2.5 rounded-lg border border-neutral-300 dark:border-neutral-700 bg-white/60 dark:bg-neutral-950/60 space-y-1.5">
                        <div className="flex items-center justify-between text-[11px] font-semibold text-neutral-800 dark:text-neutral-200">
                          <span className="flex items-center gap-1.5">
                            <FileText className="w-3.5 h-3.5 text-teal-500" />
                            {msg.generatedArtifactPreview.title}
                          </span>
                          <span className="text-[10px] text-teal-600 font-mono">Proposed Artifact</span>
                        </div>
                        <p className="text-[11px] text-neutral-500 dark:text-neutral-400">
                          {msg.generatedArtifactPreview.summary}
                        </p>
                        <button
                          onClick={() => {
                            if (msg.generatedArtifactPreview) {
                              saveArtifact({
                                questId: 'quest-ci-triage',
                                shipId: 'ship-dev',
                                producerCrewId: 'crew-qa-reviewer',
                                title: msg.generatedArtifactPreview.title || 'CI Remediation Brief',
                                type: 'ci-triage',
                                summary: msg.generatedArtifactPreview.summary || '',
                                content: `# ${msg.generatedArtifactPreview.title}\n\n${msg.generatedArtifactPreview.summary}`,
                                discoveries: [
                                  { id: 'd-new', type: 'risk', title: 'Teardown Timeout', detail: 'Remediated via explicit socket deadline.', evidenceSource: 'pkg/transport/' }
                                ],
                                evidenceCount: 6,
                                voyageCostUSD: 0.15,
                                status: 'needs_review'
                              });
                              setActiveTab('artifacts');
                            }
                          }}
                          className="w-full mt-1 py-1 px-2 rounded bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-semibold text-[11px] hover:opacity-90 flex items-center justify-center gap-1"
                        >
                          <BookmarkPlus className="w-3.5 h-3.5" />
                          Save to Artifact Gallery
                        </button>
                      </div>
                    )}
                  </div>

                  {/* Suggested actions list */}
                  {msg.suggestedActions && msg.suggestedActions.length > 0 && (
                    <div className="flex flex-wrap gap-1.5 pt-0.5">
                      {msg.suggestedActions.map((action, idx) => (
                        <button
                          key={idx}
                          onClick={() => {
                            if (action.actionType === 'open_tab' && action.payload) {
                              setActiveTab(action.payload);
                            } else if (action.actionType === 'create_quest') {
                              createQuest({ title: action.payload?.title || 'Release v1.4 Verification' });
                              setActiveTab('mission-board');
                            } else if (action.actionType === 'save_artifact' && action.payload) {
                              saveArtifact({
                                questId: 'quest-ci-triage',
                                shipId: 'ship-dev',
                                producerCrewId: 'crew-qa-reviewer',
                                title: action.payload.title,
                                type: 'ci-triage',
                                summary: action.payload.summary,
                                content: `# ${action.payload.title}\n\n${action.payload.summary}`,
                                discoveries: [],
                                evidenceCount: 4,
                                voyageCostUSD: 0.20,
                                status: 'needs_review'
                              });
                              setActiveTab('artifacts');
                            }
                          }}
                          className="px-2.5 py-1 rounded-md text-[11px] font-medium border border-neutral-200 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-800/80 text-neutral-700 dark:text-neutral-300 hover:border-teal-500 hover:text-teal-600 dark:hover:text-teal-300 transition-colors flex items-center gap-1.5"
                        >
                          <Sparkles className="w-3 h-3 text-amber-500" />
                          <span>{action.label}</span>
                        </button>
                      ))}
                    </div>
                  )}

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
        </div>

        {/* Input composer */}
        <form
          onSubmit={handleSend}
          className="p-3 border-t border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-900/30 flex items-center gap-2"
        >
          <input
            type="text"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            placeholder="Ask Quartermaster, propose a goal, or type /quest..."
            className="flex-1 bg-white dark:bg-neutral-950 border border-neutral-200 dark:border-neutral-800 rounded-lg px-3 py-2 text-xs text-neutral-900 dark:text-neutral-100 placeholder:text-neutral-400 focus:outline-none focus:ring-1 focus:ring-teal-500"
          />
          <button
            type="submit"
            disabled={!input.trim()}
            className="px-3.5 py-2 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 text-xs font-semibold hover:opacity-90 disabled:opacity-40 transition-opacity flex items-center gap-1.5"
          >
            <Send className="w-3.5 h-3.5" />
            <span className="hidden sm:inline">Command</span>
          </button>
        </form>
      </div>

      {/* Recent Artifacts Shelf */}
      <div className="space-y-3 pt-2">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
              Recent Durable Artifacts
            </span>
            <span className="text-[11px] text-neutral-400">
              · Evidence-backed deliverables
            </span>
          </div>
          <button
            onClick={() => setActiveTab('artifacts')}
            className="text-xs text-teal-600 dark:text-teal-400 hover:underline flex items-center gap-1 font-medium"
          >
            View all ({artifacts.length}) →
          </button>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
          {recentArtifacts.map((art) => (
            <div
              key={art.id}
              onClick={() => {
                setSelectedArtifactId(art.id);
                setActiveTab('artifacts');
              }}
              className="p-3.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] hover:border-teal-500/50 cursor-pointer transition-all space-y-2 group"
            >
              <div className="flex items-start justify-between gap-2">
                <span className="text-xs font-semibold text-neutral-900 dark:text-neutral-100 group-hover:text-teal-600 dark:group-hover:text-teal-400 line-clamp-1">
                  {art.title}
                </span>
                {art.status === 'treasure' ? (
                  <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-amber-500/20 text-amber-500 shrink-0 font-semibold">
                    Treasure
                  </span>
                ) : (
                  <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-neutral-200 dark:bg-neutral-800 text-neutral-500 shrink-0">
                    Review
                  </span>
                )}
              </div>

              <p className="text-xs text-neutral-500 dark:text-neutral-400 line-clamp-2 leading-relaxed">
                {art.summary}
              </p>

              <div className="flex items-center justify-between text-[11px] text-neutral-400 font-mono pt-1 border-t border-neutral-100 dark:border-neutral-800/80">
                <span>{art.evidenceCount} sources</span>
                <span>${art.voyageCostUSD.toFixed(2)} cost</span>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
};
