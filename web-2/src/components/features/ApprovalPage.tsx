import React, { useState } from 'react';
import { useFleetStore } from '../../store/fleetStore';
import { ApprovalForm } from '../ApprovalForm';
import { PageHeader } from '../common/PageHeader';

export const ApprovalPage: React.FC = () => {
  const { approvals } = useFleetStore();
  const [selectedApprovalId, setSelectedApprovalId] = useState<string | null>(null);

  // Auto-select first pending approval on mount if none selected
  React.useEffect(() => {
    if (!selectedApprovalId) {
      const pending = approvals.find((a) => a.status === 'pending');
      if (pending) {
        setSelectedApprovalId(pending.id);
      }
    }
  }, [approvals, selectedApprovalId]);

  const pendingList = approvals.filter((a) => a.status === 'pending');
  const historyList = approvals.filter((a) => a.status !== 'pending');

  const handleSubmit = (data: { decision: 'approve' | 'reject' | 'request_changes'; comments?: string }) => {
    // For E3 integration, dispatch to store
    // In production, this would call a proper submit method on the store
    const approval = approvals.find((a) => a.id === selectedApprovalId);
    if (!approval) return;

    console.log('[ApprovalPage] Submitting decision:', {
      id: approval.id,
      title: approval.title,
      decision: data.decision,
      comments: data.comments
    });

    // Reset form after submission
    setSelectedApprovalId(null);

    // Show next pending or history
    const nextPending = pendingList.find((a) => a.id !== approval.id);
    if (nextPending) {
      setSelectedApprovalId(nextPending.id);
    }
  };

  const selectedApproval = approvals.find((a) => a.id === selectedApprovalId);

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-hidden animate-view-fade-in">
      {/* Page Header */}
      <div className="px-6 pt-4 pb-2 border-b border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f]">
        <PageHeader
          icon={<svg className="w-5 h-5 text-amber-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>}
          title="Captain's Approval"
          badge={
            <span className="text-xs font-mono text-amber-500 px-2 py-0.5 rounded bg-amber-500/10">
              {pendingList.length} Action{pendingList.length === 1 ? '' : 's'} Pending
            </span>
          }
          description="Review and authorize external actions before autonomous agents affect external systems."
        />
      </div>

      {/* Content Area */}
      <div className="flex-1 flex overflow-hidden bg-neutral-50 dark:bg-[#0a0b0d]">
        {/* Sidebar - Approval Queue */}
        <div className="w-96 max-w-full flex-shrink-0 border-r border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] flex flex-col">
          <div className="p-4 border-b border-neutral-200 dark:border-neutral-800">
            <h3 className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400 mb-3">
              Pending Queue ({pendingList.length})
            </h3>
            {pendingList.length === 0 ? (
              <div className="p-4 rounded-lg bg-neutral-100 dark:bg-neutral-800 text-center">
                <p className="text-sm text-neutral-500">No pending approvals</p>
              </div>
            ) : (
              <div className="space-y-2">
                {pendingList.map((appr) => (
                  <button
                    key={appr.id}
                    onClick={() => setSelectedApprovalId(appr.id)}
                    className={`w-full text-left px-3 py-2.5 rounded-lg border text-sm transition-all ${
                      selectedApprovalId === appr.id
                        ? 'bg-teal-50 dark:bg-teal-950/20 border-teal-500/50 text-teal-900 dark:text-teal-100'
                        : 'bg-white dark:bg-[#191b1f] border-neutral-200 dark:border-neutral-800 hover:border-neutral-300 dark:hover:border-neutral-700 text-neutral-700 dark:text-neutral-300'
                    }`}
                  >
                    <div className="font-medium truncate mb-0.5">{appr.title}</div>
                    <div className="text-xs text-neutral-500 truncate">
                      {appr.targetResource}
                    </div>
                  </button>
                ))}
              </div>
            )}
          </div>

          <div className="p-4 border-b border-neutral-200 dark:border-neutral-800 flex-1 overflow-y-auto">
            <h3 className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400 mb-3">
              History ({historyList.length})
            </h3>
            {historyList.length === 0 ? (
              <div className="p-4 rounded-lg bg-neutral-100 dark:bg-neutral-800 text-center">
                <p className="text-sm text-neutral-500">No decisions yet</p>
              </div>
            ) : (
              <div className="space-y-2">
                {historyList.map((appr) => (
                  <button
                    key={appr.id}
                    onClick={() => setSelectedApprovalId(appr.id)}
                    className={`w-full text-left px-3 py-2.5 rounded-lg border text-sm transition-all ${
                      selectedApprovalId === appr.id
                        ? 'bg-neutral-100 dark:bg-neutral-800 border-neutral-300 dark:border-neutral-700'
                        : 'bg-white dark:bg-[#191b1f] border-neutral-200 dark:border-neutral-800 hover:bg-neutral-50 dark:hover:bg-neutral-800/50'
                    }`}
                  >
                    <div className="flex items-center justify-between mb-1">
                      <span className="font-medium truncate flex-1 mr-2">{appr.title}</span>
                      <span
                        className={`text-[10px] px-1.5 py-0.5 rounded uppercase font-semibold ${
                          appr.status === 'approved'
                            ? 'bg-emerald-500/20 text-emerald-600 dark:text-emerald-400'
                            : 'bg-rose-500/20 text-rose-600 dark:text-rose-400'
                        }`}
                      >
                        {appr.status}
                      </span>
                    </div>
                    <div className="text-xs text-neutral-500 truncate">{appr.targetResource}</div>
                  </button>
                ))}
              </div>
            )}
          </div>
        </div>

        {/* Main Form Area */}
        <div className="flex-1 bg-neutral-50 dark:bg-[#0a0b0d] overflow-y-auto">
          <div className="max-w-2xl mx-auto p-6">
            {selectedApproval ? (
              <ApprovalForm
                approvalId={selectedApproval.id}
                onSubmit={handleSubmit}
              />
            ) : (
              <div className="flex flex-col items-center justify-center h-[50vh] text-center space-y-4">
                <div className="w-20 h-20 rounded-full bg-neutral-200 dark:bg-neutral-800 flex items-center justify-center">
                  <svg className="w-10 h-10 text-neutral-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
                  </svg>
                </div>
                <div>
                  <h3 className="text-lg font-semibold text-neutral-900 dark:text-neutral-100">
                    Select an Approval to Review
                  </h3>
                  <p className="text-sm text-neutral-500 mt-2">
                    Choose an item from the queue to view details and make a decision.
                  </p>
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
};
