import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { PlayerBar } from './PlayerBar';
import type { PlaybackStatus } from '../model';
import type { RepeatMode } from '../usePlayer';
import type { Artwork } from '../useArtwork';

const idle: PlaybackStatus = {
  active: false, playing: false, finished: false, title: '', artist: '', album: '', path: '',
  format: '', device: '', position: 0, duration: 0, volume: 0.7, spatial: '', error: null,
};
const base = {
  hasQueue: true, busy: false, shuffle: false, repeat: 'off' as RepeatMode,
  onToggle: () => {}, onNext: () => {}, onPrevious: () => {}, onShuffle: () => {}, onRepeat: () => {},
  onSeek: () => {}, onStop: () => {}, onControls: () => {},
};
const render = (playback: PlaybackStatus, extra: Partial<typeof base> & { cover?: Artwork } = {}) =>
  renderToStaticMarkup(<PlayerBar {...base} {...extra} playback={playback} />);

describe('player bar', () => {
  it('stays honest while nothing is playing', () => {
    const markup = render(idle);
    expect(markup).toContain('No track loaded');
    expect(markup).toContain('Select a track in Library');
    expect(markup).toContain('STANDBY');
    expect(markup).toContain('aria-label="Play playback"');
    // Nothing is streaming, so there is nothing to stop.
    expect(markup).toMatch(/<button[^>]*disabled=""[^>]*aria-label="Stop playback"/);
    expect(markup).toContain('aria-valuetext="0:00 of 0:00"');
    expect(markup).toContain('Seek');
    expect(markup).toMatch(/<input[^>]*disabled/);
  });
  it('shows the real track, transport and progress while playing', () => {
    const markup = render({
      ...idle, active: true, playing: true, title: 'Says', artist: 'Nils Frahm', album: 'Spaces',
      path: 'D:\\Music\\Says.flac', format: 'FLAC · 44.1 kHz · stereo', device: 'Speakers (USB)',
      position: 42.4, duration: 200,
    });
    expect(markup).toContain('Says');
    expect(markup).toContain('Nils Frahm · Spaces');
    expect(markup).toContain('PLAYING');
    expect(markup).toContain('aria-label="Pause playback"');
    // A playing stream can be stopped, and stopping is what closes the output.
    expect(markup).not.toMatch(/<button[^>]*disabled=""[^>]*aria-label="Stop playback"/);
    expect(markup).toContain('aria-valuetext="0:42 of 3:20"');
    expect(markup).toContain('>0:42<');
    expect(markup).toContain('>3:20<');
    expect(markup).not.toMatch(/<input[^>]*disabled/);
  });
  it('reports a paused track without pretending it is playing', () => {
    const markup = render({ ...idle, active: true, playing: false, title: 'Looped', duration: 60 });
    expect(markup).toContain('PAUSED');
    expect(markup).toContain('aria-label="Play playback"');
    expect(markup).toContain('>Looped<');
  });
  it('never claims a system mix: playback is the only source', () => {
    const markup = render(idle);
    expect(markup).not.toContain('Windows output mix');
    expect(markup).not.toContain('MONITORING');
  });
  it('hangs the playing track\'s cover in the deck when one has been read', () => {
    const markup = render(
      { ...idle, active: true, playing: true, title: 'Says', path: 'D:\\Music\\Says.flac' },
      { cover: { url: 'blob:cover', state: 'ready' } },
    );
    expect(markup).toContain('src="blob:cover"');
    // The fallback glyph stays under the print, so a missing cover is still a track.
    expect(render({ ...idle, active: true, playing: true, title: 'Says' })).not.toContain('<img');
  });
  it('puts shuffle on the left of the transport and repeat on the right', () => {
    const markup = render(idle);
    const shuffle = markup.indexOf('aria-label="Shuffle off"');
    const previous = markup.indexOf('aria-label="Previous track"');
    const next = markup.indexOf('aria-label="Next track"');
    const repeat = markup.indexOf('aria-label="Repeat off"');
    expect(shuffle).toBeGreaterThan(-1);
    expect(shuffle).toBeLessThan(previous);
    expect(repeat).toBeGreaterThan(next);
    // Both are queue modes, not playback: they stay usable with nothing loaded.
    expect(markup).toMatch(/<button[^>]*aria-pressed="false"[^>]*aria-label="Shuffle off"/);
    expect(markup).not.toMatch(/<button[^>]*disabled=""[^>]*aria-label="Shuffle off"/);
  });
  it('reports both modes when they are on', () => {
    const shuffled = render(idle, { shuffle: true });
    expect(shuffled).toContain('aria-pressed="true"');
    expect(shuffled).toContain('aria-label="Shuffle on"');
    const all = render(idle, { repeat: 'all' });
    expect(all).toContain('aria-label="Repeat queue"');
    expect(all).toMatch(/<button[^>]*aria-pressed="true"[^>]*aria-label="Repeat queue"/);
    expect(render(idle, { repeat: 'one' })).toContain('aria-label="Repeat one track"');
  });
  it('disables the transport when there is no library queue', () => {
    const markup = render(idle, { hasQueue: false });
    // React writes disabled before the label, so match the whole button.
    expect(markup).toMatch(/<button[^>]*disabled=""[^>]*aria-label="Previous track"/);
    expect(markup).toMatch(/<button[^>]*aria-label="Next track"/);
    expect(markup).toMatch(/<button[^>]*disabled=""[^>]*aria-label="Play playback"/);
  });
});
