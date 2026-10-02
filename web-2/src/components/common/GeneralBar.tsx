import React from 'react';

export interface GeneralBarProps {
  children?: React.ReactNode;
  leftContent?: React.ReactNode;
  centerContent?: React.ReactNode;
  rightContent?: React.ReactNode;
  variant?: 'nav' | 'header' | 'section' | 'status' | 'action';
  sticky?: boolean;
  blur?: boolean;
  accentLine?: boolean | 'top' | 'bottom';
  className?: string;
  style?: React.CSSProperties;
  padding?: 'none' | 'xs' | 'sm' | 'md' | 'lg';
}

/**
 * GeneralBar — A standardized, theme-aware bar component
 * Adapts its surface background, border, typography, and glowing accent line
 * to active custom themes (VS Code, Dracula, One Dark, etc.)
 */
export const GeneralBar: React.FC<GeneralBarProps> = ({
  children,
  leftContent,
  centerContent,
  rightContent,
  variant = 'nav',
  sticky = false,
  blur = true,
  accentLine = false,
  className = '',
  style = {},
  padding = 'md'
}) => {
  const paddingClasses = {
    none: 'p-0',
    xs: 'px-3 py-1.5',
    sm: 'px-4 py-2',
    md: 'px-4 sm:px-6 py-2.5 sm:py-3',
    lg: 'px-5 sm:px-8 py-3.5 sm:py-4'
  }[padding];

  let variantStyles: React.CSSProperties = {
    backgroundColor: 'var(--bg-surface, #191b1f)',
    borderColor: 'var(--border-subtle, #2c3036)',
    ...style
  };

  let baseClasses = 'w-full flex items-center justify-between gap-3 shrink-0 relative transition-colors';

  if (variant === 'nav' || variant === 'header') {
    baseClasses += ' border-b shadow-2xs';
    if (blur) {
      baseClasses += ' backdrop-blur-md';
      variantStyles.backgroundColor = 'var(--bg-surface, #191b1f)';
    }
  } else if (variant === 'section') {
    baseClasses += ' border-y bg-opacity-70';
    variantStyles.backgroundColor = 'var(--bg-elevated, #22252a)';
  } else if (variant === 'status') {
    baseClasses += ' border rounded-xl text-xs';
    variantStyles.backgroundColor = 'var(--bg-elevated, #22252a)';
  } else if (variant === 'action') {
    baseClasses += ' border-t shadow-lg rounded-t-2xl';
    variantStyles.backgroundColor = 'var(--bg-surface, #191b1f)';
  }

  if (sticky) {
    baseClasses += ' sticky top-0 z-20';
  }

  const showTopAccent = accentLine === true || accentLine === 'top';
  const showBottomAccent = accentLine === 'bottom';

  return (
    <div className={`${baseClasses} ${paddingClasses} ${className}`} style={variantStyles}>
      {/* Top Luminous Accent Line */}
      {showTopAccent && (
        <div
          className="absolute top-0 inset-x-0 h-[2px] opacity-85 pointer-events-none"
          style={{
            background: 'linear-gradient(90deg, transparent 0%, var(--brand-primary, #2dd4bf) 50%, transparent 100%)'
          }}
        />
      )}

      {/* Render Structured or Direct Content */}
      {leftContent || centerContent || rightContent ? (
        <>
          <div className="flex items-center gap-2.5 min-w-0">
            {leftContent}
          </div>

          {centerContent && (
            <div className="flex-1 flex items-center justify-center min-w-0 px-2">
              {centerContent}
            </div>
          )}

          {rightContent && (
            <div className="flex items-center gap-2 shrink-0">
              {rightContent}
            </div>
          )}
        </>
      ) : (
        children
      )}

      {/* Bottom Luminous Accent Line */}
      {showBottomAccent && (
        <div
          className="absolute bottom-0 inset-x-0 h-[2px] opacity-85 pointer-events-none"
          style={{
            background: 'linear-gradient(90deg, transparent 0%, var(--brand-primary, #2dd4bf) 50%, transparent 100%)'
          }}
        />
      )}
    </div>
  );
};
