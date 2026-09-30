import React from 'react';

export interface PageStickyNavProps {
  children: React.ReactNode;
  rightContent?: React.ReactNode;
  className?: string;
  containerClassName?: string;
}

export const PageStickyNav: React.FC<PageStickyNavProps> = ({
  children,
  rightContent,
  className = '',
  containerClassName = ''
}) => {
  return (
    <div
      className={`sticky -top-4 sm:top-0 z-20 -mx-4 sm:-mx-6 px-4 sm:px-6 py-2 sm:py-2.5 bg-[var(--bg-canvas)]/95 backdrop-blur-md border-b border-neutral-200/80 dark:border-neutral-800/80 shadow-2xs shrink-0 transition-all ${containerClassName}`}
    >
      <div className={`flex items-center justify-between gap-2.5 min-w-0 ${className}`}>
        <div className="flex-1 min-w-0">
          {children}
        </div>
        {rightContent && (
          <div className="shrink-0 flex items-center gap-1.5">
            {rightContent}
          </div>
        )}
      </div>
    </div>
  );
};
