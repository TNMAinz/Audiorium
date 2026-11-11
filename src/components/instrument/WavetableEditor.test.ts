import { describe, expect, it } from 'vitest';
import { spectrum, waveform, warpSpectrum } from './WavetableEditor';

describe('wavetable spectral editing', () => {
  it('round-trips a cosine without losing its phase', () => {
    const original = Array.from({ length: 2048 }, (_, i) => Math.cos(2 * Math.PI * i / 2048));
    const bins = spectrum(original);
    expect(bins[0].cosine).toBeCloseTo(1, 5);
    expect(bins[0].sine).toBeCloseTo(0, 5);
    const restored = waveform(bins);
    expect(restored[0]).toBeCloseTo(1, 4);
    expect(restored[512]).toBeCloseTo(0, 4);
  });

  it('keeps a neutral warp unchanged and tilts upper harmonics', () => {
    const bins = Array.from({ length: 128 }, (_, i) => ({ sine: i < 2 ? 0.25 : 0, cosine: 0 }));
    expect(warpSpectrum(bins, 0, 1)).toEqual(bins);
    const tilted = warpSpectrum(bins, 1, 1);
    expect(tilted[0].sine).toBeCloseTo(0.25);
    expect(tilted[1].sine).toBeCloseTo(0.5);
  });

  it('stretches both phase components to a higher harmonic', () => {
    const bins = Array.from({ length: 128 }, (_, i) => ({ sine: i === 0 ? 0.4 : 0, cosine: i === 0 ? 0.3 : 0 }));
    const stretched = warpSpectrum(bins, 0, 2);
    expect(stretched[1].sine).toBeCloseTo(0.4);
    expect(stretched[1].cosine).toBeCloseTo(0.3);
    const reconstructed = spectrum(waveform(stretched));
    expect(reconstructed[1].sine).toBeCloseTo(0.4, 5);
    expect(reconstructed[1].cosine).toBeCloseTo(0.3, 5);
  });

  it('returns no bins for an empty frame', () => {
    expect(spectrum([])).toEqual([]);
  });
});
