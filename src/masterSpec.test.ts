import { describe, expect, it } from 'vitest';
import { idleLoudness, type LoudnessReadings } from './model';
import { CEILING_TOLERANCE_DB, checkMaster, gainToTarget, masterTargets, TARGET_TOLERANCE_LU } from './masterSpec';

const target = (id: string) => masterTargets.find(entry => entry.id === id)!;
const measured = (integratedLufs: number, truePeakDb: number): LoudnessReadings => ({
  ...idleLoudness, blocks: 40, integratedLufs, truePeakDb,
});

describe('the delivery target', () => {
  it('offers the usual destinations and a way to judge nothing', () => {
    expect(masterTargets.map(entry => entry.id)).toEqual(['streaming', 'podcast', 'broadcast', 'off']);
    expect(target('streaming').lufs).toBe(-14);
    expect(target('broadcast').lufs).toBe(-23);
    expect(target('off').lufs).toBeNull();
    // Every target states a ceiling, so the verdict can name one even when the
    // loudness figure is not being judged.
    expect(masterTargets.every(entry => entry.ceilingDb < 0)).toBe(true);
  });

  it('says a master meets the target when both the loudness and the ceiling hold', () => {
    const check = checkMaster(target('streaming'), measured(-14.2, -1.4))!;
    expect(check.judged).toBe(true);
    expect(check.loudnessHolds).toBe(true);
    expect(check.peakHolds).toBe(true);
    expect(check.meets).toBe(true);
    expect(check.deltaLu).toBeCloseTo(-0.2, 5);
    expect(check.peakOverDb).toBeCloseTo(-0.4, 5);
  });

  it('reports the distance when the loudness misses, in the direction it missed', () => {
    const loud = checkMaster(target('streaming'), measured(-11.7, -2.0))!;
    expect(loud.meets).toBe(false);
    expect(loud.loudnessHolds).toBe(false);
    expect(loud.peakHolds).toBe(true);
    expect(loud.deltaLu).toBeCloseTo(2.3, 5);
    const quiet = checkMaster(target('broadcast'), measured(-26.4, -3.0))!;
    expect(quiet.deltaLu).toBeCloseTo(-3.4, 5);
    // A target's own tolerance is what decides, not an exact equality.
    expect(checkMaster(target('streaming'), measured(-14 + TARGET_TOLERANCE_LU, -2))!.loudnessHolds).toBe(true);
    expect(checkMaster(target('streaming'), measured(-14 + TARGET_TOLERANCE_LU + 0.1, -2))!.loudnessHolds).toBe(false);
  });

  it('fails a master whose true peak is over the ceiling even when the loudness is right', () => {
    const check = checkMaster(target('streaming'), measured(-14.0, -0.2))!;
    expect(check.meets).toBe(false);
    expect(check.loudnessHolds).toBe(true);
    expect(check.peakHolds).toBe(false);
    expect(check.peakOverDb).toBeCloseTo(0.8, 5);
    // At the ceiling exactly is inside it; a hair over is not.
    expect(checkMaster(target('streaming'), measured(-14, -1))!.peakHolds).toBe(true);
    expect(checkMaster(target('streaming'), measured(-14, -1 + CEILING_TOLERANCE_DB + 0.01))!.peakHolds).toBe(false);
  });

  it('measures without judging when the target is None', () => {
    const check = checkMaster(target('off'), measured(-8.4, -0.9))!;
    expect(check.judged).toBe(false);
    expect(check.meets).toBe(true);
    expect(check.peakHolds).toBe(true);
    expect(gainToTarget(target('off'), measured(-8.4, -0.9))).toBeNull();
  });

  it('has no verdict before a render has measured anything', () => {
    expect(checkMaster(target('streaming'), idleLoudness)).toBeNull();
    expect(checkMaster(target('streaming'), { ...measured(-14, -1), blocks: 0 })).toBeNull();
    // A figure the meter reports as its floor is not a measurement either.
    expect(checkMaster(target('streaming'), { ...measured(-14, -1), truePeakDb: -120 })).toBeNull();
    expect(checkMaster(target('streaming'), { ...measured(-14, -1), integratedLufs: -70 })).toBeNull();
  });

  it('states the gain that would reach the target instead of applying it', () => {
    expect(gainToTarget(target('streaming'), measured(-11.7, -2.0))).toBeCloseTo(-2.3, 5);
    expect(gainToTarget(target('broadcast'), measured(-26.4, -3.0))).toBeCloseTo(3.4, 5);
    expect(gainToTarget(target('streaming'), idleLoudness)).toBeNull();
  });
});
