import React, { useState, useRef, useEffect, useLayoutEffect, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { ChevronDown, Check, X } from 'lucide-react';

export interface DropdownItem {
  id: string;
  label: React.ReactNode;
  icon?: React.ReactNode;
  badge?: React.ReactNode;
  description?: string;
  disabled?: boolean;
  danger?: boolean;
  divider?: boolean;
  onClick?: () => void;
}

export interface DropdownGroup {
  group: string;
  items: DropdownItem[];
}

export interface DropdownProps {
  trigger?: React.ReactNode;
  items?: DropdownItem[];
  groups?: DropdownGroup[];
  selectedId?: string;
  onSelect?: (id: string) => void;
  label?: string;
  title?: string; // Modal / Sheet title on mobile
  icon?: React.ReactNode;
  align?: 'left' | 'right';
  placement?: 'auto' | 'bottom' | 'top';
  size?: 'xs' | 'sm' | 'md';
  className?: string;
  menuWidth?: string; // e.g. 'w-56', 'w-72', 'w-80'
  disabled?: boolean;
}

export const Dropdown: React.FC<DropdownProps> = ({
  trigger,
  items,
  groups,
  selectedId,
  onSelect,
  label = 'Select',
  title,
  icon,
  align = 'left',
  placement = 'auto',
  size = 'sm',
  className = '',
  menuWidth = 'w-64',
  disabled = false
}) => {
  const [isOpen, setIsOpen] = useState(false);
  const [isMobile, setIsMobile] = useState(false);
  const [coords, setCoords] = useState<{ top?: number; bottom?: number; left?: number; right?: number; maxHeight?: number }>({});
  
  const triggerRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  // Check if mobile screen (< 640px)
  useEffect(() => {
    const checkMobile = () => {
      setIsMobile(window.innerWidth < 640);
    };
    checkMobile();
    window.addEventListener('resize', checkMobile);
    return () => window.removeEventListener('resize', checkMobile);
  }, []);

  // Calculate position in portal for desktop mode
  const updateCoords = useCallback(() => {
    if (!triggerRef.current || isMobile) return;
    const rect = triggerRef.current.getBoundingClientRect();
    const windowHeight = window.innerHeight;
    const windowWidth = window.innerWidth;

    const estimatedMenuHeight = 280;
    const spaceBelow = windowHeight - rect.bottom;
    const spaceAbove = rect.top;

    const openUpwards =
      placement === 'top' || (placement === 'auto' && spaceBelow < estimatedMenuHeight && spaceAbove > spaceBelow);

    const calculatedCoords: { top?: number; bottom?: number; left?: number; right?: number; maxHeight?: number } = {};

    if (openUpwards) {
      calculatedCoords.bottom = windowHeight - rect.top + 6;
      calculatedCoords.maxHeight = Math.min(spaceAbove - 16, 400);
    } else {
      calculatedCoords.top = rect.bottom + 6;
      calculatedCoords.maxHeight = Math.min(spaceBelow - 16, 400);
    }

    if (align === 'right') {
      calculatedCoords.right = Math.max(12, windowWidth - rect.right);
    } else {
      calculatedCoords.left = Math.max(12, Math.min(rect.left, windowWidth - 320));
    }

    setCoords(calculatedCoords);
  }, [align, placement, isMobile]);

  useLayoutEffect(() => {
    if (isOpen) {
      updateCoords();
    }
  }, [isOpen, updateCoords]);

  // Handle outside click & scroll/resize repositioning
  useEffect(() => {
    if (!isOpen) return;

    const handleOutsideClick = (e: MouseEvent | TouchEvent) => {
      const target = e.target as Node;
      if (
        triggerRef.current?.contains(target) ||
        menuRef.current?.contains(target)
      ) {
        return;
      }
      setIsOpen(false);
    };

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setIsOpen(false);
      }
    };

    const handleScrollOrResize = () => {
      if (!isMobile) {
        updateCoords();
      }
    };

    document.addEventListener('mousedown', handleOutsideClick);
    document.addEventListener('touchstart', handleOutsideClick, { passive: true });
    window.addEventListener('keydown', handleKeyDown);
    window.addEventListener('scroll', handleScrollOrResize, true);
    window.addEventListener('resize', handleScrollOrResize);

    return () => {
      document.removeEventListener('mousedown', handleOutsideClick);
      document.removeEventListener('touchstart', handleOutsideClick);
      window.removeEventListener('keydown', handleKeyDown);
      window.removeEventListener('scroll', handleScrollOrResize, true);
      window.removeEventListener('resize', handleScrollOrResize);
    };
  }, [isOpen, isMobile, updateCoords]);

  const sizeClasses = {
    xs: 'px-2 py-1 text-xs gap-1',
    sm: 'px-2.5 py-1.5 text-xs gap-1.5',
    md: 'px-3 py-2 text-sm gap-2'
  }[size];

  const handleItemClick = (item: DropdownItem) => {
    if (item.disabled) return;
    if (item.onClick) item.onClick();
    if (onSelect) onSelect(item.id);
    setIsOpen(false);
  };

  // Find selected item label
  const allItems: DropdownItem[] = [
    ...(items || []),
    ...(groups ? groups.flatMap((g) => g.items) : [])
  ];
  const selectedItem = allItems.find((i) => i.id === selectedId);

  // Render individual item
  const renderItem = (item: DropdownItem, index: number) => {
    if (item.divider) {
      return (
        <div
          key={`div-${index}`}
          className="h-px bg-neutral-100 dark:bg-neutral-800/80 my-1"
        />
      );
    }

    const isSelected = item.id === selectedId;

    return (
      <button
        key={item.id}
        type="button"
        disabled={item.disabled}
        onClick={() => handleItemClick(item)}
        className={`w-full flex items-start justify-between px-3 py-2 rounded-xl text-xs font-sans transition-all cursor-pointer text-left ${
          item.disabled
            ? 'opacity-40 cursor-not-allowed'
            : item.danger
            ? 'text-rose-600 dark:text-rose-400 hover:bg-rose-50 dark:hover:bg-rose-950/20 active:scale-[0.99]'
            : isSelected
            ? 'bg-teal-500/10 dark:bg-teal-500/15 text-teal-800 dark:text-teal-300 font-semibold'
            : 'text-neutral-700 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800/80 active:scale-[0.99]'
        }`}
      >
        <div className="flex items-start gap-2.5 min-w-0">
          {item.icon && (
            <span className="shrink-0 text-teal-500 mt-0.5">{item.icon}</span>
          )}
          <div className="min-w-0">
            <div className="truncate font-semibold text-neutral-900 dark:text-neutral-100">
              {item.label}
            </div>
            {item.description && (
              <div className="text-[11px] text-neutral-400 dark:text-neutral-500 leading-tight mt-0.5 line-clamp-2">
                {item.description}
              </div>
            )}
          </div>
        </div>

        <div className="flex items-center gap-1.5 shrink-0 ml-2 mt-0.5">
          {item.badge && <span>{item.badge}</span>}
          {isSelected && (
            <Check className="w-4 h-4 text-teal-500 shrink-0" />
          )}
        </div>
      </button>
    );
  };

  // Render items body (grouped or flat)
  const renderListContent = () => {
    if (groups && groups.length > 0) {
      return (
        <div className="space-y-3">
          {groups.map((grp) => (
            <div key={grp.group} className="space-y-1">
              <div className="text-[10px] font-mono font-semibold text-neutral-400 uppercase tracking-wider px-2 pt-1">
                {grp.group}
              </div>
              <div className="space-y-0.5">
                {grp.items.map((item, idx) => renderItem(item, idx))}
              </div>
            </div>
          ))}
        </div>
      );
    }

    return (
      <div className="space-y-0.5">
        {(items || []).map((item, idx) => renderItem(item, idx))}
      </div>
    );
  };

  // Portal Content
  const portalContent = isOpen && typeof document !== 'undefined' ? (
    createPortal(
      isMobile ? (
        /* Mobile: Native-like Bottom Sheet Overlay */
        <div
          role="dialog"
          aria-modal="true"
          className="fixed inset-0 z-[9999] bg-black/65 backdrop-blur-xs flex flex-col justify-end animate-in fade-in duration-150"
          onClick={() => setIsOpen(false)}
        >
          <div
            ref={menuRef}
            onClick={(e) => e.stopPropagation()}
            className="w-full max-h-[82vh] bg-white dark:bg-[#181a1e] rounded-t-3xl border-t border-neutral-200 dark:border-neutral-800 shadow-2xl flex flex-col overflow-hidden animate-in slide-in-from-bottom duration-200 cursor-default"
          >
            {/* Grab handle & Header */}
            <div className="px-5 pt-3 pb-3 border-b border-neutral-100 dark:border-neutral-800/80 flex items-center justify-between shrink-0">
              <div className="flex items-center gap-2">
                {icon && <span className="text-teal-500 shrink-0">{icon}</span>}
                <span className="font-bold text-sm text-neutral-900 dark:text-neutral-100">
                  {title || label || 'Select Option'}
                </span>
              </div>
              <button
                type="button"
                onClick={() => setIsOpen(false)}
                className="p-1.5 rounded-full text-neutral-400 hover:text-neutral-200 hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors"
                aria-label="Close"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            {/* Scrollable Options List */}
            <div className="flex-1 overflow-y-auto p-3 space-y-1 text-xs">
              {renderListContent()}
            </div>
          </div>
        </div>
      ) : (
        /* Desktop: Floating Popover with Exact Positioning */
        <div
          ref={menuRef}
          role="menu"
          style={{
            position: 'fixed',
            top: coords.top !== undefined ? `${coords.top}px` : undefined,
            bottom: coords.bottom !== undefined ? `${coords.bottom}px` : undefined,
            left: coords.left !== undefined ? `${coords.left}px` : undefined,
            right: coords.right !== undefined ? `${coords.right}px` : undefined,
            maxHeight: coords.maxHeight !== undefined ? `${coords.maxHeight}px` : '380px'
          }}
          className={`z-[9999] ${menuWidth} rounded-2xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#181a1e] p-2 shadow-2xl ring-1 ring-black/10 dark:ring-white/10 overflow-y-auto scrollbar-thin animate-in fade-in zoom-in-95 duration-100`}
        >
          {renderListContent()}
        </div>
      ),
      document.body
    )
  ) : null;

  return (
    <div ref={triggerRef} className={`relative inline-block ${className}`}>
      {trigger ? (
        <div
          onClick={() => !disabled && setIsOpen(!isOpen)}
          className="cursor-pointer"
        >
          {trigger}
        </div>
      ) : (
        <button
          type="button"
          disabled={disabled}
          onClick={() => setIsOpen(!isOpen)}
          className={`inline-flex items-center justify-between rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 font-medium text-neutral-800 dark:text-neutral-200 hover:border-teal-500/50 hover:bg-neutral-50 dark:hover:bg-neutral-800/60 focus:outline-none transition-colors shadow-2xs disabled:opacity-50 disabled:cursor-not-allowed cursor-pointer ${sizeClasses}`}
        >
          <div className="flex items-center gap-1.5 min-w-0">
            {icon || selectedItem?.icon}
            <span className="truncate">{selectedItem ? selectedItem.label : label}</span>
          </div>
          <ChevronDown
            className={`w-3.5 h-3.5 text-neutral-400 transition-transform duration-150 ${
              isOpen ? 'rotate-180' : 'rotate-0'
            }`}
          />
        </button>
      )}

      {portalContent}
    </div>
  );
};
