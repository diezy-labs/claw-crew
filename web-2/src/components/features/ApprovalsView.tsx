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
import { PageHeader } from '../common/PageHeader';
import { Button } from '../common/Button';
import { Modal } from '../common/Modal';
import { ItemCard } from '../common/ItemCard';
import { CardPopover } from '../common/CardPopover';

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
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto px-4 sm:px-6 pt-0 pb-6 space-y-4 max-w-5xl mx-auto w-full animate-view-fade-in">
      {/* Standard Reusable PageHeader */}
      <PageHeader
        icon={<ShieldAlert className="w-4 h-4 text-amber-500 shrink-0" />}
        title="Captain’s Approval"
        badge={
          <span className="hidden sm:inline-flex text-xs font-mono text-amber-500 px-2 py-0.5 rounded bg-amber-500/10">
            {pendingList.length} Action{pendingList.length === 1 ? '' : 's'} Pending
          </span>
        }
        description="Review and authorize external actions before autonomous agents affect external systems or codebases."
      />

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
              <ItemCard
                key={appr.id}
                selected={selectedApproval?.id === appr.id}
                onClick={() => setSelectedApproval(appr)}
                accentColor="amber"
                icon={<ShieldAlert className="w-4 h-4 text-amber-500" />}
                title={appr.title}
                subtitle={`Requested by ${crewMember?.name || 'Specialist'} · ${ship?.name || 'Vessel'}`}
                badge={
                  <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-amber-500/20 text-amber-500 font-semibold self-start sm:self-auto">
                    ACTION DIGEST VERIFIED
                  </span>
                }
                children={
                  <div className="space-y-3 pt-1">
                    <div className="grid grid-cols-1 md:grid-cols-2 gap-3 p-3 rounded-lg bg-neutral-50 dark:bg-neutral-900/60 border border-neutral-200 dark:border-neutral-800 font-mono text-[11px]">
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
                }
                footer={
                  <div className="flex items-center justify-end gap-2.5">
                    <Button
                      variant="outline"
                      size="sm"
                      icon={<XCircle className="w-3.5 h-3.5 text-rose-500" />}
                      shortLabel="Reject"
                      onClick={(e) => {
                        e.stopPropagation();
                        handleApproval(appr.id, 'rejected');
                      }}
                      className="border-rose-300 dark:border-rose-900/60 text-rose-700 dark:text-rose-400 hover:bg-rose-50 dark:hover:bg-rose-950/30"
                    >
                      Reject
                    </Button>

                    <Button
                      variant="primary"
                      size="sm"
                      icon={<CheckCircle2 className="w-3.5 h-3.5" />}
                      shortLabel="Approve"
                      onClick={(e) => {
                        e.stopPropagation();
                        handleApproval(appr.id, 'approved');
                      }}
                    >
                      Approve Action
                    </Button>
                  </div>
                }
              />
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

      {/* Detail Approval Popover / Drawer */}
      <CardPopover
        isOpen={Boolean(selectedApproval)}
        onClose={() => setSelectedApproval(null)}
        variant="sheet-right"
        drawerWidth="sm:w-[580px]"
        icon={<ShieldAlert className="w-4 h-4 text-amber-500" />}
        title={
          selectedApproval && (
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
              <span className="text-base sm:text-lg font-bold text-neutral-900 dark:text-neutral-100">
                {selectedApproval.title}
              </span>
            </div>
          )
        }
        subtitle={
          selectedApproval && (
            <span>
              Requested by {crew.find((c) => c.id === selectedApproval.crewId)?.name || 'Specialist'} ·{' '}
              {ships.find((s) => s.id === selectedApproval.shipId)?.name || 'Vessel'}
            </span>
          )
        }
        badge={
          selectedApproval && (
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
          )
        }
        footer={
          selectedApproval && (
            <div className="flex items-center justify-between w-full">
              <button
                type="button"
                onClick={() => setSelectedApproval(null)}
                className="px-3.5 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-700 text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800 text-xs font-medium cursor-pointer"
              >
                Close
              </button>

              {selectedApproval.status === 'pending' && (
                <div className="flex items-center gap-2">
                  <button
                    type="button"
                    onClick={() => {
                      handleApproval(selectedApproval.id, 'rejected');
                      setSelectedApproval(null);
                    }}
                    className="px-4 py-2 rounded-lg border border-rose-300 dark:border-rose-900/60 bg-rose-50 dark:bg-rose-950/20 text-rose-700 dark:text-rose-400 font-semibold hover:bg-rose-100 dark:hover:bg-rose-900/40 text-xs transition-colors flex items-center gap-1.5 cursor-pointer"
                  >
                    <XCircle className="w-4 h-4" />
                    <span className="hidden sm:inline">Reject Action</span>
                    <span className="sm:hidden">Reject</span>
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
                    <span className="hidden sm:inline">Approve &amp; Sign Action</span>
                    <span className="sm:hidden">Approve</span>
                  </button>
                </div>
              )}
            </div>
          )
        }
      >
        {selectedApproval && (
          <div className="space-y-4">
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
          </div>
        )}
      </CardPopover>
    </div>
  );
};
