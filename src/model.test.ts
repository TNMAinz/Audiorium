import { describe, expect, it } from 'vitest';
import { defaultChain, defaultOrder, isStageOrder, moveStage, themes, workspaces } from './model';

describe('workspace and scene configuration', () => {
  it('defines accessible copy for every navigation destination', () => {
    expect(Object.keys(workspaces)).toEqual(['Orbit', 'Spectrum', 'Flow', 'Processors', 'Eq', 'Master', 'Spatial', 'Library', 'Devices']);
    for (const item of Object.values(workspaces)) {
      expect(item.title.length).toBeGreaterThan(0);
      expect(item.description.length).toBeGreaterThan(0);
    }
  });
  it('keeps every processor in the rack when one is moved', () => {
    const moved = moveStage(defaultOrder, 0, 2);
    expect(moved).toEqual(['reverb', 'eq', 'panner', 'compressor', 'saturator', 'limiter']);
    expect(isStageOrder(moved)).toBe(true);
    // A move past either end is the same rack, not a lost stage.
    expect(moveStage(defaultOrder, -1, 2)).toBe(defaultOrder);
    expect(moveStage(defaultOrder, 0, 6)).toBe(defaultOrder);
    // A list that drops or repeats a processor is not an order the engine accepts.
    expect(isStageOrder(['panner', 'reverb'])).toBe(false);
    expect(isStageOrder([...defaultOrder, 'limiter'])).toBe(false);
    expect(defaultChain.order).toEqual(defaultOrder);
  });
  it('defines valid primary and secondary colors for each scene theme', () => {
    for (const theme of Object.values(themes)) {
      expect(theme.primary).toMatch(/^#[0-9a-f]{6}$/i);
      expect(theme.secondary).toMatch(/^#[0-9a-f]{6}$/i);
    }
  });
});
