import React from 'react';

export type StatusType =
  | 'active'
  | 'running'
  | 'healthy'
  | 'nominal'
  | 'completed'
  | 'approved'
  | 'success'
  | 'pending'
  | 'warning'
  | 'underway'
  | 'planning'
  | 'idle'
  | 'error'
  | 'failed'
  | 'stopped'
  | 'rejected'
  | 'draft'
  | 'info'
  | 'neutral';

export interface StatusBadgeProps {
  status: StatusType | string;
  label?: React.ReactNode;
  dot?: boolean;
  pulse?: boolean;
  size?: 'xs' | 'sm' | 'md';
  variant?: 'subtle' | 'outline' | 'solid';
  className?: string;
}

const statusColorStyles: Record<
  string,
  { bg: string; text: string; border: string; dot: string }
> = {
  active: {
    bg: 'bg-emerald-500/10 dark:bg-emerald-500/15',
    text: 'text-emerald-700 dark:text-emerald-400',
    border: 'border-emerald-500/25',
    dot: 'bg-emerald-500'
  },
  running: {
    bg: 'bg-emerald-500/10 dark:bg-emerald-500/15',
    text: 'text-emerald-700 dark:text-emerald-400',
    border: 'border-emerald-500/25',
    dot: 'bg-emerald-500'
  },
  healthy: {
    bg: 'bg-emerald-500/10 dark:bg-emerald-500/15',
    text: 'text-emerald-700 dark:text-emerald-400',
    border: 'border-emerald-500/25',
    dot: 'bg-emerald-500'
  },
  nominal: {
    bg: 'bg-emerald-500/10 dark:bg-emerald-500/15',
    text: 'text-emerald-700 dark:text-emerald-400',
    border: 'border-emerald-500/25',
    dot: 'bg-emerald-500'
  },
  completed: {
    bg: 'bg-teal-500/10 dark:bg-teal-500/15',
    text: 'text-teal-700 dark:text-teal-400',
    border: 'border-teal-500/25',
    dot: 'bg-teal-500'
  },
  approved: {
    bg: 'bg-teal-500/10 dark:bg-teal-500/15',
    text: 'text-teal-700 dark:text-teal-400',
    border: 'border-teal-500/25',
    dot: 'bg-teal-500'
  },
  success: {
    bg: 'bg-emerald-500/10 dark:bg-emerald-500/15',
    text: 'text-emerald-700 dark:text-emerald-400',
    border: 'border-emerald-500/25',
    dot: 'bg-emerald-500'
  },
  underway: {
    bg: 'bg-cyan-500/10 dark:bg-cyan-500/15',
    text: 'text-cyan-700 dark:text-cyan-400',
    border: 'border-cyan-500/25',
    dot: 'bg-cyan-500'
  },
  planning: {
    bg: 'bg-indigo-500/10 dark:bg-indigo-500/15',
    text: 'text-indigo-700 dark:text-indigo-400',
    border: 'border-indigo-500/25',
    dot: 'bg-indigo-500'
  },
  pending: {
    bg: 'bg-amber-500/10 dark:bg-amber-500/15',
    text: 'text-amber-700 dark:text-amber-400',
    border: 'border-amber-500/25',
    dot: 'bg-amber-500'
  },
  warning: {
    bg: 'bg-amber-500/10 dark:bg-amber-500/15',
    text: 'text-amber-700 dark:text-amber-400',
    border: 'border-amber-500/25',
    dot: 'bg-amber-500'
  },
  idle: {
    bg: 'bg-neutral-500/10 dark:bg-neutral-500/15',
    text: 'text-neutral-700 dark:text-neutral-400',
    border: 'border-neutral-500/20',
    dot: 'bg-neutral-400'
  },
  draft: {
    bg: 'bg-neutral-500/10 dark:bg-neutral-500/15',
    text: 'text-neutral-700 dark:text-neutral-400',
    border: 'border-neutral-500/20',
    dot: 'bg-neutral-400'
  },
  error: {
    bg: 'bg-rose-500/10 dark:bg-rose-500/15',
    text: 'text-rose-700 dark:text-rose-400',
    border: 'border-rose-500/25',
    dot: 'bg-rose-500'
  },
  failed: {
    bg: 'bg-rose-500/10 dark:bg-rose-500/15',
    text: 'text-rose-700 dark:text-rose-400',
    border: 'border-rose-500/25',
    dot: 'bg-rose-500'
  },
  stopped: {
    bg: 'bg-rose-500/10 dark:bg-rose-500/15',
    text: 'text-rose-700 dark:text-rose-400',
    border: 'border-rose-500/25',
    dot: 'bg-rose-500'
  },
  rejected: {
    bg: 'bg-rose-500/10 dark:bg-rose-500/15',
    text: 'text-rose-700 dark:text-rose-400',
    border: 'border-rose-500/25',
    dot: 'bg-rose-500'
  },
  info: {
    bg: 'bg-blue-500/10 dark:bg-blue-500/15',
    text: 'text-blue-700 dark:text-blue-400',
    border: 'border-blue-500/25',
    dot: 'bg-blue-500'
  },
  neutral: {
    bg: 'bg-neutral-200/70 dark:bg-neutral-800',
    text: 'text-neutral-700 dark:text-neutral-300',
    border: 'border-neutral-300 dark:border-neutral-700',
    dot: 'bg-neutral-400'
  }
};

export const StatusBadge: React.FC<StatusBadgeProps> = ({
  status,
  label,
  dot = true,
  pulse,
  size = 'sm',
  variant = 'subtle',
  className = ''
}) => {
  const normalizedKey = String(status).toLowerCase();
  const theme = statusColorStyles[normalizedKey] || statusColorStyles.neutral;

  const sizeClasses = {
    xs: 'px-1.5 py-0.2 text-[9px]',
    sm: 'px-2 py-0.5 text-[10px]',
    md: 'px-2.5 py-1 text-xs'
  }[size];

  const dotSizes = {
    xs: 'w-1 h-1',
    sm: 'w-1.5 h-1.5',
    md: 'w-2 h-2'
  }[size];

  const isPulseActive = pulse !== undefined ? pulse : normalizedKey === 'running' || normalizedKey === 'active' || normalizedKey === 'underway';

  const displayLabel = label !== undefined ? label : status;

  return (
    <span
      className={`inline-flex items-center gap-1.5 font-mono font-medium rounded uppercase tracking-wider select-none shrink-0 border ${
        variant === 'outline'
          ? `bg-transparent ${theme.text} ${theme.border}`
          : `${theme.bg} ${theme.text} ${theme.border}`
      } ${sizeClasses} ${className}`}
    >
      {dot && (
        <span
          className={`rounded-full shrink-0 ${theme.dot} ${dotSizes} ${
            isPulseActive ? 'animate-pulse' : ''
          }`}
        />
      )}
      <span className="truncate">{displayLabel}</span>
    </span>
  );
};
