import React, { useEffect, useRef } from 'react';
import { createPortal } from 'react-dom';
import { X } from 'lucide-react';

export interface ModalProps {
  isOpen: boolean;
  onClose: () => void;
  title?: React.ReactNode;
  subtitle?: React.ReactNode;
  icon?: React.ReactNode;
  badge?: React.ReactNode;
  children: React.ReactNode;
  footer?: React.ReactNode;
  maxWidth?: 'sm' | 'md' | 'lg' | 'xl' | '2xl' | '3xl' | '4xl';
  className?: string;
  showCloseButton?: boolean;
  preventBackdropClose?: boolean;
  variant?: 'center' | 'sheet-right';
}

const maxWidthMap = {
  sm: 'max-w-sm',
  md: 'max-w-md',
  lg: 'max-w-lg',
  xl: 'max-w-xl',
  '2xl': 'max-w-2xl',
  '3xl': 'max-w-3xl',
  '4xl': 'max-w-4xl'
};

export const Modal: React.FC<ModalProps> = ({
  isOpen,
  onClose,
  title,
  subtitle,
  icon,
  badge,
  children,
  footer,
  maxWidth = 'lg',
  className = '',
  showCloseButton = false,
  preventBackdropClose = false,
  variant = 'center'
}) => {
  const modalContentRef = useRef<HTMLDivElement>(null);

  // Close on Escape key press & prevent background scroll
  useEffect(() => {
    if (!isOpen) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onClose();
      }
    };

    const prevOverflow = document.documentElement.style.overflow;
    document.documentElement.style.overflow = 'hidden';
    window.addEventListener('keydown', handleKeyDown);

    return () => {
      document.documentElement.style.overflow = prevOverflow;
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  const isSheet = variant === 'sheet-right';

  const content = (
    <div
      role="dialog"
      aria-modal="true"
      onClick={() => {
        if (!preventBackdropClose) onClose();
      }}
      className={`fixed inset-0 z-50 bg-black/65 backdrop-blur-xs animate-in fade-in duration-150 ${
        isSheet ? 'flex justify-end' : 'flex items-center justify-center p-3 sm:p-4 overflow-y-auto'
      }`}
    >
      <div
        ref={modalContentRef}
        onClick={(e) => e.stopPropagation()}
        className={
          isSheet
            ? `w-full sm:w-[500px] h-full flex flex-col bg-white dark:bg-[#181a1e] border-l border-neutral-200 dark:border-neutral-800 shadow-2xl animate-in slide-in-from-right duration-200 cursor-default ${className}`
            : `w-full ${maxWidthMap[maxWidth]} my-auto max-h-[88vh] flex flex-col rounded-2xl border border-teal-500/30 dark:border-teal-500/25 bg-white dark:bg-[#181a1e] shadow-2xl overflow-hidden animate-in zoom-in-95 duration-150 cursor-default ${className}`
        }
      >
        {/* Header */}
        {(title || icon || badge || showCloseButton) && (
          <div className="px-5 py-4 sm:px-6 sm:py-4.5 border-b border-neutral-100 dark:border-neutral-800/80 flex items-start justify-between gap-3 shrink-0">
            <div className="flex items-start gap-3 min-w-0">
              {icon && (
                <div className="w-8 h-8 rounded-lg bg-teal-500/10 text-teal-600 dark:text-teal-400 flex items-center justify-center shrink-0 ring-1 ring-teal-500/20 mt-0.5">
                  {icon}
                </div>
              )}
              <div className="min-w-0">
                <div className="flex items-center gap-2 flex-wrap">
                  {typeof title === 'string' ? (
                    <h2 className="text-base sm:text-lg font-bold text-neutral-900 dark:text-neutral-100 leading-tight">
                      {title}
                    </h2>
                  ) : (
                    title
                  )}
                  {badge && <div className="shrink-0">{badge}</div>}
                </div>
                {subtitle && (
                  <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-1 leading-relaxed">
                    {subtitle}
                  </p>
                )}
              </div>
            </div>

            <div className="flex items-center gap-2 shrink-0">
              <span className="text-[10px] font-mono text-neutral-400 bg-neutral-100 dark:bg-neutral-850 border border-neutral-200 dark:border-neutral-750 px-2 py-0.5 rounded select-none hidden sm:inline-block">
                ESC to close
              </span>
              {showCloseButton && (
                <button
                  type="button"
                  onClick={onClose}
                  aria-label="Close dialog"
                  className="p-1 rounded-lg text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200 hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors cursor-pointer"
                >
                  <X className="w-4 h-4" />
                </button>
              )}
            </div>
          </div>
        )}

        {/* Scrollable Body */}
        <div className="flex-1 overflow-y-auto p-5 sm:p-6 space-y-4 text-xs">
          {children}
        </div>

        {/* Footer */}
        {footer && (
          <div className="px-5 py-3.5 sm:px-6 sm:py-4 border-t border-neutral-100 dark:border-neutral-800/80 bg-neutral-50/50 dark:bg-neutral-900/30 flex items-center justify-end gap-2.5 shrink-0">
            {footer}
          </div>
        )}
      </div>
    </div>
  );

  return typeof document !== 'undefined' ? createPortal(content, document.body) : content;
};
