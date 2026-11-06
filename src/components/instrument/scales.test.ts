import { describe, expect, it } from 'vitest';
import {
  arcPath, clamp, dbTicks, dragToValue, knobAngle, knobFraction, padToValue, quantize,
  reflectionTaps, spacedLabels, stepValue, valueToPad, KNOB_START, KNOB_SWEEP,
} from './scales';

describe('knob geometry', () => {
  it('sweeps 270° from the minimum to the maximum', () => {
    expect(knobAngle(-24, -24, 0)).toBe(KNOB_START);
    expect(knobAngle(0, -24, 0)).toBe(KNOB_START + KNOB_SWEEP);
    expect(knobAngle(-12, -24, 0)).toBeCloseTo(-135 + 135, 6);
    expect(knobFraction(5, 0, 10)).toBe(0.5);
  });
  it('clamps values outside its range instead of over-rotating', () => {
    expect(knobAngle(-40, -24, 0)).toBe(KNOB_START);
    expect(knobAngle(9, -24, 0)).toBe(KNOB_START + KNOB_SWEEP);
    expect(knobFraction(1, 2, 2)).toBe(0);
    expect(clamp(12, 0, 1)).toBe(1);
  });
  it('draws an arc between two angles', () => {
    expect(arcPath(10, 10, 5, 0, 90)).toContain('A 5 5');
    expect(arcPath(10, 10, 5, 0, 200)).toContain(' 1 1 ');
  });
});

describe('knob interaction', () => {
  it('turns up when dragged up and down when dragged down', () => {
    const base = { start: -12, min: -24, max: 0, step: 0.5 };
    expect(dragToValue({ ...base, dy: -90 })).toBe(0);
    expect(dragToValue({ ...base, dy: 90 })).toBe(-24);
    // One pixel is 24 dB / 180 px, then the value snaps to its 0.5 dB step.
    expect(dragToValue({ ...base, dy: -15 })).toBe(-10);
    expect(dragToValue({ ...base, dy: -9 })).toBe(-11);
  });
  it('is five times finer with the fine modifier held', () => {
    const base = { start: 0, min: 0, max: 1, step: 0.01 };
    expect(dragToValue({ ...base, dy: -18 })).toBe(0.1);
    expect(dragToValue({ ...base, dy: -18, fine: true })).toBe(0.02);
  });
  it('steps, quantizes and stays inside the range', () => {
    expect(stepValue(0.5, 1, 0.01, 0, 1)).toBe(0.51);
    expect(stepValue(0.5, -1, 0.01, 0, 1, 10)).toBe(0.4);
    expect(stepValue(1, 1, 0.01, 0, 1)).toBe(1);
    expect(quantize(0.30000000000000004, 0, 1, 0.01)).toBe(0.3);
  });
});

describe('position field', () => {
  it('maps the pad centre to straight ahead at ear level', () => {
    const centre = padToValue(0, 0, { radius: 60 });
    expect(centre.azimuth).toBe(0);
    expect(centre.elevation).toBeCloseTo(0, 6);
  });
  it('maps the right edge hard right and the top edge overhead', () => {
    expect(padToValue(60, 0, { radius: 60 }).azimuth).toBe(180);
    expect(padToValue(0, -60, { radius: 60 }).elevation).toBe(90);
    expect(padToValue(-60, 0, { radius: 60 }).azimuth).toBe(-180);
    expect(padToValue(0, 30, { radius: 60 }).elevation).toBe(-45);
  });
  it('round-trips a position back to the same point', () => {
    const point = valueToPad(-90, 45, { radius: 60 });
    const back = padToValue(point.x, point.y, { radius: 60 });
    expect(back.azimuth).toBeCloseTo(-90, 6);
    expect(back.elevation).toBeCloseTo(45, 6);
  });
  it('never leaves the ranges at the pad corners', () => {
    const corner = padToValue(999, -999, { radius: 60 });
    expect(corner.azimuth).toBe(180);
    expect(corner.elevation).toBe(90);
  });
});

describe('displays', () => {
  it('places the four reflection taps and fades them in order', () => {
    const taps = reflectionTaps(0, 1);
    expect(taps.map(tap => tap.ms)).toEqual([2.45, 4.02, 5.95, 8.05]);
    expect(taps[0].level).toBeGreaterThan(taps[3].level);
    const longer = reflectionTaps(1, 0.5);
    expect(longer[3].ms).toBe(23);
  });
  it('grades a meter from its tick values', () => {
    const ticks = dbTicks(0, -24, [0, -12, -24]);
    expect(ticks.map(tick => tick.fraction)).toEqual([1, 0.5, 0]);
  });
  it('drops labels that would collide instead of letting them overlap', () => {
    const labels = spacedLabels(dbTicks(0, -48, [-48, -36, -24, -12, -6, -3, 0]));
    expect(labels.map(tick => tick.db)).toEqual([-48, -36, -24, -12, -6, 0]);
    // The scale still ends on its loudest tick.
    const crowded = spacedLabels([{ fraction: 0.95 }, { fraction: 1 }]);
    expect(crowded).toEqual([{ fraction: 1 }]);
  });
});
