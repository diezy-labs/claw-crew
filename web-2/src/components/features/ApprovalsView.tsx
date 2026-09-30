import React, { useState } from 'react';
import {
  ShieldAlert,
  CheckCircle2,
  XCircle,
  AlertTriangle,
  ExternalLink,
  ShieldCheck,
  Ship,
  Clock,
  Coins
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';
import { CaptainApproval } from '../../types';

export const ApprovalsView: React.FC = () => {
  const { approvals, handleApproval, ships, crew } = useFleetStore();

  const [selectedApproval, setSelectedApproval] = useState<CaptainApproval | null>(null);

  // Close modal on Escape key press
  React.useEffect(() => {
    if (!selectedApproval) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setSelectedApproval(null);
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [selectedApproval]);

  const pendingList = approvals.filter((a) => a.status === 'pending');
  const historyList = approvals.filter((a) => a.status !== 'pending');

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto p-4 sm:p-6 space-y-6 max-w-5xl mx-auto w-full animate-view-fade-in">
      {/* Header - Compact on mobile with icon and title only */}
      <div className="border-b border-neutral-200 dark:border-neutral-800 pb-4">
        <div className="flex items-center gap-2.5">
          <div className="w-8 h-8 rounded-lg bg-amber-500/10 text-amber-500 flex items-center justify-center shrink-0 ring-1 ring-amber-500/20">
            <ShieldAlert className="w-4 h-4 text-amber-500 shrink-0" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h1 className="text-lg sm:text-2xl font-bold tracking-tight text-neutral-900 dark:text-neutral-100 leading-tight">
                Captain’s Approval
              </h1>
              <span className="hidden sm:inline-flex text-xs font-mono text-amber-500 px-2 py-0.5 rounded bg-amber-500/10">
                {pendingList.length} Action{pendingList.length === 1 ? '' : 's'} Pending
              </span>
            </div>
          </div>
        </div>
        <p className="hidden sm:block text-xs text-neutral-500 dark:text-neutral-400 mt-1 pl-10.5">
          Review and authorize external actions before autonomous agents affect external systems or codebases.
        </p>
      </div>

      {/* Pending Approvals Queue */}
      <div className="space-y-4">
        <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
          Pending Verification ({pendingList.length})
        </span>

        {pendingList.length === 0 ? (
          <div className="p-8 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] text-center space-y-2">
            <ShieldCheck className="w-8 h-8 text-emerald-500 mx-auto" />
            <h3 className="text-sm font-semibold text-neutral-800 dark:text-neutral-200">
              No Pending Actions
            </h3>
            <p className="text-xs text-neutral-400 max-w-sm mx-auto">
              All agent side-effects have been reviewed. Specialist ships will request verification whenever write-level tools are invoked.
            </p>
          </div>
        ) : (
          pendingList.map((appr) => {
            const ship = ships.find((s) => s.id === appr.shipId);
            const crewMember = crew.find((c) => c.id === appr.crewId);

            return (
              <div
                key={appr.id}
                onClick={() => setSelectedApproval(appr)}
                className="p-5 rounded-xl border border-amber-500/40 bg-white dark:bg-[#191b1f] shadow-sm space-y-4 text-xs cursor-pointer hover:border-amber-500/80 hover:shadow-md transition-all group"
                title="Click to inspect complete verification details"
              >
                {/* Header item */}
                <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 border-b border-neutral-100 dark:border-neutral-800 pb-3">
                  <div>
                    <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100 group-hover:text-amber-600 dark:group-hover:text-amber-400 transition-colors">
                      {appr.title}
                    </h3>
                    <div className="text-[11px] text-neutral-500 font-mono mt-0.5">
                      Requested by {crewMember?.name || 'Specialist'} · {ship?.name}
                    </div>
                  </div>
                  <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-amber-500/20 text-amber-500 font-semibold self-start sm:self-auto">
                    ACTION DIGEST VERIFIED
                  </span>
                </div>

                {/* Grid details */}
                <div className="grid grid-cols-1 md:grid-cols-2 gap-3 p-3.5 rounded-lg bg-neutral-50 dark:bg-neutral-900/60 border border-neutral-200 dark:border-neutral-800 font-mono text-[11px]">
                  <div>
                    <span className="text-[10px] text-neutral-400 uppercase block">Target Resource</span>
                    <span className="text-neutral-900 dark:text-neutral-100 font-semibold break-all">
                      {appr.targetResource}
                    </span>
                  </div>
                  <div>
                    <span className="text-[10px] text-neutral-400 uppercase block">Action Type</span>
                    <span className="text-teal-600 dark:text-teal-400 font-semibold uppercase">
                      {appr.actionType.replace(/_/g, ' ')}
                    </span>
                  </div>
                </div>

                {/* Justification & Effect */}
                <div className="space-y-3">
                  <div>
                    <span className="font-semibold text-neutral-800 dark:text-neutral-200 block mb-0.5">
                      Why Now?
                    </span>
                    <p className="text-neutral-600 dark:text-neutral-400 leading-relaxed">
                      {appr.justification}
                    </p>
                  </div>

                  <div>
                    <span className="font-semibold text-neutral-800 dark:text-neutral-200 block mb-0.5">
                      Exact Side Effect &amp; Scope Boundary
                    </span>
                    <p className="text-neutral-600 dark:text-neutral-400 leading-relaxed bg-neutral-50 dark:bg-neutral-900/40 p-2.5 rounded border border-neutral-200 dark:border-neutral-800">
                      {appr.effect}
                    </p>
                  </div>

                  <div>
                    <span className="font-semibold text-neutral-800 dark:text-neutral-200 block mb-0.5">
                      Draft Body Preview
                    </span>
                    <pre className="p-3 rounded-lg bg-neutral-100 dark:bg-neutral-950 border border-neutral-200 dark:border-neutral-800 text-[11px] font-mono text-neutral-800 dark:text-neutral-200 overflow-x-auto whitespace-pre-wrap">
                      {appr.draftSummary}
                    </pre>
                  </div>
                </div>

                {/* Action Buttons */}
                <div className="pt-3 border-t border-neutral-100 dark:border-neutral-800 flex items-center justify-end gap-2.5">
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      handleApproval(appr.id, 'rejected');
                    }}
                    className="px-4 py-2 rounded-lg border border-rose-300 dark:border-rose-900/60 bg-rose-50 dark:bg-rose-950/20 text-rose-700 dark:text-rose-400 font-semibold hover:bg-rose-100 dark:hover:bg-rose-900/40 transition-colors flex items-center gap-1.5 cursor-pointer"
                  >
                    <XCircle className="w-4 h-4" />
                    <span>Reject Action</span>
                  </button>

                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      handleApproval(appr.id, 'approved');
                    }}
                    className="px-5 py-2 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-bold hover:opacity-90 transition-opacity flex items-center gap-1.5 cursor-pointer shadow-xs"
                  >
                    <CheckCircle2 className="w-4 h-4" />
                    <span>Approve &amp; Sign Action</span>
                  </button>
                </div>
              </div>
            );
          })
        )}
      </div>

      {/* Audit History of Decisions */}
      {historyList.length > 0 && (
        <div className="space-y-3 pt-4">
          <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
            Resolved Decisions ({historyList.length})
          </span>

          <div className="space-y-2">
            {historyList.map((appr) => (
              <div
                key={appr.id}
                onClick={() => setSelectedApproval(appr)}
                className="p-3 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] flex items-center justify-between gap-3 text-xs cursor-pointer hover:border-neutral-300 dark:hover:border-neutral-700 hover:shadow-xs transition-all"
                title="Click to view resolution record"
              >
                <div>
                  <div className="font-semibold text-neutral-900 dark:text-neutral-100">
                    {appr.title}
                  </div>
                  <div className="text-[11px] text-neutral-400 font-mono">
                    {appr.targetResource}
                  </div>
                </div>
                <span
                  className={`text-[10px] font-mono px-2 py-0.5 rounded uppercase font-semibold ${
                    appr.status === 'approved'
                      ? 'bg-emerald-500/20 text-emerald-500'
                      : 'bg-rose-500/20 text-rose-500'
                  }`}
                >
                  {appr.status}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Detail Approval Modal / Popup (Click outside to close, NO close X button) */}
      {selectedApproval && (
        <div
          onClick={() => setSelectedApproval(null)}
          className="fixed inset-0 z-50 bg-black/65 backdrop-blur-xs flex items-center justify-center p-3 sm:p-4 animate-in fade-in duration-150"
        >
          <div
            onClick={(e) => e.stopPropagation()}
            className="w-full max-w-2xl rounded-2xl border border-amber-500/50 bg-white dark:bg-[#181a1e] p-5 sm:p-7 shadow-2xl space-y-5 max-h-[90vh] overflow-y-auto"
          >
            {/* Header info */}
            <div className="flex flex-col sm:flex-row sm:items-start justify-between gap-2 border-b border-neutral-100 dark:border-neutral-800 pb-4">
              <div>
                <div className="flex items-center gap-2">
                  <span
                    className={`w-2.5 h-2.5 rounded-full ${
                      selectedApproval.status === 'pending'
                        ? 'bg-amber-500 animate-pulse'
                        : selectedApproval.status === 'approved'
                        ? 'bg-emerald-500'
                        : 'bg-rose-500'
                    }`}
                  />
                  <span className="text-[11px] font-mono text-neutral-400 uppercase tracking-wider">
                    Captain Risk Verification Gate
                  </span>
                </div>
                <h2 className="text-base sm:text-lg font-bold text-neutral-900 dark:text-neutral-100 mt-1">
                  {selectedApproval.title}
                </h2>
                <div className="text-xs text-neutral-500 font-mono mt-1">
                  Requested by{' '}
                  {crew.find((c) => c.id === selectedApproval.crewId)?.name || 'Specialist'} ·{' '}
                  {ships.find((s) => s.id === selectedApproval.shipId)?.name || 'Vessel'}
                </div>
              </div>
              <div className="flex items-center gap-2 self-start">
                <span
                  className={`text-[10px] font-mono px-2.5 py-1 rounded uppercase font-bold ${
                    selectedApproval.status === 'pending'
                      ? 'bg-amber-500/20 text-amber-500 border border-amber-500/30'
                      : selectedApproval.status === 'approved'
                      ? 'bg-emerald-500/20 text-emerald-500 border border-emerald-500/30'
                      : 'bg-rose-500/20 text-rose-500 border border-rose-500/30'
                  }`}
                >
                  {selectedApproval.status === 'pending'
                    ? 'ACTION DIGEST VERIFIED'
                    : selectedApproval.status.toUpperCase()}
                </span>
                <span className="text-[10px] font-mono text-neutral-400 bg-neutral-100 dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 px-2 py-0.5 rounded select-none">
                  Click outside to close
                </span>
              </div>
            </div>

            {/* Target and Action metadata */}
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 p-3.5 rounded-xl bg-neutral-50 dark:bg-neutral-900/60 border border-neutral-200 dark:border-neutral-800 font-mono text-xs">
              <div>
                <span className="text-[10px] text-neutral-400 uppercase block">Target Resource</span>
                <span className="text-neutral-900 dark:text-neutral-100 font-semibold break-all">
                  {selectedApproval.targetResource}
                </span>
              </div>
              <div>
                <span className="text-[10px] text-neutral-400 uppercase block">Action Type</span>
                <span className="text-teal-600 dark:text-teal-400 font-semibold uppercase">
                  {selectedApproval.actionType.replace(/_/g, ' ')}
                </span>
              </div>
            </div>

            {/* Justification & Effect */}
            <div className="space-y-3.5 text-xs">
              <div>
                <span className="font-bold text-neutral-800 dark:text-neutral-200 block mb-1">
                  Why Now? (Strategic Motivation)
                </span>
                <p className="text-neutral-700 dark:text-neutral-300 leading-relaxed bg-neutral-50/70 dark:bg-neutral-900/30 p-3 rounded-lg border border-neutral-200 dark:border-neutral-800">
                  {selectedApproval.justification}
                </p>
              </div>

              <div>
                <span className="font-bold text-neutral-800 dark:text-neutral-200 block mb-1">
                  Exact Side Effect &amp; Scope Boundary
                </span>
                <p className="text-neutral-700 dark:text-neutral-300 leading-relaxed bg-neutral-50/70 dark:bg-neutral-900/30 p-3 rounded-lg border border-neutral-200 dark:border-neutral-800">
                  {selectedApproval.effect}
                </p>
              </div>

              <div>
                <span className="font-bold text-neutral-800 dark:text-neutral-200 block mb-1">
                  Complete Draft Body Preview
                </span>
                <pre className="p-3.5 rounded-xl bg-neutral-100 dark:bg-neutral-950 border border-neutral-200 dark:border-neutral-800 text-xs font-mono text-neutral-800 dark:text-neutral-200 overflow-x-auto whitespace-pre-wrap leading-relaxed max-h-60">
                  {selectedApproval.draftSummary}
                </pre>
              </div>
            </div>

            {/* Action Buttons inside modal */}
            <div className="pt-4 border-t border-neutral-100 dark:border-neutral-800 flex items-center justify-between gap-3">
              <span className="text-[11px] font-mono text-neutral-400 select-none">
                Tap or click outside to dismiss &bull; Esc
              </span>

              {selectedApproval.status === 'pending' && (
                <>
                  <button
                    type="button"
                    onClick={() => {
                      handleApproval(selectedApproval.id, 'rejected');
                      setSelectedApproval(null);
                    }}
                    className="px-4 py-2 rounded-lg border border-rose-300 dark:border-rose-900/60 bg-rose-50 dark:bg-rose-950/20 text-rose-700 dark:text-rose-400 font-semibold hover:bg-rose-100 dark:hover:bg-rose-900/40 text-xs transition-colors flex items-center gap-1.5 cursor-pointer"
                  >
                    <XCircle className="w-4 h-4" />
                    <span>Reject Action</span>
                  </button>

                  <button
                    type="button"
                    onClick={() => {
                      handleApproval(selectedApproval.id, 'approved');
                      setSelectedApproval(null);
                    }}
                    className="px-5 py-2 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-bold text-xs hover:opacity-90 transition-opacity flex items-center gap-1.5 cursor-pointer shadow-xs"
                  >
                    <CheckCircle2 className="w-4 h-4" />
                    <span>Approve &amp; Sign Action</span>
                  </button>
                </>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
