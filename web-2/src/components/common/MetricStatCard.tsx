import React from 'react';

export interface MetricStatCardProps {
  label: string;
  value: React.ReactNode;
  unit?: string;
  subtext?: React.ReactNode;
  icon?: React.ReactNode;
  trend?: {
    value: string | number;
    positive?: boolean;
    neutral?: boolean;
  };
  variant?: 'default' | 'highlight' | 'warning';
  className?: string;
}

export const MetricStatCard: React.FC<MetricStatCardProps> = ({
  label,
  value,
  unit,
  subtext,
  icon,
  trend,
  variant = 'default',
  className = ''
}) => {
  const variantStyles = {
    default: 'border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f]',
    highlight: 'border-teal-500/30 dark:border-teal-500/20 bg-teal-500/5 dark:bg-teal-500/10',
    warning: 'border-amber-500/30 dark:border-amber-500/20 bg-amber-500/5 dark:bg-amber-500/10'
  }[variant];

  return (
    <div
      className={`p-3.5 sm:p-4 rounded-xl border ${variantStyles} shadow-2xs space-y-1.5 transition-all ${className}`}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="text-[10px] font-mono uppercase tracking-wider text-neutral-500 dark:text-neutral-400 truncate">
          {label}
        </span>
        {icon && (
          <div className="w-5 h-5 rounded-md bg-neutral-100 dark:bg-neutral-800 text-neutral-500 dark:text-neutral-400 flex items-center justify-center shrink-0 text-xs">
            {icon}
          </div>
        )}
      </div>

      <div className="flex items-baseline gap-1.5">
        <div className="text-xl sm:text-2xl font-bold font-mono text-neutral-900 dark:text-neutral-100 tabular-nums">
          {value}
        </div>
        {unit && (
          <span className="text-xs font-mono text-neutral-400">
            {unit}
          </span>
        )}
      </div>

      {(subtext || trend) && (
        <div className="flex items-center gap-1.5 text-[10px] text-neutral-400 flex-wrap">
          {trend && (
            <span
              className={`font-semibold font-mono ${
                trend.neutral
                  ? 'text-neutral-400'
                  : trend.positive
                  ? 'text-emerald-500'
                  : 'text-rose-500'
              }`}
            >
              {trend.positive ? '↑' : trend.neutral ? '•' : '↓'} {trend.value}
            </span>
          )}
          {subtext && <span>{subtext}</span>}
        </div>
      )}
    </div>
  );
};
