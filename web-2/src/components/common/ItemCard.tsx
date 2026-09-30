import React from 'react';

export interface ItemCardProps {
  title: React.ReactNode;
  subtitle?: React.ReactNode;
  description?: React.ReactNode;
  icon?: React.ReactNode;
  badge?: React.ReactNode;
  meta?: React.ReactNode;
  tags?: string[];
  footer?: React.ReactNode;
  actions?: React.ReactNode;
  onClick?: () => void;
  selected?: boolean;
  className?: string;
  compact?: boolean;
  children?: React.ReactNode;
  descriptionClamp?: 1 | 2 | 3 | 4 | 'none';
  accentColor?: 'teal' | 'amber' | 'blue' | 'rose' | 'neutral';
}

const clampClasses = {
  1: 'line-clamp-1',
  2: 'line-clamp-2',
  3: 'line-clamp-3',
  4: 'line-clamp-4',
  none: ''
};

export const ItemCard: React.FC<ItemCardProps> = ({
  title,
  subtitle,
  description,
  icon,
  badge,
  meta,
  tags,
  footer,
  actions,
  onClick,
  selected = false,
  className = '',
  compact = false,
  children,
  descriptionClamp = 3,
  accentColor = 'teal'
}) => {
  const isClickable = Boolean(onClick);

  const selectedBorderMap = {
    teal: 'border-teal-500 bg-teal-500/5 shadow-md ring-1 ring-teal-500/25',
    amber: 'border-amber-500 bg-amber-500/5 shadow-md ring-1 ring-amber-500/25',
    blue: 'border-blue-500 bg-blue-500/5 shadow-md ring-1 ring-blue-500/25',
    rose: 'border-rose-500 bg-rose-500/5 shadow-md ring-1 ring-rose-500/25',
    neutral: 'border-neutral-500 bg-neutral-500/5 shadow-md ring-1 ring-neutral-500/25'
  };

  const hoverBorderMap = {
    teal: 'hover:border-teal-500/60 dark:hover:border-teal-500/50',
    amber: 'hover:border-amber-500/60 dark:hover:border-amber-500/50',
    blue: 'hover:border-blue-500/60 dark:hover:border-blue-500/50',
    rose: 'hover:border-rose-500/60 dark:hover:border-rose-500/50',
    neutral: 'hover:border-neutral-400 dark:hover:border-neutral-600'
  };

  return (
    <div
      onClick={onClick}
      role={isClickable ? 'button' : undefined}
      tabIndex={isClickable ? 0 : undefined}
      onKeyDown={
        isClickable
          ? (e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault();
                onClick?.();
              }
            }
          : undefined
      }
      className={`rounded-xl border transition-all flex flex-col justify-between select-none ${
        compact ? 'p-3 sm:p-3.5 space-y-2' : 'p-4 sm:p-5 space-y-3'
      } ${isClickable ? 'cursor-pointer' : ''} ${
        selected
          ? selectedBorderMap[accentColor]
          : `border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] ${hoverBorderMap[accentColor]} hover:shadow-xs`
      } ${className}`}
    >
      <div className={compact ? 'space-y-2' : 'space-y-2.5'}>
        {/* Header: Title + Subtitle + Badge/Actions */}
        <div className="flex items-start justify-between gap-2">
          <div className="flex items-start gap-2.5 min-w-0 flex-1">
            {icon && (
              <div className={`rounded-lg bg-teal-500/10 text-teal-600 dark:text-teal-400 flex items-center justify-center shrink-0 ring-1 ring-teal-500/20 mt-0.5 ${
                compact ? 'w-6 h-6' : 'w-7 h-7'
              }`}>
                {icon}
              </div>
            )}
            <div className="min-w-0 flex-1">
              <div className={`font-bold text-neutral-900 dark:text-neutral-100 truncate leading-snug ${
                compact ? 'text-xs' : 'text-sm'
              }`}>
                {title}
              </div>
              {subtitle && (
                <div className="text-[11px] text-neutral-500 dark:text-neutral-400 truncate mt-0.5">
                  {subtitle}
                </div>
              )}
            </div>
          </div>

          <div
            className="flex items-center gap-1.5 shrink-0"
            onClick={(e) => actions && e.stopPropagation()}
          >
            {badge}
            {actions}
          </div>
        </div>

        {/* Optional Meta info bar */}
        {meta && (
          <div className="text-[11px] text-neutral-500 dark:text-neutral-400 pt-0.5">
            {meta}
          </div>
        )}

        {/* Description */}
        {description && (
          <div
            className={`text-neutral-600 dark:text-neutral-400 leading-relaxed ${
              compact ? 'text-[11px]' : 'text-xs'
            } ${clampClasses[descriptionClamp]}`}
          >
            {description}
          </div>
        )}

        {/* Optional Custom Middle Children (Progress bars, mini stats, etc.) */}
        {children}

        {/* Tags / Pills */}
        {tags && tags.length > 0 && (
          <div className="flex flex-wrap gap-1.5 pt-0.5">
            {tags.map((tag) => (
              <span
                key={tag}
                className="px-2 py-0.5 rounded text-[10px] font-mono bg-neutral-100 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-400 border border-neutral-200/50 dark:border-neutral-700/50"
              >
                {tag}
              </span>
            ))}
          </div>
        )}
      </div>

      {/* Footer */}
      {footer && (
        <div className="pt-2.5 mt-2 border-t border-neutral-100 dark:border-neutral-800/80 text-xs text-neutral-500 dark:text-neutral-400">
          {footer}
        </div>
      )}
    </div>
  );
};
