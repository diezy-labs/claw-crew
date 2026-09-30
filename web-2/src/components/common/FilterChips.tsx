import React from 'react';
import { SubMenuScroller } from './SubMenuScroller';

export interface FilterChipItem<T extends string = string> {
  id: T;
  label: string;
  count?: number | string;
  icon?: React.ReactNode;
  badge?: React.ReactNode | boolean;
  color?: string;
  disabled?: boolean;
}

export interface ChipProps {
  id?: string;
  label: React.ReactNode;
  count?: number | string;
  icon?: React.ReactNode;
  badge?: React.ReactNode | boolean;
  selected?: boolean;
  disabled?: boolean;
  onClick?: () => void;
  variant?: 'subtle' | 'pills' | 'tabs';
  size?: 'xs' | 'sm' | 'md';
  className?: string;
  style?: React.CSSProperties;
}

export const Chip: React.FC<ChipProps> = ({
  label,
  count,
  icon,
  badge,
  selected = false,
  disabled = false,
  onClick,
  variant = 'subtle',
  size = 'sm',
  className = '',
  style = {}
}) => {
  const sizeClasses = {
    xs: 'px-2.5 py-1 text-[11px] gap-1',
    sm: 'px-3 py-1.5 text-xs gap-1.5',
    md: 'px-3.5 py-2 text-xs sm:text-sm gap-2'
  }[size];

  // Dynamic theme-aware styles
  let dynamicStyle: React.CSSProperties = { ...style };
  let dynamicClasses = '';

  if (variant === 'tabs') {
    if (selected) {
      dynamicClasses = 'active-theme-chip font-semibold shadow-2xs text-white';
      dynamicStyle = {
        backgroundColor: 'rgba(var(--brand-primary-rgb, 13, 148, 136), 0.22)',
        color: '#ffffff',
        borderColor: 'rgba(var(--brand-primary-rgb, 13, 148, 136), 0.45)',
        ...dynamicStyle
      };
    } else {
      dynamicClasses = 'hover:bg-neutral-100 dark:hover:bg-neutral-800/60 transition-colors';
      dynamicStyle = {
        color: 'var(--text-secondary, #a7adb5)',
        ...dynamicStyle
      };
    }
  } else {
    // 'subtle' (Ships page style) or 'pills'
    if (selected) {
      dynamicClasses = 'active-theme-chip font-semibold border shadow-2xs active:scale-[0.98] transition-all text-white';
      dynamicStyle = {
        backgroundColor: 'rgba(var(--brand-primary-rgb, 13, 148, 136), 0.16)',
        borderColor: 'rgba(var(--brand-primary-rgb, 13, 148, 136), 0.45)',
        color: '#ffffff',
        boxShadow: '0 0 12px rgba(var(--brand-primary-rgb, 13, 148, 136), 0.16)',
        ...dynamicStyle
      };
    } else {
      dynamicClasses = 'border font-medium hover:border-[var(--brand-primary)]/40 hover:shadow-2xs active:scale-[0.98] transition-all';
      dynamicStyle = {
        backgroundColor: 'var(--bg-surface, #191b1f)',
        borderColor: 'var(--border-subtle, #2c3036)',
        color: 'var(--text-secondary, #a7adb5)',
        ...dynamicStyle
      };
    }
  }

  return (
    <button
      type="button"
      onClick={disabled ? undefined : onClick}
      disabled={disabled}
      style={dynamicStyle}
      className={`rounded-xl shrink-0 cursor-pointer flex items-center whitespace-nowrap select-none disabled:opacity-50 disabled:cursor-not-allowed ${sizeClasses} ${dynamicClasses} ${className}`}
    >
      {icon && (
        <span
          style={selected ? { color: '#ffffff' } : undefined}
          className="shrink-0 transition-colors opacity-95 group-hover:opacity-100"
        >
          {icon}
        </span>
      )}
      <span className="truncate" style={selected ? { color: '#ffffff' } : undefined}>
        {label}
      </span>
      {count !== undefined && (
        <span
          style={
            selected
              ? {
                  backgroundColor: 'rgba(var(--brand-primary-rgb, 13, 148, 136), 0.28)',
                  color: '#ffffff'
                }
              : {
                  backgroundColor: 'var(--bg-elevated, #22252a)',
                  color: 'var(--text-muted, #747c86)'
                }
          }
          className="text-[10px] font-mono px-1.5 py-0.2 rounded transition-colors font-medium ml-0.5"
        >
          {count}
        </span>
      )}
      {badge === true && (
        <span className="w-1.5 h-1.5 rounded-full bg-amber-400 animate-pulse shrink-0" />
      )}
      {typeof badge === 'object' && badge}
    </button>
  );
};

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
  variant = 'subtle',
  size = 'sm'
}: FilterChipsProps<T>): React.ReactElement => {
  return (
    <SubMenuScroller className={`gap-1.5 sm:gap-2 ${className}`} containerClassName={containerClassName}>
      {items.map((item) => (
        <Chip
          key={item.id}
          label={item.label}
          count={item.count}
          icon={item.icon}
          badge={item.badge}
          selected={selectedId === item.id}
          disabled={item.disabled}
          onClick={() => onSelect(item.id)}
          variant={variant}
          size={size}
        />
      ))}
    </SubMenuScroller>
  );
};
