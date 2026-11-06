/** The rail starts and ends inside the region, so the pill never touches a corner. */
export const RAIL_INSET = 4;
/** A short list still gets a pill the hand can aim at. */
export const MIN_THUMB = 16;

export interface RailMetrics {
  /** Height of the run the pill travels. */
  track: number;
  /** Height of the pill itself. */
  thumb: number;
  /** Distance from the top of the track. */
  top: number;
}

/** Where the pill sits for the current scroll offset; null when nothing overflows. */
export function railMetrics(view: number, content: number, scrollTop: number): RailMetrics | null {
  const track = Math.max(0, view - RAIL_INSET * 2);
  if (track <= 0 || content <= view + 1) return null;
  const thumb = Math.min(track, Math.max(MIN_THUMB, Math.round((track * view) / content)));
  const travel = content - view;
  const progress = travel > 0 ? Math.min(1, Math.max(0, scrollTop / travel)) : 0;
  return { track, thumb, top: Math.round((track - thumb) * progress) };
}

/**
 * Scroll offset for a pointer held at `offset` px below the top of the rail,
 * with `grab` px of the pill above the pointer.
 */
export function railScrollTop(view: number, content: number, offset: number, grab: number): number {
  const metrics = railMetrics(view, content, 0);
  if (!metrics) return 0;
  const travel = metrics.track - metrics.thumb;
  if (travel <= 0) return 0;
  const held = Math.min(travel, Math.max(0, offset - grab));
  return (held / travel) * (content - view);
}
