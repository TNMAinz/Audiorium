import { describe, expect, it } from 'vitest';
import { goniometerPoint, holdsLastFrame, SURFACE_FADE_MS, surfaceFade, TRACE_FADE_MS, traceFade } from './SignalLandscape';

describe('goniometer mapping', () => {
  it('puts mid on the vertical axis and nothing of it across', () => {
    const mono = goniometerPoint(0.5, 0, 100);
    expect(mono.x).toBeCloseTo(100, 5);
    expect(mono.y).toBeCloseTo(50, 5);
  });

  it('draws a left-heavy pair on the left, on the L diagonal', () => {
    const left = goniometerPoint(0.4, 0.4, 100);
    expect(left.x).toBeLessThan(100);
    expect(left.y).toBeLessThan(100);
    // Equal mid and side is a hard-left source: exactly on the diagonal.
    expect(left.x - 100).toBeCloseTo(left.y - 100, 5);
  });

  it('draws an inverted pair across the horizontal axis', () => {
    const inverted = goniometerPoint(0, 0.4, 100);
    expect(inverted.y).toBeCloseTo(100, 5);
    expect(inverted.x).toBeCloseTo(60, 5);
  });

  it('lets a hard-panned frame reach the ring instead of stopping at the diamond', () => {
    // Full scale in one channel is mid 0.5, side 0.5: the mid/side diamond puts that at 71 %
    // of the ring, so the L and R diagonals were physically unreachable.
    const hardLeft = goniometerPoint(0.5, 0.5, 100);
    const hardRight = goniometerPoint(0.5, -0.5, 100);
    expect(Math.hypot(hardLeft.x - 100, hardLeft.y - 100)).toBeCloseTo(100, 5);
    expect(Math.hypot(hardRight.x - 100, hardRight.y - 100)).toBeCloseTo(100, 5);
  });

  it('keeps a sample at its own direction, halfway out at half level', () => {
    const half = goniometerPoint(0.125, 0.375, 100);
    expect(Math.hypot(half.x - 100, half.y - 100)).toBeCloseTo(50, 5);
    expect(half.x - 100).toBeCloseTo(3 * (half.y - 100), 5);
  });

  it('reaches every direction of the circle at full scale', () => {
    for (const [mid, side] of [[1, 0], [0, 1], [0.5, 0.5], [0.5, -0.5], [0.2, 0.8], [-0.5, 0.5]]) {
      const point = goniometerPoint(mid, side, 100);
      expect(Math.hypot(point.x - 100, point.y - 100)).toBeCloseTo(100, 5);
    }
  });
});

describe('pause fade', () => {
  it('holds the last picture on a pause and gives it up only on a stop', () => {
    // Both plots ask this: while the player is loaded or playing the picture stands, and
    // only a stop (neither) starts the fade.
    expect(holdsLastFrame(true, false)).toBe(true);
    expect(holdsLastFrame(false, true)).toBe(true);
    expect(holdsLastFrame(false, false)).toBe(false);
  });

  it('wipes the trace a little harder as the pause ages, and not in one poll', () => {
    // A tenth of the trace at most per 20 ms poll: the first idle frame cannot blank it.
    expect(traceFade(0)).toBeCloseTo(0.04, 5);
    expect(traceFade(0)).toBeLessThan(0.1);
    expect(traceFade(TRACE_FADE_MS)).toBeCloseTo(0.14, 5);
    expect(traceFade(TRACE_FADE_MS / 2)).toBeGreaterThan(traceFade(0));
    expect(traceFade(TRACE_FADE_MS * 3)).toBeCloseTo(0.14, 5);
  });

  it('holds the landscape, then takes it to nothing over its own fade', () => {
    expect(surfaceFade(SURFACE_FADE_MS)).toBe(1);
    expect(surfaceFade(SURFACE_FADE_MS / 2)).toBeCloseTo(0.5, 5);
    expect(surfaceFade(0)).toBe(0);
    expect(surfaceFade(-40)).toBe(0);
    expect(surfaceFade(SURFACE_FADE_MS * 2)).toBe(1);
  });
});
