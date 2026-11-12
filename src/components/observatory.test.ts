import { describe, expect, it } from 'vitest';
import { idleMeters, type ChainMeters } from '../model';
import {
  engagedStages, formatClock, formatLevel, levelDb, loudestBand,
  spectrumHz, trackFraction,
} from './observatory';

describe('levels as the meters show them', () => {
  it('reads a magnitude as dBFS, with a floor instead of minus infinity', () => {
    expect(formatLevel(1)).toBe('0.0 dBFS');
    expect(formatLevel(0.5)).toBe('−6.0 dBFS');
    expect(formatLevel(0.001)).toBe('−60.0 dBFS');
    expect(formatLevel(0)).toBe('−60.0 dBFS');
    expect(levelDb(Number.NaN)).toBe(-60);
  });
  it('reads a clock the way the transport does', () => {
    expect(formatClock(0)).toBe('0:00');
    expect(formatClock(64.4)).toBe('1:04');
    expect(formatClock(3725)).toBe('62:05');
    expect(formatClock(-3)).toBe('0:00');
  });
});

describe('the log bands behind the spectrum ring', () => {
  it('starts at 30 Hz and ends at 16 kHz', () => {
    expect(spectrumHz(0).low).toBeCloseTo(30, 5);
    expect(spectrumHz(55).high).toBeCloseTo(16_000, 0);
    const band = spectrumHz(28);
    expect(band.low).toBeLessThan(band.centre);
    expect(band.centre).toBeLessThan(band.high);
  });
  it('finds the loudest band and names its frequency', () => {
    const spectrum = Array.from({ length: 56 }, (_, at) => (at === 28 ? 0.9 : 0.1));
    const loudest = loudestBand(spectrum);
    expect(loudest?.band).toBe(28);
    expect(loudest!.hz).toBeGreaterThan(700);
    expect(loudest!.hz).toBeLessThan(760);
  });
  it('has no loudest band when nothing was measured', () => {
    expect(loudestBand([])).toBeNull();
    expect(loudestBand([Number.NaN])).toBeNull();
  });
});

describe('where the track is on its orbit', () => {
  it('is a fraction of the duration, clamped', () => {
    expect(trackFraction(30, 60)).toBe(0.5);
    expect(trackFraction(90, 60)).toBe(1);
    expect(trackFraction(-4, 60)).toBe(0);
    expect(trackFraction(10, 0)).toBe(0);
    expect(trackFraction(Number.NaN, 60)).toBe(0);
  });
});

describe('how many processors the engine is running', () => {
  it('counts the stages a bypassed rack leaves off', () => {
    expect(engagedStages(idleMeters)).toBe(0);
  });
  it('counts only the enabled ones', () => {
    const meters: ChainMeters = { ...idleMeters, eqEnabled: true, limiterEnabled: true };
    expect(engagedStages(meters)).toBe(2);
  });
});
