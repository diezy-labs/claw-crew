import React from 'react';

export interface HeaderToolbarProps {
  children?: React.ReactNode;
  leftContent?: React.ReactNode;
  rightContent?: React.ReactNode;
  className?: string;
  style?: React.CSSProperties;
  borderBottom?: boolean;
}

/**
 * HeaderToolbar — A responsive header/sub-header bar container
 * Automatically handles flex-wrapping, prevents action buttons from overflowing
 * or protruding off-screen on mobile and web viewports.
 */
export const HeaderToolbar: React.FC<HeaderToolbarProps> = ({
  children,
  leftContent,
  rightContent,
  className = '',
  style = {},
  borderBottom = true
}) => {
  return (
    <div
      style={style}
      className={`w-full flex flex-wrap items-center justify-between gap-2.5 pb-3 min-w-0 ${
        borderBottom ? 'border-b border-neutral-200 dark:border-neutral-800' : ''
      } ${className}`}
    >
      {leftContent || rightContent ? (
        <>
          <div className="flex items-center gap-1.5 min-w-0 max-w-full overflow-x-auto scrollbar-none py-0.5">
            {leftContent}
          </div>
          {rightContent && (
            <div className="flex items-center gap-1.5 shrink-0 ml-auto">
              {rightContent}
            </div>
          )}
        </>
      ) : (
        children
      )}
    </div>
  );
};
