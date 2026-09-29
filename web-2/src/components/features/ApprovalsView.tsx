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

export const ApprovalsView: React.FC = () => {
  const { approvals, handleApproval, ships, crew } = useFleetStore();

  const pendingList = approvals.filter((a) => a.status === 'pending');
  const historyList = approvals.filter((a) => a.status !== 'pending');

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto p-4 sm:p-6 space-y-6 max-w-5xl mx-auto w-full animate-view-fade-in">
      {/* Header */}
      <div>
        <div className="flex items-center gap-2">
          <h1 className="text-xl sm:text-2xl font-bold tracking-tight text-neutral-900 dark:text-neutral-100">
            Captain’s Approval
          </h1>
          <span className="text-xs font-mono text-amber-500 px-2 py-0.5 rounded bg-amber-500/10">
            {pendingList.length} Action{pendingList.length === 1 ? '' : 's'} Pending
          </span>
        </div>
        <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
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
                className="p-5 rounded-xl border border-amber-500/40 bg-white dark:bg-[#191b1f] shadow-sm space-y-4 text-xs"
              >
                {/* Header item */}
                <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 border-b border-neutral-100 dark:border-neutral-800 pb-3">
                  <div>
                    <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100">
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
                    onClick={() => handleApproval(appr.id, 'rejected')}
                    className="px-4 py-2 rounded-lg border border-rose-300 dark:border-rose-900/60 bg-rose-50 dark:bg-rose-950/20 text-rose-700 dark:text-rose-400 font-semibold hover:bg-rose-100 dark:hover:bg-rose-900/40 transition-colors flex items-center gap-1.5"
                  >
                    <XCircle className="w-4 h-4" />
                    <span>Reject Action</span>
                  </button>

                  <button
                    onClick={() => handleApproval(appr.id, 'approved')}
                    className="px-5 py-2 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-bold hover:opacity-90 transition-opacity flex items-center gap-1.5"
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
                className="p-3 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] flex items-center justify-between gap-3 text-xs"
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
    </div>
  );
};
