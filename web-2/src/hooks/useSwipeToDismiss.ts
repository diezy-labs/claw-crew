import { useState, useRef, useCallback } from 'react';

export interface UseSwipeToDismissOptions {
  onDismiss: () => void;
  enabled?: boolean;
  edgeThreshold?: number; // max distance from left edge to start swipe, default 100
  dismissThreshold?: number; // min distance swiped to trigger dismiss, default 75
}

export function useSwipeToDismiss({
  onDismiss,
  enabled = true,
  edgeThreshold = 120,
  dismissThreshold = 75
}: UseSwipeToDismissOptions) {
  const [dragOffset, setDragOffset] = useState<number>(0);
  const [isDragging, setIsDragging] = useState<boolean>(false);
  const startPos = useRef<{ x: number; y: number; time: number } | null>(null);
  const isEligibleSwipe = useRef<boolean>(false);

  const handleTouchStart = useCallback(
    (e: React.TouchEvent) => {
      if (!enabled) return;
      const touch = e.touches[0];
      if (!touch) return;

      const x = touch.clientX;
      const y = touch.clientY;

      // Allow swipe if initiated from left side of screen or left edge of the popover
      const isNearLeftEdge = x <= edgeThreshold || x <= (typeof window !== 'undefined' ? window.innerWidth * 0.4 : 150);
      isEligibleSwipe.current = isNearLeftEdge;
      startPos.current = { x, y, time: Date.now() };
      setIsDragging(false);
      setDragOffset(0);
    },
    [enabled, edgeThreshold]
  );

  const handleTouchMove = useCallback(
    (e: React.TouchEvent) => {
      if (!enabled || !startPos.current || !isEligibleSwipe.current) return;
      const touch = e.touches[0];
      if (!touch) return;

      const deltaX = touch.clientX - startPos.current.x;
      const deltaY = touch.clientY - startPos.current.y;

      // Only respond when moving rightwards (deltaX > 0) and horizontal travel dominates vertical
      if (deltaX > 8 && deltaX > Math.abs(deltaY) * 1.1) {
        setIsDragging(true);
        // Add subtle natural rubber-band resistance
        const offset = Math.max(0, deltaX);
        setDragOffset(offset);
      } else if (Math.abs(deltaY) > 25 && !isDragging) {
        // User is scrolling content vertically, cancel swipe gesture
        isEligibleSwipe.current = false;
        setDragOffset(0);
        setIsDragging(false);
      }
    },
    [enabled, isDragging]
  );

  const handleTouchEnd = useCallback(() => {
    if (!enabled || !startPos.current) return;
    const elapsed = Date.now() - startPos.current.time;
    const velocity = dragOffset / Math.max(elapsed, 1);

    // Trigger dismiss if swiped past threshold or flicked with sufficient velocity to the right
    if (dragOffset >= dismissThreshold || (dragOffset > 40 && velocity > 0.4)) {
      onDismiss();
    }

    setDragOffset(0);
    setIsDragging(false);
    startPos.current = null;
    isEligibleSwipe.current = false;
  }, [enabled, dragOffset, dismissThreshold, onDismiss]);

  const style: React.CSSProperties = isDragging
    ? {
        transform: `translateX(${dragOffset}px)`,
        transition: 'none',
        touchAction: 'pan-y'
      }
    : dragOffset === 0
    ? {
        transition: 'transform 0.22s cubic-bezier(0.16, 1, 0.3, 1)'
      }
    : {};

  return {
    dragOffset,
    isDragging,
    touchHandlers: {
      onTouchStart: handleTouchStart,
      onTouchMove: handleTouchMove,
      onTouchEnd: handleTouchEnd
    },
    style
  };
}
