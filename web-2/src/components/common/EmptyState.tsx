import React from 'react';

export interface EmptyStateProps {
  icon?: React.ReactNode;
  title: string;
  description?: string;
  action?: React.ReactNode;
  className?: string;
}

export const EmptyState: React.FC<EmptyStateProps> = ({
  icon,
  title,
  description,
  action,
  className = ''
}) => {
  return (
    <div
      className={`flex flex-col items-center justify-center text-center p-8 sm:p-12 rounded-2xl border border-dashed border-neutral-300 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-900/20 max-w-md mx-auto my-6 animate-in fade-in duration-200 ${className}`}
    >
      {icon && (
        <div className="w-12 h-12 rounded-2xl bg-teal-500/10 text-teal-600 dark:text-teal-400 flex items-center justify-center mb-3 ring-1 ring-teal-500/20 shrink-0">
          {icon}
        </div>
      )}
      <h3 className="text-sm sm:text-base font-bold text-neutral-900 dark:text-neutral-100 mb-1">
        {title}
      </h3>
      {description && (
        <p className="text-xs text-neutral-500 dark:text-neutral-400 max-w-xs leading-relaxed mb-4">
          {description}
        </p>
      )}
      {action && <div className="mt-1">{action}</div>}
    </div>
  );
};
