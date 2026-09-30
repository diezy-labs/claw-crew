import React, { useRef, useState, useEffect, useCallback } from 'react';
import { ChevronLeft, ChevronRight } from 'lucide-react';

interface SubMenuScrollerProps {
  children: React.ReactNode;
  className?: string;
  containerClassName?: string;
  scrollStep?: number;
}

export const SubMenuScroller: React.FC<SubMenuScrollerProps> = ({
  children,
  className = '',
  containerClassName = '',
  scrollStep = 180
}) => {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const [canScrollLeft, setCanScrollLeft] = useState(false);
  const [canScrollRight, setCanScrollRight] = useState(false);

  const checkScroll = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    const { scrollLeft, scrollWidth, clientWidth } = el;
    setCanScrollLeft(scrollLeft > 4);
    setCanScrollRight(scrollLeft + clientWidth < scrollWidth - 4);
  }, []);

  useEffect(() => {
    checkScroll();
    const el = scrollRef.current;
    if (!el) return;

    el.addEventListener('scroll', checkScroll, { passive: true });
    window.addEventListener('resize', checkScroll);

    // Mutation observer to detect child additions or tab changes
    const observer = new MutationObserver(checkScroll);
    observer.observe(el, { childList: true, subtree: true });

    return () => {
      el.removeEventListener('scroll', checkScroll);
      window.removeEventListener('resize', checkScroll);
      observer.disconnect();
    };
  }, [checkScroll, children]);

  const handleScrollLeft = () => {
    if (!scrollRef.current) return;
    scrollRef.current.scrollBy({ left: -scrollStep, behavior: 'smooth' });
  };

  const handleScrollRight = () => {
    if (!scrollRef.current) return;
    scrollRef.current.scrollBy({ left: scrollStep, behavior: 'smooth' });
  };

  return (
    <div className={`relative flex items-center min-w-0 ${containerClassName}`}>
      {/* Left Scroll Arrow Button (<) - Automatically hidden when at leftmost position */}
      {canScrollLeft && (
        <button
          type="button"
          onClick={handleScrollLeft}
          aria-label="Scroll sub-menu left"
          className="absolute left-0 z-20 h-7 w-7 rounded-full bg-white/95 dark:bg-[#181a1e]/95 border border-neutral-200 dark:border-neutral-700 shadow-md flex items-center justify-center text-neutral-700 dark:text-neutral-200 hover:text-teal-600 dark:hover:text-teal-400 hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-all cursor-pointer shrink-0 -translate-x-1"
        >
          <ChevronLeft className="w-4 h-4" />
        </button>
      )}

      {/* Scrollable Sub-Menu Content with hidden scrollbar */}
      <div
        ref={scrollRef}
        className={`flex-1 overflow-x-auto scrollbar-none flex items-center gap-1.5 py-0.5 ${className}`}
        style={{ scrollbarWidth: 'none', msOverflowStyle: 'none' }}
      >
        {children}
      </div>

      {/* Right Scroll Arrow Button (>) - Automatically hidden when at rightmost position */}
      {canScrollRight && (
        <button
          type="button"
          onClick={handleScrollRight}
          aria-label="Scroll sub-menu right"
          className="absolute right-0 z-20 h-7 w-7 rounded-full bg-white/95 dark:bg-[#181a1e]/95 border border-neutral-200 dark:border-neutral-700 shadow-md flex items-center justify-center text-neutral-700 dark:text-neutral-200 hover:text-teal-600 dark:hover:text-teal-400 hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-all cursor-pointer shrink-0 translate-x-1"
        >
          <ChevronRight className="w-4 h-4" />
        </button>
      )}
    </div>
  );
};
