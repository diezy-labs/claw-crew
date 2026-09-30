import React, { useState } from 'react';
import { Search, X } from 'lucide-react';

export interface PageHeaderProps {
  icon?: React.ReactNode;
  title: string;
  badge?: React.ReactNode;
  description?: string;
  actions?: React.ReactNode;
  search?: {
    value: string;
    onChange: (value: string) => void;
    placeholder?: string;
  };
  className?: string;
}

export const PageHeader: React.FC<PageHeaderProps> = ({
  icon,
  title,
  badge,
  description,
  actions,
  search,
  className = ''
}) => {
  const [isMobileSearchOpen, setIsMobileSearchOpen] = useState(false);

  return (
    <div
      className={`border-b border-neutral-200 dark:border-neutral-800 pb-3 sm:pb-4 shrink-0 transition-all ${className}`}
    >
      <div className="flex items-center justify-between gap-3">
        {/* Left: Icon, Title & Badge */}
        <div className="min-w-0 flex-1">
          {/* Mobile Search Overlay: expands on mobile when search icon is clicked */}
          {search && isMobileSearchOpen ? (
            <div className="flex sm:hidden items-center gap-2 w-full animate-in fade-in duration-150">
              <div className="relative flex-1">
                <Search className="w-3.5 h-3.5 absolute left-2.5 top-1/2 -translate-y-1/2 text-neutral-400" />
                <input
                  type="text"
                  placeholder={search.placeholder || 'Search...'}
                  value={search.value}
                  onChange={(e) => search.onChange(e.target.value)}
                  autoFocus
                  className="w-full pl-8 pr-7 py-1.5 text-xs rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 placeholder:text-neutral-400 focus:outline-none focus:ring-1 focus:ring-teal-500"
                />
                {search.value && (
                  <button
                    type="button"
                    onClick={() => search.onChange('')}
                    className="absolute right-2 top-1/2 -translate-y-1/2 text-neutral-400 hover:text-neutral-600 dark:hover:text-neutral-200"
                  >
                    <X className="w-3 h-3" />
                  </button>
                )}
              </div>
              <button
                type="button"
                onClick={() => setIsMobileSearchOpen(false)}
                className="px-2 py-1 text-xs text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-100 font-medium shrink-0 cursor-pointer"
              >
                Cancel
              </button>
            </div>
          ) : (
            <div className="flex items-center gap-2 sm:gap-2.5 min-w-0">
              {icon && (
                <div className="w-7 h-7 sm:w-8 sm:h-8 rounded-lg bg-teal-500/10 text-teal-600 dark:text-teal-400 flex items-center justify-center shrink-0 ring-1 ring-teal-500/20">
                  {icon}
                </div>
              )}
              <div className="min-w-0 flex items-center gap-2 flex-wrap sm:flex-nowrap">
                <h1 className="text-lg sm:text-2xl font-bold tracking-tight text-neutral-900 dark:text-neutral-100 leading-tight truncate">
                  {title}
                </h1>
                {badge && (
                  <div className="shrink-0">{badge}</div>
                )}
              </div>
            </div>
          )}

          {/* Description (Hidden on mobile for clean minimalist header, shown on sm+) */}
          {description && !isMobileSearchOpen && (
            <p className="hidden sm:block text-xs text-neutral-500 dark:text-neutral-400 mt-1 pl-10.5 line-clamp-1">
              {description}
            </p>
          )}
        </div>

        {/* Right: Actions & Search */}
        {(!search || !isMobileSearchOpen) && (
          <div className="flex items-center gap-2 shrink-0">
            {/* Desktop Search */}
            {search && (
              <>
                {/* Mobile Search Toggle Icon */}
                <button
                  type="button"
                  onClick={() => setIsMobileSearchOpen(true)}
                  className="sm:hidden p-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] text-neutral-600 dark:text-neutral-400 hover:text-neutral-900 dark:hover:text-neutral-100 cursor-pointer transition-colors"
                  title="Search"
                  aria-label="Open search"
                >
                  <Search className="w-3.5 h-3.5" />
                </button>

                {/* Desktop Search Input */}
                <div className="hidden sm:block relative">
                  <Search className="w-3.5 h-3.5 absolute left-2.5 top-1/2 -translate-y-1/2 text-neutral-400" />
                  <input
                    type="text"
                    placeholder={search.placeholder || 'Search...'}
                    value={search.value}
                    onChange={(e) => search.onChange(e.target.value)}
                    className="pl-8 pr-3 py-1.5 text-xs rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 placeholder:text-neutral-400 focus:outline-none focus:ring-1 focus:ring-teal-500 w-44 lg:w-56 transition-all"
                  />
                </div>
              </>
            )}

            {/* Custom Action buttons (e.g. + Quest, + Craft, Filters) */}
            {actions}
          </div>
        )}
      </div>
    </div>
  );
};
