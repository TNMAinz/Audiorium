import { invoke, isTauri } from '@tauri-apps/api/core';
import { useCallback, useEffect, useRef, useState } from 'react';
import type { LibraryTrack, PlaybackStatus } from './model';
import { localLevelStorage, readLevel, writeLevel } from './levelMemory';

const idle: PlaybackStatus = {
  active: false, playing: false, finished: false,
  title: '', artist: '', album: '', path: '', format: '', device: '',
  position: 0, duration: 0, volume: 0.7, spatial: '', error: null,
};
const normalize = (incoming: Partial<PlaybackStatus>): PlaybackStatus => ({ ...idle, ...incoming, error: incoming.error ?? null });

/** Repeat is off, loops the whole queue, or holds the one track. */
export type RepeatMode = 'off' | 'all' | 'one';

/**
 * Transport for library playback. The Rust side owns decoding and the output
 * stream; this hook only sends commands and reports what the engine says, so a
 * stalled poll can never invent progress.
 */
export function usePlayer(queue: LibraryTrack[], deviceId: string) {
  const native = isTauri();
  // The level the launch starts at: a setting the user made, so it is read back
  // rather than reset, and the engine is told before the first track opens.
  const startLevel = useRef(readLevel(localLevelStorage()));
  const [status, setStatus] = useState<PlaybackStatus>(() => ({ ...idle, volume: startLevel.current }));
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [shuffle, setShuffle] = useState(false);
  const [repeat, setRepeat] = useState<RepeatMode>('off');
  const autoAdvanced = useRef('');
  // A poll that left before the last drag can carry the level from before it. The
  // chosen level is held here until the engine reports the same one, so the slider
  // never jumps back under the pointer.
  const settlingVolume = useRef<number | null>(startLevel.current);

  const accept = useCallback((next: PlaybackStatus) => {
    const pending = settlingVolume.current;
    if (pending !== null && Math.abs(next.volume - pending) < 0.005) settlingVolume.current = null;
    setStatus(settlingVolume.current === null ? next : { ...next, volume: settlingVolume.current });
  }, []);

  const poll = useCallback(async () => {
    try { accept(normalize(await invoke<Partial<PlaybackStatus>>('playback_status'))); }
    catch (reason) { setError(String(reason)); }
  }, [accept]);

  // The engine keeps the level in memory only, so a launch hands it the remembered
  // one once; without this the first track would play at the default and the slider
  // would be right again only after the user moved it.
  useEffect(() => {
    if (!native) return;
    void invoke('set_playback_volume', { volume: startLevel.current })
      // A failed hand-off must not leave the slider holding a level the engine never
      // took: the poll's own reading is the truth from here.
      .catch(reason => { settlingVolume.current = null; setError(String(reason)); });
  }, [native]);

  useEffect(() => {
    if (!native) return;
    let active = true;
    let inFlight = false;
    const tick = async () => {
      if (inFlight) return;
      inFlight = true;
      try {
        const next = normalize(await invoke<Partial<PlaybackStatus>>('playback_status'));
        if (active) accept(next);
      } catch (reason) {
        if (active) setError(String(reason));
      } finally {
        inFlight = false;
      }
    };
    void tick();
    const timer = window.setInterval(() => void tick(), 250);
    return () => { active = false; window.clearInterval(timer); };
  }, [native, accept]);

  // The output is chosen on the Devices page. A track already open moves to it at
  // the position it has reached, because Windows cannot repoint an open stream.
  // The first run only records the device the next start will use.
  const openedOn = useRef(deviceId);
  useEffect(() => {
    if (openedOn.current === deviceId) return;
    openedOn.current = deviceId;
    if (!native || !status.active) return;
    void (async () => {
      try {
        await invoke('set_playback_device', { deviceId });
        await poll();
      } catch (reason) { setError(String(reason)); }
    })();
  }, [deviceId, native, status.active, poll]);

  const play = useCallback(async (track: LibraryTrack) => {
    if (!native) { setError('Playback runs in the Windows app.'); return; }
    setBusy(true);
    setError('');
    autoAdvanced.current = '';
    try {
      await invoke('play_track', {
        path: track.path, deviceId, title: track.title, artist: track.artist, album: track.album,
      });
      await poll();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }, [native, deviceId, poll]);

  const indexOf = useCallback((path: string) => queue.findIndex(item => item.path === path), [queue]);

  // Shuffle draws from the whole queue but never from the track already open, so one press
  // always changes the song.
  const otherThan = useCallback((from: number) => {
    if (queue.length < 2) return Math.max(0, from);
    let index = from;
    while (index === from) index = Math.floor(Math.random() * queue.length);
    return index;
  }, [queue.length]);

  const step = useCallback(async (delta: number) => {
    if (!queue.length) return;
    const current = indexOf(status.path);
    if (shuffle) {
      // A single-track queue has nowhere to draw from, so the press is a no-op.
      if (queue.length > 1) await play(queue[otherThan(Math.max(0, current))]);
      return;
    }
    const at = current < 0 ? 0 : current + delta;
    // Passing either end only continues into the other one when the queue repeats.
    if (at < 0 || at >= queue.length) {
      if (repeat === 'all') await play(queue[(at + queue.length) % queue.length]);
      return;
    }
    await play(queue[at]);
  }, [indexOf, otherThan, play, queue, repeat, shuffle, status.path]);

  const next = useCallback(() => step(1), [step]);
  const previous = useCallback(() => step(-1), [step]);

  const toggle = useCallback(async () => {
    if (!native) { setError('Playback runs in the Windows app.'); return; }
    try {
      if (status.playing) await invoke('pause_playback');
      else if (status.active) await invoke('resume_playback');
      else if (queue.length) await play(queue[Math.max(0, indexOf(status.path))]);
      await poll();
    } catch (reason) { setError(String(reason)); }
  }, [native, status.playing, status.active, status.path, queue, indexOf, play, poll]);

  const seek = useCallback(async (seconds: number) => {
    setStatus(current => ({ ...current, position: seconds }));
    try { await invoke('seek_playback', { position: seconds }); }
    catch (reason) { setError(String(reason)); }
  }, []);

  const setVolume = useCallback(async (volume: number) => {
    settlingVolume.current = volume;
    writeLevel(localLevelStorage(), volume);
    setStatus(current => ({ ...current, volume }));
    try { await invoke('set_playback_volume', { volume }); }
    catch (reason) { settlingVolume.current = null; setError(String(reason)); }
  }, []);

  const stop = useCallback(async () => {
    try { await invoke('stop_playback'); await poll(); }
    catch (reason) { setError(String(reason)); }
  }, [poll]);

  // When a track ends on its own, continue with the next one in the queue, or follow the mode.
  useEffect(() => {
    if (!status.finished || status.active || !status.path) return;
    if (autoAdvanced.current === status.path) return;
    autoAdvanced.current = status.path;
    const at = indexOf(status.path);
    if (at < 0) return;
    if (repeat === 'one') { void play(queue[at]); return; }
    if (shuffle) { if (queue.length > 1) void play(queue[otherThan(at)]); return; }
    if (at + 1 < queue.length) void play(queue[at + 1]);
    else if (repeat === 'all') void play(queue[0]);
  }, [status.finished, status.active, status.path, indexOf, queue, play, otherThan, repeat, shuffle]);

  const toggleShuffle = useCallback(() => setShuffle(on => !on), []);
  // One button walks off → whole queue → one track → off, so the mode is always one press away.
  const cycleRepeat = useCallback(() => setRepeat(mode => mode === 'off' ? 'all' : mode === 'all' ? 'one' : 'off'), []);

  return {
    native, status, busy, hasQueue: queue.length > 0,
    error: error || status.error || '',
    shuffle, repeat, toggleShuffle, cycleRepeat,
    play, toggle, next, previous, seek, setVolume, stop,
  };
}
