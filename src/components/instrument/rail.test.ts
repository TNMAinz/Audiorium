import { describe, expect, it } from 'vitest';
import { MIN_THUMB, RAIL_INSET, railMetrics, railScrollTop } from './rail';

describe('panel scroll rail', () => {
  it('draws nothing when the list fits', () => {
    expect(railMetrics(223, 223, 0)).toBeNull();
    expect(railMetrics(223, 200, 0)).toBeNull();
  });

  it('scales the pill to the visible share of the list', () => {
    const metrics = railMetrics(223, 446, 0)!;
    expect(metrics.track).toBe(223 - RAIL_INSET * 2);
    expect(metrics.thumb).toBe(Math.round(metrics.track / 2));
    expect(metrics.top).toBe(0);
  });

  it('keeps the pill long enough to grab on a long list', () => {
    const metrics = railMetrics(120, 20_000, 0)!;
    expect(metrics.thumb).toBe(MIN_THUMB);
  });

  it('runs the pill from the top to the bottom of the track', () => {
    expect(railMetrics(200, 800, 0)!.top).toBe(0);
    const end = railMetrics(200, 800, 600)!;
    expect(end.top).toBe(end.track - end.thumb);
  });

  it('clamps an overscrolled offset instead of leaving the track', () => {
    const metrics = railMetrics(200, 800, 5000)!;
    expect(metrics.top).toBe(metrics.track - metrics.thumb);
  });

  it('maps a pointer to a scroll offset that keeps the grabbed point under the hand', () => {
    // 800 px of content in a 200 px view: the pill travels 168 px for 600 px of scroll.
    const metrics = railMetrics(200, 800, 0)!;
    const travel = metrics.track - metrics.thumb;
    expect(railScrollTop(200, 800, 0, 0)).toBe(0);
    expect(railScrollTop(200, 800, travel, 0)).toBeCloseTo(600, 5);
    expect(railScrollTop(200, 800, travel / 2, 0)).toBeCloseTo(300, 5);
    // Grabbing the middle of the pill holds that point, it does not jump to the top.
    expect(railScrollTop(200, 800, metrics.thumb / 2, metrics.thumb / 2)).toBeCloseTo(0, 5);
  });

  it('does not scroll a list that fits', () => {
    expect(railScrollTop(200, 200, 50, 0)).toBe(0);
  });
});
