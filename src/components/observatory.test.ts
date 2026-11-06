import { describe, expect, it } from 'vitest';
import { defaultChain, idleMeters, workspaces, type ChainMeters, type ChainSettings, type PlaybackStatus } from '../model';
import type { AnalysisStatus } from '../useAnalysis';
import {
  engagedStages, formatClock, formatLevel, levelDb, loudestBand, partById,
  sceneParts, spectrumHz, trackFraction, type ScenePartId,
} from './observatory';

const idleStatus: AnalysisStatus = {
  playing: false, source: 'playback', paused: false, format: '48 kHz · 32-bit float · stereo',
  rms: 0, peak: 0, low: 0, mid: 0, high: 0, correlation: 0, balance: 0,
  vectorscope: [], spectrum: [],
};

const idlePlayback: PlaybackStatus = {
  active: false, playing: false, finished: false, title: '', artist: '', album: '', path: '',
  format: '', device: '', position: 0, duration: 0, volume: 0.7, spatial: '', error: null,
};

const sound = (patch: Partial<AnalysisStatus> = {}): AnalysisStatus => ({ ...idleStatus, ...patch });

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

describe('what the object says when nothing is running', () => {
  const parts = sceneParts({ analysis: idleStatus, chain: defaultChain, meters: idleMeters, playback: idlePlayback });

  it('admits the engine has written nothing rather than drawing a floor', () => {
    expect(partById(parts, 'programme')?.value).toBe('Nothing written');
    expect(partById(parts, 'spectrum')?.value).toBe('Flat · no frames');
    expect(partById(parts, 'track')?.value).toBe('Nothing loaded');
  });
  it('reports the bypassed stages as bypassed, with their settings still visible', () => {
    expect(partById(parts, 'equaliser')?.value).toBe('Bypassed');
    expect(partById(parts, 'panner')?.engaged).toBe(false);
    expect(partById(parts, 'panner')?.value).toBe('Bypassed · width 25 %');
    expect(partById(parts, 'limiter')?.value).toBe('Bypassed · threshold −3.0 dB');
    expect(engagedStages(idleMeters)).toBe(0);
  });
  it('gives every part a page that exists and an explanation', () => {
    for (const part of parts) {
      expect(Object.keys(workspaces)).toContain(part.action);
      expect(part.detail.length).toBeGreaterThan(20);
    }
    expect(parts.map(part => part.id)).toEqual<ScenePartId[]>([
      'programme', 'spectrum', 'equaliser', 'panner', 'room', 'limiter', 'track',
    ]);
  });
});

describe('what the object says while it plays', () => {
  const spectrum = Array.from({ length: 56 }, (_, at) => (at === 40 ? 0.95 : 0.05));
  const playing = sound({ playing: true, rms: 0.12, peak: 0.5, correlation: 0.5, balance: -0.2, spectrum });
  const meters: ChainMeters = { ...idleMeters, eqEnabled: true, limiterEnabled: true, gainReductionDb: -3.2 };
  const chain: ChainSettings = {
    ...defaultChain,
    eq: { enabled: true, preampDb: -2, filters: [
      { kind: 'peaking', frequency: 100, gainDb: 3, q: 1, enabled: true },
      { kind: 'peaking', frequency: 3_000, gainDb: -2, q: 1, enabled: true },
    ] },
  };
  const parts = sceneParts({ analysis: playing, chain, meters, playback: { ...idlePlayback, active: true, playing: true, title: 'Says', position: 64, duration: 200 } });

  it('reads the level the engine measured', () => {
    expect(partById(parts, 'programme')?.value).toBe('−18.4 dBFS RMS · peak −6.0 dBFS');
  });
  it('names the loudest band of the measured spectrum', () => {
    expect(partById(parts, 'spectrum')?.value).toMatch(/^loudest \d/);
    expect(partById(parts, 'spectrum')?.value).toContain('56 log bands');
  });
  it('describes the running equaliser, panner, room and limiter', () => {
    expect(partById(parts, 'equaliser')?.value).toBe('2 bands · preamp −2.0 dB');
    expect(partById(parts, 'limiter')?.value).toBe('−3.2 dB of reduction');
    expect(engagedStages(meters)).toBe(2);
    expect(partById(parts, 'track')?.value).toBe('1:04 / 3:20 · Says');
  });
  it('says a limiter that is not working is not reducing', () => {
    const resting = sceneParts({ analysis: playing, chain, meters: { ...meters, gainReductionDb: 0 }, playback: idlePlayback });
    expect(partById(resting, 'limiter')?.value).toBe('Not reducing · threshold −3.0 dB');
  });
  it('keeps a held reading when the track is paused', () => {
    const paused = sceneParts({ analysis: { ...playing, playing: false, paused: true }, chain, meters, playback: idlePlayback });
    expect(partById(paused, 'programme')?.value).toContain('Held ·');
  });
});

describe('focus', () => {
  const parts = sceneParts({ analysis: idleStatus, chain: defaultChain, meters: idleMeters, playback: idlePlayback });
  it('resolves a part by id and nothing by no id', () => {
    expect(partById(parts, 'equaliser')?.label).toBe('Equaliser');
    expect(partById(parts, null)).toBeNull();
  });
});
