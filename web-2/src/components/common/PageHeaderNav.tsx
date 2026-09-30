import React, { useState } from 'react';
import { Search, X } from 'lucide-react';
import { FilterChips, FilterChipItem } from './FilterChips';

export interface PageHeaderNavProps<T extends string = string> {
  // Header Props
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

  // Integrated Chips / Tabs Props
  chips?: {
    items: FilterChipItem<T>[];
    selectedId: T;
    onSelect: (id: T) => void;
    variant?: 'pills' | 'tabs' | 'subtle';
    size?: 'xs' | 'sm' | 'md';
  };

  // Or custom nav content for flexible sub-menus
  navContent?: React.ReactNode;
  navRightContent?: React.ReactNode;

  className?: string;
  containerClassName?: string;
  sticky?: boolean;
}

export const PageHeaderNav = <T extends string = string>({
  icon,
  title,
  badge,
  description,
  actions,
  search,
  chips,
  navContent,
  navRightContent,
  className = '',
  containerClassName = '',
  sticky = true
}: PageHeaderNavProps<T>): React.ReactElement => {
  const [isMobileSearchOpen, setIsMobileSearchOpen] = useState(false);

  const stickyClasses = sticky
    ? 'sticky top-0 z-20 bg-[var(--bg-canvas)]/95 backdrop-blur-md border-b border-neutral-200/80 dark:border-neutral-800/80 shadow-2xs'
    : 'border-b border-neutral-200/80 dark:border-neutral-800/80';

  return (
    <div
      className={`-mx-4 sm:-mx-6 px-4 sm:px-6 transition-all shrink-0 ${stickyClasses} ${containerClassName}`}
    >
      {/* Top Header Row: Icon, Title, Badge, Description, Search, Actions */}
      <div className={`flex items-center justify-between gap-3 pt-2.5 sm:pt-3 pb-2 min-w-0 ${className}`}>
        {/* Left: Icon, Title, Badge & optional inline description */}
        <div className="min-w-0 flex-1">
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
              <div className="min-w-0 flex items-baseline gap-2 flex-wrap sm:flex-nowrap">
                <h1 className="text-base sm:text-xl font-bold tracking-tight text-neutral-900 dark:text-neutral-100 leading-tight truncate">
                  {title}
                </h1>
                {badge && <div className="shrink-0">{badge}</div>}
                {description && (
                  <span className="hidden xl:inline-block text-xs text-neutral-400 dark:text-neutral-500 truncate max-w-sm ml-1 font-normal">
                    · {description}
                  </span>
                )}
              </div>
            </div>
          )}
        </div>

        {/* Right: Search & Actions */}
        {(!search || !isMobileSearchOpen) && (
          <div className="flex items-center gap-2 shrink-0">
            {/* Desktop Search */}
            {search && (
              <>
                <button
                  type="button"
                  onClick={() => setIsMobileSearchOpen(true)}
                  className="sm:hidden p-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] text-neutral-600 dark:text-neutral-400 hover:text-neutral-900 dark:hover:text-neutral-100 cursor-pointer transition-colors"
                  title="Search"
                  aria-label="Open search"
                >
                  <Search className="w-3.5 h-3.5" />
                </button>

                <div className="hidden sm:block relative">
                  <Search className="w-3.5 h-3.5 absolute left-2.5 top-1/2 -translate-y-1/2 text-neutral-400" />
                  <input
                    type="text"
                    placeholder={search.placeholder || 'Search...'}
                    value={search.value}
                    onChange={(e) => search.onChange(e.target.value)}
                    className="pl-8 pr-3 py-1 text-xs rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 placeholder:text-neutral-400 focus:outline-none focus:ring-1 focus:ring-teal-500 w-36 md:w-48 lg:w-56 transition-all"
                  />
                </div>
              </>
            )}

            {/* Action buttons */}
            {actions}
          </div>
        )}
      </div>

      {/* Chips / Sub-Tabs Row: directly docked with minimal vertical spacing */}
      {(chips || navContent || navRightContent) && (
        <div className="flex items-center justify-between gap-2.5 pb-2 pt-0.5 border-t border-neutral-100/60 dark:border-neutral-800/40 min-w-0">
          <div className="flex-1 min-w-0">
            {chips ? (
              <FilterChips
                items={chips.items}
                selectedId={chips.selectedId}
                onSelect={chips.onSelect}
                variant={chips.variant || 'pills'}
                size={chips.size || 'sm'}
              />
            ) : (
              navContent
            )}
          </div>
          {navRightContent && (
            <div className="shrink-0 flex items-center gap-1.5">
              {navRightContent}
            </div>
          )}
        </div>
      )}
    </div>
  );
};
