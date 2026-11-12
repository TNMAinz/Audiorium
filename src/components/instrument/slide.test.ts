import { describe, expect, it } from 'vitest';
import { slideNote } from './slide';

describe('a pointer on the keyboard bed', () => {
  it('sounds the key it presses and nothing is released on the way in', () => {
    expect(slideNote(undefined, 48)).toEqual({ off: null, on: 48 });
  });

  it('moves the note to the key under the cursor, releasing the one it left', () => {
    expect(slideNote(48, 50)).toEqual({ off: 48, on: 50 });
    // A sharp between two naturals is still one release and one start.
    expect(slideNote(50, 51)).toEqual({ off: 50, on: 51 });
  });

  it('silences the note between keys without ending the drag', () => {
    // A null note is "on no key": the pointer is still down, so it keeps its place on
    // the bed and the next key it reaches plays.
    expect(slideNote(48, null)).toEqual({ off: 48, on: null });
    expect(slideNote(null, 50)).toEqual({ off: null, on: 50 });
  });

  it('does nothing when the pointer stays where it is', () => {
    expect(slideNote(48, 48)).toBeNull();
    expect(slideNote(null, null)).toBeNull();
    expect(slideNote(undefined, null)).toBeNull();
  });

  it('is the same answer whether a pointer is new or between keys', () => {
    // The regression this pins: leaving the bed used to drop the pointer, so coming
    // back was treated as a new press (or not tracked at all). Both are "on no key".
    expect(slideNote(undefined, 60)).toEqual(slideNote(null, 60));
  });
});
