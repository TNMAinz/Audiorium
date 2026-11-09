import { describe, expect, it } from 'vitest';
import { defaultSynth } from '../../model';
import {
  channelFilterState, channelFilterStep, envelopePlot, midiHz, previewVoice, settledCutoff, svfStep, waveSample,
} from './voice';

describe('the voice, without the engine', () => {
  it('tunes A4 to 440 Hz and an octave to twice that', () => {
    expect(midiHz(69)).toBeCloseTo(440, 6);
    expect(midiHz(81) / midiHz(69)).toBeCloseTo(2, 6);
    expect(midiHz(57) / midiHz(69)).toBeCloseTo(0.5, 6);
  });

  it('draws the four shapes the engine draws', () => {
    // A saw ramps from -1 to 1 and falls back at the wrap. At the wrap itself the
    // anti-aliasing correction rounds the step, so the sample sits on the way down.
    expect(waveSample('saw', 0, 0.001)).toBeCloseTo(0, 3);
    expect(waveSample('saw', 0.0015, 0.001)).toBeCloseTo(-0.997, 2);
    expect(waveSample('saw', 0.25, 0.001)).toBeCloseTo(-0.5, 3);
    expect(waveSample('saw', 0.5, 0.001)).toBeCloseTo(0, 3);
    // A square is +1 for the first half, -1 for the second.
    expect(waveSample('square', 0.25, 0.001)).toBeCloseTo(1, 3);
    expect(waveSample('square', 0.75, 0.001)).toBeCloseTo(-1, 3);
    // A triangle peaks in the middle and troughs at both ends.
    expect(waveSample('triangle', 0.5, 0.001)).toBeCloseTo(1, 3);
    expect(waveSample('triangle', 0, 0.001)).toBeCloseTo(-1, 3);
    // A sine is a sine.
    expect(waveSample('sine', 0.25, 0.001)).toBeCloseTo(1, 3);
    expect(waveSample('sine', 0.5, 0.001)).toBeCloseTo(0, 3);
  });

  it('starts both oscillators at their own phase zero, and keeps them apart after', () => {
    const { osc1, osc2 } = previewVoice(defaultSynth, 69);
    expect(osc1[0]).toBeCloseTo(0, 6);
    expect(osc2[0]).toBeCloseTo(0, 6);
    expect(osc1.length).toBe(osc2.length);
    expect(osc1.length).toBeGreaterThan(100);
    // Each is scaled by its own level, so neither can be mistaken for the other.
    // The trace keeps one sample a column, so the exact peak of a ramp can fall
    // between two of them: it is the level, to a tenth.
    const peak = (values: number[]) => Math.max(...values.map(Math.abs));
    expect(peak(osc1)).toBeCloseTo(defaultSynth.osc1.level, 1);
    expect(peak(osc2)).toBeCloseTo(defaultSynth.osc2.level, 1);
  });

  it('is the same note every time, because a patch is a fixed thing', () => {
    const once = previewVoice(defaultSynth, 69);
    const twice = previewVoice(defaultSynth, 69);
    expect(twice.high).toEqual(once.high);
    expect(twice.low).toEqual(once.low);
  });

  it('closes the filter down when the cutoff is closed', () => {
    const bright = previewVoice({ ...defaultSynth, filter: { ...defaultSynth.filter, cutoff: 18_000 } }, 69);
    const dark = previewVoice({ ...defaultSynth, filter: { ...defaultSynth.filter, cutoff: 120 } }, 69);
    const energy = (values: number[]) => values.reduce((total, value) => total + Math.abs(value), 0) / values.length;
    // A closed cutoff takes most of a saw at A4 with it: a third of the level, not a
    // polish on the top end.
    expect(energy(bright.high)).toBeGreaterThan(energy(dark.high) * 2.5);
  });

  it('lets the key track move the cutoff an octave per octave', () => {
    const tracked = { ...defaultSynth, filter: { ...defaultSynth.filter, keyTrack: 1, envAmount: 0 } };
    expect(settledCutoff(tracked, 72)).toBeCloseTo(settledCutoff(tracked, 60) * 2, 4);
    expect(settledCutoff(tracked, 48)).toBeCloseTo(settledCutoff(tracked, 60) / 2, 4);
  });

  it('sweeps the cutoff with the filter envelope it is holding', () => {
    const flat = { ...defaultSynth, filter: { ...defaultSynth.filter, envAmount: 0, keyTrack: 0 } };
    const swept = { ...flat, filter: { ...flat.filter, envAmount: 1 }, filterEnv: { ...flat.filterEnv, sustain: 1 } };
    // One octave of envelope amount at full sustain is one octave of cutoff.
    expect(settledCutoff(swept, 60)).toBeCloseTo(settledCutoff(flat, 60) * 2 ** 4, 1);
  });

  it('damps a step through the state-variable filter instead of ringing forever', () => {
    const state = { ic1: 0, ic2: 0 };
    let last = 0;
    for (let index = 0; index < 4_800; index += 1) {
      last = svfStep(state, index === 0 ? 1 : 0, 1_000, 0.9, 'lowpass');
    }
    expect(Math.abs(last)).toBeLessThan(1e-3);
  });

  it('draws an envelope by running it: a linear attack, an exponential tail', () => {
    const plot = envelopePlot({ attack: 0.1, decay: 0.2, sustain: 0.5, release: 0.3 });
    const levels = plot.points.map(point => point.level);
    expect(Math.max(...levels)).toBeCloseTo(1, 6);
    expect(levels[0]).toBeGreaterThan(0);
    expect(levels[levels.length - 1]).toBe(0);
    // The stages come in the order they are played, and each one occupies its share.
    expect(plot.attackEnd).toBeGreaterThan(0);
    expect(plot.decayEnd).toBeGreaterThan(plot.attackEnd);
    expect(plot.releaseStart).toBeGreaterThan(plot.decayEnd);
    // Time only ever moves forward.
    const xs = plot.points.map(point => point.x);
    expect(xs.every((x, index) => index === 0 || x >= xs[index - 1])).toBe(true);
    expect(xs[xs.length - 1]).toBeCloseTo(1, 6);
  });

  it('holds the sustain for the tail it is given, and decays to nothing without one', () => {
    const held = envelopePlot({ attack: 0.01, decay: 0.05, sustain: 0.6, release: 0.2 }, 1);
    const short = envelopePlot({ attack: 0.01, decay: 0.05, sustain: 0.6, release: 0.2 }, 0.2);
    expect(held.releaseStart - held.decayEnd).toBeGreaterThan(short.releaseStart - short.decayEnd);
    const silent = envelopePlot({ attack: 0.01, decay: 0.05, sustain: 0, release: 0.2 }, 0.2);
    const tail = silent.points.filter(point => point.x > silent.releaseStart);
    expect(tail.every(point => point.level < 0.0006)).toBe(true);
  });

  it('reads a table cycle the engine handed over instead of inventing one', () => {
    const cycle = [0, 1, 0, -1];
    expect(waveSample('table', 0, 0.01, cycle)).toBeCloseTo(0, 6);
    expect(waveSample('table', 0.25, 0.01, cycle)).toBeCloseTo(1, 6);
    expect(waveSample('table', 0.75, 0.01, cycle)).toBeCloseTo(-1, 6);
    // Without a cycle it draws a sine, which is the only shape it may claim.
    expect(waveSample('table', 0.25, 0.01)).toBeCloseTo(1, 6);

    const patch = {
      ...defaultSynth,
      osc1: { ...defaultSynth.osc1, wave: 'table' as const, level: 1 },
      osc2: { ...defaultSynth.osc2, level: 0 },
      noise: 0,
      sub: 0,
      filter: { ...defaultSynth.filter, cutoff: 18_000, envAmount: 0, keyTrack: 0 },
    };
    const drawn = previewVoice(patch, 69, 64, { one: cycle });
    expect(Math.max(...drawn.osc1.map(Math.abs))).toBeGreaterThan(0.9);
  });

  it('runs the ladder and the comb at full resonance without running away', () => {
    for (const kind of ['ladder', 'comb'] as const) {
      const state = channelFilterState();
      let peak = 0;
      let finite = true;
      for (let index = 0; index < 24_000; index += 1) {
        const sample = channelFilterStep(state, Math.sin(index * 0.05) * 0.9, 220, 10, 1, kind);
        finite = finite && Number.isFinite(sample);
        peak = Math.max(peak, Math.abs(sample));
      }
      expect(finite, `${kind} produced a non-finite sample`).toBe(true);
      expect(peak, `${kind} ran away`).toBeLessThan(20);
    }
  });
});
