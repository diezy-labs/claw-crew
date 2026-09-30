import React from 'react';
import { SubMenuScroller } from './SubMenuScroller';

export interface FilterChipItem<T extends string = string> {
  id: T;
  label: string;
  count?: number | string;
  icon?: React.ReactNode;
  badge?: React.ReactNode | boolean;
  color?: string;
}

export interface FilterChipsProps<T extends string = string> {
  items: FilterChipItem<T>[];
  selectedId: T;
  onSelect: (id: T) => void;
  className?: string;
  containerClassName?: string;
  variant?: 'pills' | 'tabs' | 'subtle';
  size?: 'xs' | 'sm' | 'md';
}

export const FilterChips = <T extends string>({
  items,
  selectedId,
  onSelect,
  className = '',
  containerClassName = 'w-full',
  variant = 'pills',
  size = 'sm'
}: FilterChipsProps<T>): React.ReactElement => {
  const sizeClasses = {
    xs: 'px-2.5 py-1 text-[11px] gap-1',
    sm: 'px-3 py-1.5 text-xs gap-1.5',
    md: 'px-3.5 py-2 text-xs sm:text-sm gap-2'
  }[size];

  return (
    <SubMenuScroller className={`gap-1.5 sm:gap-2 ${className}`} containerClassName={containerClassName}>
      {items.map((item) => {
        const isSelected = selectedId === item.id;

        let styleClasses = '';
        if (variant === 'tabs') {
          styleClasses = isSelected
            ? 'bg-neutral-200 dark:bg-neutral-800 text-neutral-900 dark:text-white font-semibold'
            : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200';
        } else if (variant === 'subtle') {
          styleClasses = isSelected
            ? 'bg-teal-500/10 dark:bg-teal-500/15 text-teal-700 dark:text-teal-300 font-semibold border border-teal-500/30'
            : 'text-neutral-600 dark:text-neutral-400 hover:text-neutral-900 dark:hover:text-white border border-neutral-200 dark:border-neutral-800 bg-white/60 dark:bg-[#181a1d]';
        } else {
          // 'pills' (default)
          styleClasses = isSelected
            ? 'bg-neutral-200 dark:bg-neutral-800 text-neutral-900 dark:text-white font-semibold shadow-2xs'
            : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200 border border-transparent';
        }

        return (
          <button
            key={item.id}
            type="button"
            onClick={() => onSelect(item.id)}
            className={`rounded-xl font-medium transition-all shrink-0 cursor-pointer flex items-center whitespace-nowrap select-none ${sizeClasses} ${styleClasses}`}
          >
            {item.icon && <span className="shrink-0">{item.icon}</span>}
            <span>{item.label}</span>
            {item.count !== undefined && (
              <span
                className={`text-[10px] font-mono px-1.5 py-0.2 rounded transition-colors ${
                  isSelected
                    ? 'bg-neutral-300/80 dark:bg-neutral-700 text-neutral-900 dark:text-neutral-100 font-semibold'
                    : 'bg-neutral-200/60 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-400'
                }`}
              >
                {item.count}
              </span>
            )}
            {item.badge === true && (
              <span className="w-1.5 h-1.5 rounded-full bg-amber-500 animate-pulse shrink-0" />
            )}
            {typeof item.badge === 'object' && item.badge}
          </button>
        );
      })}
    </SubMenuScroller>
  );
};
