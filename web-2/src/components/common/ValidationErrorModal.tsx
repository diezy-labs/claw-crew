import React, { useState } from 'react';
import { XCircle, AlertCircle, ChevronDown, ChevronRight, Copy, Check } from 'lucide-react';
import { Modal } from './Modal';
import { Button } from './Button';

export interface ValidationErrorItem {
  field?: string;
  message: string;
  suggestion?: string;
}

export interface ValidationErrorModalProps {
  isOpen: boolean;
  onClose: () => void;
  title?: string;
  subtitle?: string;
  errors: ValidationErrorItem[] | string[];
  rawDetails?: string;
  onFix?: () => void;
  fixLabel?: string;
}

export const ValidationErrorModal: React.FC<ValidationErrorModalProps> = ({
  isOpen,
  onClose,
  title = 'Validation Notice',
  subtitle = 'Some entries did not pass schema constraints. Please review before proceeding.',
  errors,
  rawDetails,
  onFix,
  fixLabel = 'Review & Correct'
}) => {
  const [showRaw, setShowRaw] = useState(false);
  const [copiedRaw, setCopiedRaw] = useState(false);

  const normalizedErrors: ValidationErrorItem[] = errors.map((err) =>
    typeof err === 'string' ? { message: err } : err
  );

  const handleCopyRaw = () => {
    if (!rawDetails) return;
    navigator.clipboard?.writeText(rawDetails);
    setCopiedRaw(true);
    setTimeout(() => setCopiedRaw(false), 1500);
  };

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      maxWidth="md"
      className="p-5 sm:p-6"
    >
      <div className="space-y-4">
        {/* Header */}
        <div className="flex items-start gap-3">
          <div className="w-10 h-10 rounded-xl bg-rose-500/10 text-rose-500 ring-1 ring-rose-500/20 flex items-center justify-center shrink-0 mt-0.5">
            <XCircle className="w-5 h-5 text-rose-500" />
          </div>
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <h3 className="text-base font-bold text-neutral-900 dark:text-neutral-100">
                {title}
              </h3>
              <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-rose-500/10 text-rose-500 font-bold shrink-0">
                {normalizedErrors.length} {normalizedErrors.length === 1 ? 'Issue' : 'Issues'}
              </span>
            </div>
            <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-1 leading-relaxed">
              {subtitle}
            </p>
          </div>
        </div>

        {/* List of Validation Issues */}
        <div className="space-y-2 max-h-60 overflow-y-auto pr-1 scrollbar-thin">
          {normalizedErrors.map((item, index) => (
            <div
              key={index}
              className="p-3 rounded-xl border border-rose-500/20 dark:border-rose-500/20 bg-rose-500/5 dark:bg-rose-500/5 flex items-start gap-2.5 text-xs"
            >
              <AlertCircle className="w-3.5 h-3.5 text-rose-500 shrink-0 mt-0.5" />
              <div className="min-w-0 flex-1 space-y-0.5">
                {item.field && (
                  <div className="font-mono font-bold text-neutral-900 dark:text-neutral-100 text-[11px] truncate">
                    {item.field}
                  </div>
                )}
                <div className="text-neutral-700 dark:text-neutral-300 leading-snug">
                  {item.message}
                </div>
                {item.suggestion && (
                  <div className="text-[10px] text-teal-600 dark:text-teal-400 font-mono mt-1">
                    Tip: {item.suggestion}
                  </div>
                )}
              </div>
            </div>
          ))}
        </div>

        {/* Optional Collapsible Technical Raw Details */}
        {rawDetails && (
          <div className="pt-1 border-t border-neutral-100 dark:border-neutral-800">
            <button
              type="button"
              onClick={() => setShowRaw(!showRaw)}
              className="flex items-center justify-between w-full text-[11px] font-mono text-neutral-400 hover:text-neutral-200 transition-colors py-1 cursor-pointer select-none"
            >
              <span className="flex items-center gap-1.5">
                {showRaw ? <ChevronDown className="w-3.5 h-3.5" /> : <ChevronRight className="w-3.5 h-3.5" />}
                Technical Trace / Payload
              </span>
              <span className="text-[10px] text-neutral-500">
                {showRaw ? 'Hide' : 'Show'}
              </span>
            </button>

            {showRaw && (
              <div className="mt-2 relative rounded-lg border border-neutral-800 bg-[#0d0e12] p-3 text-[11px] font-mono text-neutral-300 max-h-36 overflow-y-auto">
                <button
                  type="button"
                  onClick={handleCopyRaw}
                  className="absolute top-2 right-2 p-1 rounded bg-neutral-800 text-neutral-400 hover:text-neutral-200 transition-colors"
                  title="Copy details"
                >
                  {copiedRaw ? <Check className="w-3 h-3 text-emerald-400" /> : <Copy className="w-3 h-3" />}
                </button>
                <pre className="whitespace-pre-wrap leading-relaxed">{rawDetails}</pre>
              </div>
            )}
          </div>
        )}

        {/* Footer Actions */}
        <div className="flex items-center justify-end gap-2.5 pt-2 border-t border-neutral-100 dark:border-neutral-800/80">
          <Button variant="outline" size="sm" onClick={onClose}>
            Dismiss
          </Button>

          {onFix && (
            <Button
              variant="primary"
              size="sm"
              onClick={() => {
                onFix();
                onClose();
              }}
            >
              {fixLabel}
            </Button>
          )}
        </div>
      </div>
    </Modal>
  );
};
