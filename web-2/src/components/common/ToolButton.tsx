import React from 'react';

export type ToolButtonVariant = 'default' | 'primary' | 'subtle' | 'danger' | 'amber';
export type ToolButtonSize = 'xs' | 'sm' | 'md';

export interface ToolButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  icon?: React.ReactNode;
  label?: string;
  shortLabel?: string;
  hideLabelOnMobile?: boolean;
  active?: boolean;
  variant?: ToolButtonVariant;
  size?: ToolButtonSize;
  className?: string;
  style?: React.CSSProperties;
}

/**
 * ToolButton — A standardized, theme-aware compact tool & action button
 * Handles responsive label display (icon only on mobile, label on desktop)
 * to prevent buttons from overflowing or protruding on mobile and constrained panels.
 */
export const ToolButton: React.FC<ToolButtonProps> = ({
  icon,
  label,
  shortLabel,
  hideLabelOnMobile = true,
  active = false,
  variant = 'default',
  size = 'sm',
  className = '',
  style = {},
  disabled,
  title,
  ...props
}) => {
  const sizeClasses = {
    xs: 'px-2 py-1 text-[11px] gap-1 rounded-md h-7',
    sm: 'px-2.5 py-1.5 text-xs gap-1.5 rounded-lg h-8',
    md: 'px-3.5 py-2 text-xs sm:text-sm gap-2 rounded-xl h-9'
  }[size];

  let variantStyle: React.CSSProperties = { ...style };
  let variantClasses = '';

  if (active || variant === 'primary') {
    variantClasses = 'font-semibold border shadow-2xs';
    variantStyle = {
      backgroundColor: 'rgba(var(--brand-primary-rgb, 13, 148, 136), 0.16)',
      borderColor: 'rgba(var(--brand-primary-rgb, 13, 148, 136), 0.45)',
      color: 'var(--brand-primary, #2dd4bf)',
      ...variantStyle
    };
  } else if (variant === 'danger') {
    variantClasses = 'bg-rose-500/10 hover:bg-rose-500/20 text-rose-600 dark:text-rose-400 border border-rose-500/30';
  } else if (variant === 'amber') {
    variantClasses = 'bg-amber-500/10 hover:bg-amber-500/20 text-amber-600 dark:text-amber-400 border border-amber-500/30';
  } else if (variant === 'subtle') {
    variantClasses = 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-100 hover:bg-neutral-100 dark:hover:bg-neutral-800/80 border border-transparent';
  } else {
    // 'default'
    variantClasses = 'border font-medium hover:text-[var(--text-primary)] transition-colors shadow-2xs';
    variantStyle = {
      backgroundColor: 'var(--bg-surface, #191b1f)',
      borderColor: 'var(--border-subtle, #2c3036)',
      color: 'var(--text-secondary, #a7adb5)',
      ...variantStyle
    };
  }

  const tooltipText = title || label || shortLabel;

  return (
    <button
      type="button"
      disabled={disabled}
      title={tooltipText}
      aria-label={tooltipText}
      style={variantStyle}
      className={`inline-flex items-center justify-center shrink-0 whitespace-nowrap select-none cursor-pointer transition-all active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed ${sizeClasses} ${variantClasses} ${className}`}
      {...props}
    >
      {icon && <span className="shrink-0 flex items-center justify-center">{icon}</span>}
      {label && (
        <span
          className={
            hideLabelOnMobile
              ? shortLabel
                ? 'hidden sm:inline truncate'
                : 'hidden md:inline truncate'
              : 'truncate'
          }
        >
          {label}
        </span>
      )}
      {shortLabel && hideLabelOnMobile && (
        <span className="inline sm:hidden truncate">{shortLabel}</span>
      )}
    </button>
  );
};
