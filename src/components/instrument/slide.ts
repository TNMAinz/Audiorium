/**
 * What one pointer on the keyboard bed means, given the note it was holding and the
 * note now under it.
 *
 * A pointer that pressed on the bed keeps that place until it is let go: travelling
 * onto no key silences the note it had, but the pointer is still on the bed, so a drag
 * that wanders off the keys and comes back plays again without a second press. A null
 * `on` is "on no key", never "let go" — letting go is the caller's business, which is
 * why `held` is `undefined` for a pointer that is not on the bed at all.
 */
export interface Slide {
  /** The note to release, or null when nothing was sounding. */
  off: number | null;
  /** The note to start, or null when the pointer is between keys. */
  on: number | null;
}

export function slideNote(held: number | null | undefined, under: number | null): Slide | null {
  const current = held ?? null;
  if (current === under) return null;
  return { off: current, on: under };
}
