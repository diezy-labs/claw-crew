import React, { useEffect, useRef } from 'react';
import { createPortal } from 'react-dom';
import { X } from 'lucide-react';

export interface CardPopoverProps {
  isOpen: boolean;
  onClose: () => void;
  title: React.ReactNode;
  subtitle?: React.ReactNode;
  icon?: React.ReactNode;
  badge?: React.ReactNode;
  headerActions?: React.ReactNode;
  children: React.ReactNode;
  footer?: React.ReactNode;
  variant?: 'sheet-right' | 'center';
  maxWidth?: 'sm' | 'md' | 'lg' | 'xl' | '2xl' | '3xl' | '4xl';
  drawerWidth?: string; // e.g. 'sm:w-[540px]'
  className?: string;
  preventBackdropClose?: boolean;
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

export const CardPopover: React.FC<CardPopoverProps> = ({
  isOpen,
  onClose,
  title,
  subtitle,
  icon,
  badge,
  headerActions,
  children,
  footer,
  variant = 'sheet-right',
  maxWidth = 'lg',
  drawerWidth = 'sm:w-[540px]',
  className = '',
  preventBackdropClose = false
}) => {
  const contentRef = useRef<HTMLDivElement>(null);

  // Close on Escape key press & prevent background scroll without jump
  useEffect(() => {
    if (!isOpen) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onClose();
      }
    };

    window.addEventListener('keydown', handleKeyDown);

    // Save previous overflow style and prevent body scroll
    const prevOverflow = document.documentElement.style.overflow;
    document.documentElement.style.overflow = 'hidden';

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
      className={`fixed inset-0 z-50 bg-black/60 backdrop-blur-xs transition-opacity duration-200 ${
        isSheet
          ? 'flex justify-end'
          : 'flex items-center justify-center p-3 sm:p-5 overflow-y-auto'
      }`}
    >
      <div
        ref={contentRef}
        onClick={(e) => e.stopPropagation()}
        className={
          isSheet
            ? `w-full ${drawerWidth} h-full h-[100dvh] flex flex-col bg-white dark:bg-[#181a1e] border-l border-neutral-200 dark:border-neutral-800 shadow-2xl transition-transform duration-200 cursor-default animate-slide-in-right ${className}`
            : `w-full ${maxWidthMap[maxWidth]} my-auto max-h-[88vh] flex flex-col rounded-2xl border border-teal-500/30 dark:border-teal-500/25 bg-white dark:bg-[#181a1e] shadow-2xl overflow-hidden cursor-default transition-all duration-200 animate-view-fade-in ${className}`
        }
      >
        {/* Popover Header */}
        <div className="px-5 py-4 sm:px-6 sm:py-4.5 border-b border-neutral-100 dark:border-neutral-800/80 flex items-start justify-between gap-3 shrink-0 bg-neutral-50/50 dark:bg-neutral-900/30">
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
            {headerActions}
            <span className="text-[10px] font-mono text-neutral-400 bg-neutral-100 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 px-2 py-0.5 rounded select-none hidden sm:inline-block">
              ESC
            </span>
            <button
              type="button"
              onClick={onClose}
              aria-label="Close"
              className="p-1.5 rounded-lg text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200 hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors cursor-pointer"
            >
              <X className="w-4 h-4" />
            </button>
          </div>
        </div>

        {/* Popover Scrollable Body */}
        <div className="flex-1 overflow-y-auto p-5 sm:p-6 space-y-4 text-xs">
          {children}
        </div>

        {/* Popover Footer */}
        {footer && (
          <div className="px-5 py-3.5 sm:px-6 sm:py-4 border-t border-neutral-100 dark:border-neutral-800/80 bg-neutral-50/70 dark:bg-neutral-900/40 flex items-center justify-end gap-2.5 shrink-0">
            {footer}
          </div>
        )}
      </div>
    </div>
  );

  return typeof document !== 'undefined' ? createPortal(content, document.body) : content;
};
