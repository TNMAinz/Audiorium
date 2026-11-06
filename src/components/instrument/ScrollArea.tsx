import { useCallback, useEffect, useRef, useState } from 'react';
import type { AriaRole, CSSProperties, PointerEvent as ReactPointerEvent, ReactNode, WheelEvent as ReactWheelEvent } from 'react';
import { railMetrics, railScrollTop, type RailMetrics } from './rail';

interface Props {
  children: ReactNode;
  /** Sits on the frame; the scrolling element is the row container inside it. */
  className?: string;
  role?: AriaRole;
  ariaLabel?: string;
  style?: CSSProperties;
}

/**
 * A panel list that scrolls under its own rail. The native gutter is hidden, so
 * the region keeps the panel's margins on both sides and a row rule is never cut
 * short by a scrollbar lane. The rail itself is a 3 px pill that sits in the
 * panel's margin, appears while the region is hovered, focused or moving, and
 * can be dragged.
 */
export function ScrollArea({ children, className = '', role, ariaLabel, style }: Props) {
  const view = useRef<HTMLDivElement>(null);
  const rail = useRef<HTMLDivElement>(null);
  const grab = useRef(0);
  const hide = useRef<number | undefined>(undefined);
  const [metrics, setMetrics] = useState<RailMetrics | null>(null);
  const [dragging, setDragging] = useState(false);
  const [moving, setMoving] = useState(false);

  const sync = useCallback(() => {
    const element = view.current;
    if (!element) return;
    setMetrics(railMetrics(element.clientHeight, element.scrollHeight, element.scrollTop));
  }, []);

  useEffect(() => {
    const element = view.current;
    if (!element) return;
    sync();
    const box = new ResizeObserver(sync);
    box.observe(element);
    // Rows arriving or leaving change the content height without resizing the box.
    const rows = new MutationObserver(sync);
    rows.observe(element, { childList: true });
    return () => { box.disconnect(); rows.disconnect(); };
  }, [sync]);

  useEffect(() => () => window.clearTimeout(hide.current), []);

  const scrolled = () => {
    sync();
    setMoving(true);
    window.clearTimeout(hide.current);
    if (!dragging) hide.current = window.setTimeout(() => setMoving(false), 900);
  };

  const scrollToPointer = (clientY: number) => {
    const element = view.current;
    const track = rail.current;
    if (!element || !track) return;
    element.scrollTop = railScrollTop(
      element.clientHeight, element.scrollHeight, clientY - track.getBoundingClientRect().top, grab.current,
    );
  };

  const startDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    const element = view.current;
    const current = element ? railMetrics(element.clientHeight, element.scrollHeight, element.scrollTop) : null;
    if (!element || !current) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    const held = event.clientY - event.currentTarget.getBoundingClientRect().top;
    const inside = held >= current.top && held <= current.top + current.thumb;
    grab.current = inside ? held - current.top : Math.round(current.thumb / 2);
    setDragging(true);
    scrollToPointer(event.clientY);
  };

  const endDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!dragging) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
    setDragging(false);
    setMoving(false);
  };

  // The rail sits a few px outside the list, so a wheel over it must still scroll the list.
  const forwardWheel = (event: ReactWheelEvent<HTMLDivElement>) => {
    if (view.current) view.current.scrollTop += event.deltaY;
  };

  return <div className={`scroll-area ${className}`} style={style}>
    <div
      className="scroll-view"
      ref={view}
      role={role}
      aria-label={ariaLabel}
      /* Only a list that overflows takes a tab stop, so arrow keys can move it. */
      tabIndex={metrics ? 0 : undefined}
      onScroll={scrolled}
    >
      {children}
    </div>
    {metrics && <div
      className={`scroll-rail ${dragging ? 'active' : ''} ${moving ? 'moving' : ''}`}
      ref={rail}
      aria-hidden="true"
      onPointerDown={startDrag}
      onPointerMove={event => { if (dragging) scrollToPointer(event.clientY); }}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
      onWheel={forwardWheel}
    >
      <i style={{ height: metrics.thumb, transform: `translateY(${metrics.top}px)` }} />
    </div>}
  </div>;
}
