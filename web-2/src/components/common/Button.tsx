import React from 'react';

export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger' | 'amber' | 'outline';
export type ButtonSize = 'xs' | 'sm' | 'md' | 'lg' | 'icon';

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  icon?: React.ReactNode;
  rightIcon?: React.ReactNode;
  shortLabel?: string;
  loading?: boolean;
}

export const Button: React.FC<ButtonProps> = ({
  children,
  variant = 'primary',
  size = 'md',
  icon,
  rightIcon,
  shortLabel,
  loading = false,
  className = '',
  disabled,
  ...props
}) => {
  // Variant styles
  const variantStyles: Record<ButtonVariant, string> = {
    primary:
      'bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 hover:opacity-90 active:scale-[0.98] shadow-xs font-semibold',
    secondary:
      'border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-neutral-800 dark:text-neutral-200 hover:bg-neutral-50 dark:hover:bg-neutral-900 shadow-2xs font-medium',
    outline:
      'border border-neutral-300 dark:border-neutral-700 bg-transparent text-neutral-700 dark:text-neutral-300 hover:border-teal-500/50 hover:text-teal-600 dark:hover:text-teal-400 font-medium',
    ghost:
      'text-neutral-600 dark:text-neutral-400 hover:text-neutral-900 dark:hover:text-neutral-100 hover:bg-neutral-100 dark:hover:bg-neutral-800/80 font-medium',
    danger:
      'bg-rose-600 text-white hover:bg-rose-700 active:scale-[0.98] shadow-xs font-semibold',
    amber:
      'bg-amber-500 text-neutral-950 hover:bg-amber-400 active:scale-[0.98] shadow-xs font-semibold'
  };

  // Size styles
  const sizeStyles: Record<ButtonSize, string> = {
    xs: 'px-2 py-1 text-[11px] gap-1 rounded-md',
    sm: 'px-2.5 py-1 text-xs gap-1.5 rounded-lg',
    md: 'px-3 py-1.5 text-xs gap-1.5 rounded-lg',
    lg: 'px-4 py-2 text-sm gap-2 rounded-xl',
    icon: 'p-1.5 rounded-lg shrink-0'
  };

  return (
    <button
      {...props}
      disabled={disabled || loading}
      className={`inline-flex items-center justify-center whitespace-nowrap shrink-0 transition-all select-none cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed ${variantStyles[variant]} ${sizeStyles[size]} ${className}`}
    >
      {loading ? (
        <span className="w-3.5 h-3.5 border-2 border-current border-t-transparent rounded-full animate-spin shrink-0" />
      ) : (
        icon && <span className="shrink-0">{icon}</span>
      )}

      {children && (
        <>
          {shortLabel ? (
            <>
              <span className="hidden sm:inline truncate">{children}</span>
              <span className="sm:hidden truncate">{shortLabel}</span>
            </>
          ) : (
            <span className="truncate">{children}</span>
          )}
        </>
      )}

      {rightIcon && !loading && <span className="shrink-0">{rightIcon}</span>}
    </button>
  );
};
