export const COVER_WINDOW = 9;
export const ART_CACHE_LIMIT = 24;
export const DRAG_PIXELS_PER_TRACK = 58;
export const WHEEL_PIXELS_PER_TRACK = 100;

/** Map any number of turns to a library index, preserving fractional drag progress. */
export function wrapCursor(value: number, count: number): number {
  if (count <= 0 || !Number.isFinite(value)) return 0;
  return ((value % count) + count) % count;
}

/** Signed distance around the library, choosing a consistent direction at the midpoint. */
export function circularDistance(index: number, cursor: number, count: number): number {
  return count > 0 ? wrapCursor(index - cursor + count / 2, count) - count / 2 : 0;
}

/** A bounded window around the cursor, wrapping without duplicating small libraries. */
export function coverWindow(count: number, cursor: number): number[] {
  const length = Math.max(0, Math.min(COVER_WINDOW, Math.floor(count)));
  const center = Math.round(wrapCursor(cursor, count)) % (count || 1);
  const start = center - Math.floor(length / 2);
  return Array.from({ length }, (_, at) => wrapCursor(start + at, count));
}

/** Keep the cursor unbounded during gestures so crossing the seam stays continuous. */
export function dragCursor(start: number, distance: number, count: number): number {
  return count > 1 && Number.isFinite(start) && Number.isFinite(distance) ? start - distance / DRAG_PIXELS_PER_TRACK : 0;
}

/**
 * Converts wheel travel into whole song steps plus the pixels carried into the next
 * event, so slow trackpad scrolling browses too and fast scrolling stays in step.
 */
export function wheelSteps(carry: number, delta: number, perTrack = WHEEL_PIXELS_PER_TRACK): { steps: number; carry: number } {
  const total = (Number.isFinite(carry) ? carry : 0) + (Number.isFinite(delta) ? delta : 0);
  const steps = Math.trunc(total / perTrack);
  return { steps, carry: total - steps * perTrack };
}

/**
 * Whether the shelf should invite the user to bring a library.
 *
 * It is for a library that is not there, not for a lack of one: someone listening
 * to the input with no folder scanned is already using the app, so the card would
 * be telling them about the wrong thing. It comes back once that input falls quiet.
 */
export function showsLibraryInvite(count: number, busy: boolean, error: string, inputHeard: boolean): boolean {
  // A scan in flight and a folder that cannot be read are states to report, not
  // invitations, so an input being heard does not hide them.
  if (busy || error) return true;
  return count <= 0 && !inputHeard;
}

export const COVER_COLUMN = 0.135;
export const COVER_WIDTH = 0.09;
const REACH = 4.2;
/** How far the shelf falls away towards its ends, in screen heights. */
const ARC = 0.02;
/** The farthest sleeve the mounted window reaches, either side of the song in hand. */
const SHELF_EDGE = (COVER_WINDOW - 1) / 2;

export interface CoverLayout { x: number; y: number; scale: number; opacity: number }

const EASE_STEP = 0.24;
const EASE_CAP = 0.9;
/** Momentum: the share of speed kept each 60 fps frame, and the speeds it runs between. */
export const FLING_FRICTION = 0.962;
export const FLING_MIN = 0.5;
export const FLING_MAX = 10;

/** Pointer speed over the samples of one gesture, as songs per second (leftwards advances). */
export function dragVelocity(samples: { x: number; t: number }[]): number {
  const usable = samples.filter(sample => Number.isFinite(sample.x) && Number.isFinite(sample.t));
  if (usable.length < 2) return 0;
  const first = usable[0], last = usable[usable.length - 1];
  const elapsed = last.t - first.t;
  if (!(elapsed > 8)) return 0;
  return -(last.x - first.x) / DRAG_PIXELS_PER_TRACK / (elapsed / 1000);
}

/** A flick is capped: a hard throw crosses a few sleeves, never half the library. */
export function clampFling(velocity: number): number {
  if (!Number.isFinite(velocity)) return 0;
  return Math.max(-FLING_MAX, Math.min(FLING_MAX, velocity));
}

/** One frame of momentum: the cursor advances at the current speed, which then decays. */
export function flingStep(cursor: number, velocity: number, seconds: number): { cursor: number; velocity: number } {
  const dt = Math.max(0, Math.min(0.05, Number.isFinite(seconds) ? seconds : 0));
  const decayed = velocity * Math.pow(FLING_FRICTION, dt * 60);
  return { cursor: cursor + velocity * dt, velocity: Math.abs(decayed) < FLING_MIN ? 0 : decayed };
}

/**
 * The cursor as the shelf shows it. A drag is locked to the pointer and passes straight
 * through; a wheel notch, an arrow key or a browse button glides into place instead. The
 * step is capped below one sleeve so even a long jump is a sweep, never a teleport.
 */
export function easeCursor(current: number, target: number, immediate: boolean): number {
  if (!Number.isFinite(target)) return current;
  const gap = target - current;
  if (immediate || Math.abs(gap) < 0.001) return target;
  return current + Math.max(-EASE_CAP, Math.min(EASE_CAP, gap * EASE_STEP));
}

/**
 * Screen-normalised placement on one shelf: songs slide sideways with the library,
 * shrinking and dropping a little towards the ends so browsing has depth. The album in
 * hand sits foremost on the shelf line; the arc is symmetric either side of it.
 */
export function coverLayout(index: number, cursor: number, count: number): CoverLayout {
  const distance = circularDistance(index, cursor, count);
  const reach = Math.min(Math.abs(distance), REACH) / REACH;
  // Sleeves fade out to nothing at the end of the mounted window, so a sleeve entering
  // or leaving never appears at partial strength: the shelf shifts without a pop.
  const leaving = Math.max(0, Math.abs(distance) - (SHELF_EDGE - 1));
  return {
    x: distance * COVER_COLUMN,
    y: reach * reach * ARC,
    scale: 1 - reach * 0.18,
    opacity: 1 - Math.pow(Math.min(1, leaving), 1.35),
  };
}

export interface GlobeMotion { cursor: number; tilt: number; dragging: boolean; gliding: boolean }
export interface CoverNode { card: HTMLButtonElement }

/**
 * Whether the shelf is in motion: a hand on it, a throw still coasting, or a step still
 * gliding into place. The scene asks for every frame the display has while this is true,
 * so the idle frame rate is never what browsing looks like.
 */
export function shelfMoving(motion: GlobeMotion, eased: number): boolean {
  return motion.dragging || motion.gliding || Math.abs(motion.cursor - eased) >= 0.001;
}
