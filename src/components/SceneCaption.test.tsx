import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { defaultChain, idleMeters } from '../model';
import { SceneCaption } from './SceneCaption';
import { sceneParts } from './observatory';

const parts = sceneParts({
  analysis: {
    playing: false, source: 'playback', paused: false, format: '48 kHz · 32-bit float · stereo',
    rms: 0, peak: 0, low: 0, mid: 0, high: 0, correlation: 0, balance: 0, vectorscope: [], spectrum: [],
  },
  chain: defaultChain,
  meters: idleMeters,
  playback: {
    active: false, playing: false, finished: false, title: '', artist: '', album: '', path: '',
    format: '', device: '', position: 0, duration: 0, volume: 0.7, spatial: '', error: null,
  },
});

describe('the observatory caption', () => {
  it('says nothing at all until a part is under the pointer', () => {
    expect(renderToStaticMarkup(<SceneCaption part={null} />)).toBe('');
  });
  it('names the part, its live reading and the page behind it', () => {
    const markup = renderToStaticMarkup(<SceneCaption part={parts.find(part => part.id === 'limiter')!} />);
    expect(markup).toContain('scene-caption');
    expect(markup).toContain('Limiter');
    expect(markup).toContain('Bypassed · threshold −3.0 dB');
    expect(markup).toContain('Signal chain →');
  });
  it('shows a reading that is held, not a fabricated one', () => {
    const held = sceneParts({
      analysis: {
        playing: false, source: 'playback', paused: true, format: '48 kHz · stereo',
        rms: 0.2, peak: 0.4, low: 0, mid: 0, high: 0, correlation: 0.9, balance: 0,
        vectorscope: [], spectrum: [0.1, 0.2],
      },
      chain: defaultChain,
      meters: idleMeters,
      playback: {
        active: true, playing: false, finished: false, title: 'Says', artist: '', album: '', path: '',
        format: '', device: '', position: 30, duration: 120, volume: 0.7, spatial: '', error: null,
      },
    });
    const markup = renderToStaticMarkup(<SceneCaption part={held.find(part => part.id === 'programme')!} />);
    expect(markup).toContain('Held ·');
    expect(markup).toContain('Analysis →');
  });
});
