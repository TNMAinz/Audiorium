import { describe, expect, it } from 'vitest';
import { defaultChain, defaultOrder, defaultSynth, describeSynth, idleSynthStatus, isStageOrder, moveStage, themes } from './model';

describe('rack and theme configuration', () => {
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

describe('the patch description', () => {
  it('names the unison stack and the filter model it will play', () => {
    const stack = {
      ...defaultSynth,
      osc1: { ...defaultSynth.osc1, unison: 7 },
      filter: { ...defaultSynth.filter, kind: 'ladder' as const },
    };
    const line = describeSynth(stack, idleSynthStatus);
    expect(line).toContain('7× unison');
    expect(line).toContain('Ladder');
    // A patch without a stack says nothing about one.
    expect(describeSynth(defaultSynth, idleSynthStatus)).not.toContain('unison');
  });
});
