import { describe, expect, it } from 'vitest';
import { ART_CACHE_LIMIT, COVER_COLUMN, COVER_WINDOW, FLING_MAX, FLING_MIN, WHEEL_PIXELS_PER_TRACK, circularDistance, clampFling, coverLayout, coverWindow, dragCursor, dragVelocity, easeCursor, flingStep, shelfMoving, showsLibraryInvite, wheelSteps, wrapCursor } from './globeLibrary';

describe('the album globe library window', () => {
  it('wraps at both ends without duplicating small libraries', () => {
    expect(coverWindow(0, 0)).toEqual([]);
    expect(coverWindow(1, 0)).toEqual([0]);
    expect(new Set(coverWindow(3, 0))).toEqual(new Set([0, 1, 2]));
    expect(coverWindow(20, 0)).toEqual([16, 17, 18, 19, 0, 1, 2, 3, 4]);
    expect(coverWindow(20, 20)).toEqual(coverWindow(20, 0));
    expect(ART_CACHE_LIMIT).toBeGreaterThan(COVER_WINDOW);
  });
  it('can reach every song without growing with library size', () => {
    for (const count of [1, 8, 16, 73, 5000]) {
      const seen = new Set<number>();
      for (let cursor = 0; cursor < count; cursor++) {
        const window = coverWindow(count, cursor);
        expect(new Set(window).size).toBe(window.length);
        expect(window.length).toBeLessThanOrEqual(COVER_WINDOW);
        expect(window).toContain(cursor);
        for (const index of window) { expect(index).toBeGreaterThanOrEqual(0); expect(index).toBeLessThan(count); seen.add(index); }
      }
      expect(seen.size).toBe(count);
    }
  });
  it('keeps moving through either seam over multiple turns', () => {
    expect(wrapCursor(dragCursor(0, 580, 100), 100)).toBe(90);
    expect(wrapCursor(dragCursor(99, -580, 100), 100)).toBe(9);
    expect(wrapCursor(301, 100)).toBe(1);
    expect(wrapCursor(-201, 100)).toBe(99);
    expect(wrapCursor(Number.NaN, 20)).toBe(0);
    expect(dragCursor(12, -580, 1)).toBe(0);
    expect(dragCursor(12, -580, 0)).toBe(0);
    expect(circularDistance(0, 19.9, 20)).toBeCloseTo(0.1);
    expect(circularDistance(1, 20, 20)).toBe(1);
  });
  it('turns wheel travel into looping steps without losing the remainder', () => {
    expect(wheelSteps(0, WHEEL_PIXELS_PER_TRACK)).toEqual({ steps: 1, carry: 0 });
    expect(wheelSteps(0, 40)).toEqual({ steps: 0, carry: 40 });
    expect(wheelSteps(40, 70)).toEqual({ steps: 1, carry: 10 });
    expect(wheelSteps(0, -250)).toEqual({ steps: -2, carry: -50 });
    expect(wheelSteps(Number.NaN, 30)).toEqual({ steps: 0, carry: 30 });
    // A whole turn of wheel travel returns to the same song, in either direction.
    expect(wrapCursor(0 + wheelSteps(0, 300).steps, 3)).toBe(0);
    expect(wrapCursor(0 + wheelSteps(0, -100).steps, 3)).toBe(2);
  });
  it('never places neighbouring covers closer than their own width', () => {
    for (const count of [7, 9, 11, 45, 5000]) {
      for (let cursor = 0; cursor < Math.min(count, 60); cursor++) {
        const window = coverWindow(count, cursor);
        const xs = window.map(index => coverLayout(index, cursor, count).x).sort((a, b) => a - b);
        for (let at = 1; at < xs.length; at++) expect(xs[at] - xs[at - 1]).toBeGreaterThanOrEqual(COVER_COLUMN - 1e-9);
        for (const index of window) {
          const layout = coverLayout(index, cursor, count);
          expect(Number.isFinite(layout.x) && Number.isFinite(layout.y)).toBe(true);
          expect(layout.opacity).toBeGreaterThanOrEqual(0);
          expect(layout.opacity).toBeLessThanOrEqual(1);
        }
      }
    }
  });
  it('glides a browsed step while a drag stays locked to the pointer', () => {
    expect(easeCursor(3, 4, true)).toBe(4);
    // A step crosses part of the way in one frame and settles without overshooting.
    const first = easeCursor(3, 4, false);
    expect(first).toBeGreaterThan(3);
    expect(first).toBeLessThan(4);
    let at = 3;
    for (let frame = 0; frame < 60 && at !== 4; frame++) {
      const next = easeCursor(at, 4, false);
      expect(next).toBeGreaterThanOrEqual(at);
      expect(next).toBeLessThanOrEqual(4);
      at = next;
    }
    expect(at).toBe(4);
    // Even a jump across the library is a sweep of under one sleeve a frame.
    expect(easeCursor(0, 40, false)).toBeLessThanOrEqual(1);
    expect(easeCursor(0, Number.NaN, false)).toBe(0);
  });
  it('knows when the shelf has come to rest', () => {
    // Settled: no hand, no throw and nothing left to glide.
    expect(shelfMoving({ cursor: 2, tilt: 0, dragging: false, gliding: false }, 2)).toBe(false);
    // A hand on the shelf, a throw still coasting and a half-finished step all move.
    expect(shelfMoving({ cursor: 2, tilt: 0, dragging: true, gliding: false }, 2)).toBe(true);
    expect(shelfMoving({ cursor: 2, tilt: 0, dragging: false, gliding: true }, 2)).toBe(true);
    expect(shelfMoving({ cursor: 2.4, tilt: 0, dragging: false, gliding: false }, 2)).toBe(true);
  });
  it('carries a thrown shelf and settles it on a song', () => {
    // Leftwards is forwards: two sleeves in 40 ms is fifty songs a second.
    expect(dragVelocity([{ x: 300, t: 0 }, { x: 242, t: 20 }, { x: 184, t: 40 }])).toBeCloseTo(50, 6);
    expect(dragVelocity([{ x: 184, t: 40 }, { x: 242, t: 70 }])).toBeLessThan(0);
    expect(dragVelocity([{ x: 300, t: 0 }])).toBe(0);
    expect(dragVelocity([{ x: 300, t: 0 }, { x: 184, t: 0 }])).toBe(0);
    expect(clampFling(1e6)).toBe(FLING_MAX);
    expect(clampFling(-1e6)).toBe(-FLING_MAX);
    expect(clampFling(Number.NaN)).toBe(0);

    // The hardest throw crosses a handful of sleeves, and takes long enough to read as
    // momentum rather than as a jump.
    let cursor = 0, velocity = clampFling(1e6), frames = 0;
    while (velocity && frames < 600) {
      const next = flingStep(cursor, velocity, 1 / 60);
      expect(next.cursor).toBeGreaterThan(cursor);
      cursor = next.cursor; velocity = next.velocity; frames++;
    }
    expect(cursor).toBeGreaterThan(3);
    expect(cursor).toBeLessThan(6);
    expect(frames).toBeGreaterThan(20);
    expect(Math.abs(velocity)).toBeLessThan(FLING_MIN);
  });
  it('lays the sleeves on one shelf with a shallow arc for depth', () => {
    for (const cursor of [0, 7, 33]) {
      const shelf = coverWindow(40, cursor).map(index => coverLayout(index, cursor, 40).y);
      // The album in hand sits on the shelf line and the whole shelf stays one band.
      expect(Math.abs(Math.min(...shelf))).toBeLessThan(0.001);
      expect(Math.max(...shelf) - Math.min(...shelf)).toBeLessThan(0.04);
    }
    // Songs the same distance out sit at the same height, either side of the cursor.
    expect(coverLayout(6, 10, 40).y).toBeCloseTo(coverLayout(14, 10, 40).y, 6);
    // Height grows with distance, so sleeves fall away smoothly instead of stepping rows.
    const walk = [0, 1, 2, 3, 4].map(step => coverLayout(10 + step, 10, 40).y);
    for (let at = 1; at < walk.length; at++) expect(walk[at]).toBeGreaterThan(walk[at - 1]);
    const a = coverLayout(0, 39.99, 40);
    const b = coverLayout(0, 40.01, 40);
    expect(Math.abs(a.x - b.x)).toBeLessThan(0.01);
  });
  it('brings a sleeve in from nothing, so the window never pops', () => {
    const edge = (COVER_WINDOW - 1) / 2;
    expect(coverLayout(0, 0, 40).opacity).toBe(1);
    expect(coverLayout(1, 0, 40).opacity).toBe(1);
    expect(coverLayout(edge, 0, 40).opacity).toBe(0);
    // Fading only starts beyond the sleeve beside the one in hand.
    for (let step = 0; step <= 1; step++) expect(coverLayout(step, 0, 40).opacity).toBe(1);
    const fading = Array.from({ length: 8 }, (_, step) => coverLayout(1.5 + step * 0.2, 0, 40).opacity);
    for (let at = 1; at < fading.length; at++) expect(fading[at]).toBeLessThanOrEqual(fading[at - 1]);
  });
});

describe('when the shelf asks for a library', () => {
  it('invites only when there is no library and no input being heard', () => {
    expect(showsLibraryInvite(0, false, '', false)).toBe(true);
    expect(showsLibraryInvite(0, false, '', true)).toBe(false);
    // A library that is there is never invited, whatever the input is doing.
    expect(showsLibraryInvite(12, false, '', false)).toBe(false);
    expect(showsLibraryInvite(12, false, '', true)).toBe(false);
  });
  it('still reports a library being mapped or a failure while the input is heard', () => {
    // These are not the "no music" message: they are the state of an actual request.
    expect(showsLibraryInvite(0, true, '', true)).toBe(true);
    expect(showsLibraryInvite(0, false, 'The folder is gone', true)).toBe(true);
  });
  it('comes back once the input falls quiet', () => {
    const during = showsLibraryInvite(0, false, '', true);
    const after = showsLibraryInvite(0, false, '', false);
    expect(during).toBe(false);
    expect(after).toBe(true);
  });
});
