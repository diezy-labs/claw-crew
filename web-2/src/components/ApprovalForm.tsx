import React, { useState } from 'react';
import { ChevronLeft } from 'lucide-react';
import { Button } from './common/Button';
import { useFleetStore } from '../store/fleetStore';

interface ApprovalFormProps {
  approvalId: string;
  onBack?: () => void;
  onSubmit?: (data: { decision: 'approve' | 'reject' | 'request_changes'; comments?: string }) => void;
}

export const ApprovalForm: React.FC<ApprovalFormProps> = ({
  approvalId,
  onBack,
  onSubmit
}) => {
  const { approvals, ships, crew } = useFleetStore();
  const approval = approvals.find((a) => a.id === approvalId);

  const [decision, setDecision] = useState<'approve' | 'reject' | 'request_changes'>('approve');
  const [comments, setComments] = useState('');

  if (!approval) {
    return (
      <div className="flex flex-col items-center justify-center h-full text-center space-y-4 p-8">
        <div className="p-4 rounded-full bg-neutral-100 dark:bg-neutral-800">
          <ChevronLeft className="w-8 h-8 text-neutral-500" />
        </div>
        <div>
          <h3 className="text-lg font-semibold text-neutral-900 dark:text-neutral-100">
            Approval Not Found
          </h3>
          <p className="text-sm text-neutral-500 mt-2">
            The requested approval could not be located.
          </p>
        </div>
        {onBack && (
          <Button variant="outline" onClick={onBack}>
            Back to Approvals
          </Button>
        )}
      </div>
    );
  }

  const ship = ships.find((s) => s.id === approval.shipId);
  const crewMember = crew.find((c) => c.id === approval.crewId);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    onSubmit?.({ decision, comments: comments.trim() || undefined });
  };

  return (
    <form onSubmit={handleSubmit} className="flex flex-col h-full animate-fade-in">
      {/* Header */}
      <div className="flex items-center gap-3 pb-4 border-b border-neutral-200 dark:border-neutral-800">
        <button
          type="button"
          onClick={onBack}
          className="p-2 rounded-lg hover:bg-neutral-100 dark:hover:bg-neutral-800 text-neutral-600 dark:text-neutral-400 transition-colors"
          aria-label="Back to approvals"
        >
          <ChevronLeft className="w-5 h-5" />
        </button>
        <div>
          <h2 className="text-lg font-bold text-neutral-900 dark:text-neutral-100">
            Approval Form
          </h2>
          <p className="text-xs text-neutral-500">
            Requested by {crewMember?.name || 'Specialist'} · {ship?.name || 'Vessel'}
          </p>
        </div>
      </div>

      {/* Scrollable Content */}
      <div className="flex-1 overflow-y-auto space-y-6 py-4">
        {/* Task ID & Description */}
        <div className="space-y-2">
          <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 uppercase">
            Task ID
          </label>
          <div className="p-3 rounded-lg bg-neutral-50 dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 font-mono text-sm text-neutral-900 dark:text-neutral-100">
            {approval.id}
          </div>
        </div>

        <div className="space-y-2">
          <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 uppercase">
            Task Description
          </label>
          <div className="p-3 rounded-lg bg-neutral-50 dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 text-sm text-neutral-800 dark:text-neutral-200 leading-relaxed">
            {approval.draftSummary}
          </div>
        </div>

        {/* Decision */}
        <div className="space-y-3">
          <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 uppercase">
            Approver Decision
          </label>
          <div className="grid grid-cols-3 gap-2">
            <button
              type="button"
              onClick={() => setDecision('approve')}
              className={`flex flex-col items-center justify-center p-3 rounded-lg border transition-all ${
                decision === 'approve'
                  ? 'bg-emerald-50 dark:bg-emerald-950/30 border-emerald-500 text-emerald-700 dark:text-emerald-400'
                  : 'bg-white dark:bg-neutral-950 border-neutral-200 dark:border-neutral-800 text-neutral-700 dark:text-neutral-300 hover:border-neutral-300 dark:hover:border-neutral-700'
              }`}
            >
              <span className="text-sm font-medium mb-1">Approve</span>
              <span className="text-[10px] opacity-75">Accept request</span>
            </button>

            <button
              type="button"
              onClick={() => setDecision('reject')}
              className={`flex flex-col items-center justify-center p-3 rounded-lg border transition-all ${
                decision === 'reject'
                  ? 'bg-rose-50 dark:bg-rose-950/30 border-rose-500 text-rose-700 dark:text-rose-400'
                  : 'bg-white dark:bg-neutral-950 border-neutral-200 dark:border-neutral-800 text-neutral-700 dark:text-neutral-300 hover:border-neutral-300 dark:hover:border-neutral-700'
              }`}
            >
              <span className="text-sm font-medium mb-1">Reject</span>
              <span className="text-[10px] opacity-75">Deny request</span>
            </button>

            <button
              type="button"
              onClick={() => setDecision('request_changes')}
              className={`flex flex-col items-center justify-center p-3 rounded-lg border transition-all ${
                decision === 'request_changes'
                  ? 'bg-amber-50 dark:bg-amber-950/30 border-amber-500 text-amber-700 dark:text-amber-400'
                  : 'bg-white dark:bg-neutral-950 border-neutral-200 dark:border-neutral-800 text-neutral-700 dark:text-neutral-300 hover:border-neutral-300 dark:hover:border-neutral-700'
              }`}
            >
              <span className="text-sm font-medium mb-1">Request Changes</span>
              <span className="text-[10px] opacity-75">Ask for edits</span>
            </button>
          </div>
        </div>

        {/* Comments */}
        <div className="space-y-2">
          <label htmlFor="comments" className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 uppercase">
            Comments (Optional)
          </label>
          <textarea
            id="comments"
            value={comments}
            onChange={(e) => setComments(e.target.value)}
            placeholder="Add your comments or reasoning here..."
            className="w-full min-h-[120px] p-3 rounded-lg bg-white dark:bg-neutral-950 border border-neutral-200 dark:border-neutral-800 text-sm text-neutral-900 dark:text-neutral-100 placeholder-neutral-400 focus:outline-none focus:border-teal-500 dark:focus:border-teal-500 focus:ring-1 focus:ring-teal-500/20 transition-all resize-none"
          />
        </div>
      </div>

      {/* Footer */}
      <div className="pt-4 border-t border-neutral-200 dark:border-neutral-800 flex items-center justify-end gap-3">
        {onBack && (
          <Button variant="outline" onClick={onBack} type="button">
            Cancel
          </Button>
        )}
        <Button type="submit" variant="primary" size="sm">
          Submit Decision
        </Button>
      </div>
    </form>
  );
};
