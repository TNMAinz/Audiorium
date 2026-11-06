import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useCallback, useEffect, useRef, useState } from 'react';
import type { LibraryScan, SpatialSettings, Speaker } from './model';
import { localLibraryStorage, readLibraryRoot, writeLibraryRoot } from './libraryMemory';

/** The virtual speakers the Rust renderer is actually using, not a redrawing of them. */
export function useSpatialLayout(spatial: SpatialSettings) {
  const [layout, setLayout] = useState<Speaker[]>([]);
  const [error, setError] = useState('');
  useEffect(() => {
    if (!isTauri()) { setLayout([]); return; }
    let active = true;
    invoke<Speaker[]>('spatial_layout')
      .then(rows => { if (active) { setLayout(rows); setError(''); } })
      .catch(reason => { if (active) { setLayout([]); setError(String(reason)); } });
    return () => { active = false; };
  }, [spatial.mode, spatial.width, spatial.crossfeed, spatial.height, spatial.rotation]);
  return { layout, layoutError: error };
}

/**
 * Whether the window has been put away to the tray.
 *
 * Nothing on screen is being watched then, so the meters stop asking the engine and
 * the scene stops drawing: an app in the tray should cost what a stopped one costs.
 * Hiding and showing are the app's own commands, so the engine says which happened
 * rather than the interface guessing from focus — clicking another window must not
 * freeze a reading that is still in front of the user.
 */
export function useWindowAway(): boolean {
  const native = isTauri();
  const [away, setAway] = useState(false);
  useEffect(() => {
    if (!native) return;
    let active = true;
    const stops: (() => void)[] = [];
    const subscribe = (event: string, value: boolean) => {
      void listen(event, () => { if (active) setAway(value); })
        .then(stop => { if (active) stops.push(stop); else stop(); })
        .catch(() => { /* a missing listener only costs the saving, never the reading */ });
    };
    subscribe('audiorium:window-hidden', true);
    subscribe('audiorium:window-shown', false);
    return () => { active = false; stops.forEach(stop => stop()); };
  }, [native]);
  return away;
}

/** The level a reading has to reach before it counts as sound rather than the floor. */
export const AUDIBLE = 1e-4;

/**
 * Whether the chosen input has been heard recently.
 *
 * The hold is long enough that a quiet passage in a track does not flash the
 * library invite, and short enough that real silence brings it back. It only ever
 * answers for a running capture: a stopped one is not being listened to.
 */
export function useInputHeard(active: boolean, rms: number, holdMs = 4000): boolean {
  const [heard, setHeard] = useState(false);
  const lastHeard = useRef(0);
  useEffect(() => {
    if (!active) {
      lastHeard.current = 0;
      if (heard) setHeard(false);
      return;
    }
    if (Number.isFinite(rms) && rms > AUDIBLE) lastHeard.current = performance.now();
    const fresh = lastHeard.current > 0 && performance.now() - lastHeard.current < holdMs;
    if (fresh !== heard) setHeard(fresh);
  }, [active, rms, heard, holdMs]);
  // No reading arrives while the input is silent, so the end of the hold is timed
  // here instead of waiting for a poll that would never change.
  useEffect(() => {
    if (!heard) return;
    const timer = window.setInterval(() => {
      if (performance.now() - lastHeard.current >= holdMs) setHeard(false);
    }, 500);
    return () => window.clearInterval(timer);
  }, [heard, holdMs]);
  return heard;
}

export function useLibrary() {
  const native = isTauri();
  const [path, setPath] = useState('');
  const [result, setResult] = useState<LibraryScan | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const restored = useRef(false);
  // A scan that is superseded by Clear or by a newer scan must not bring the library back.
  const generation = useRef(0);

  const scan = useCallback(async (target: string) => {
    if (!native) { setError('Folder scanning runs in the Windows app.'); return; }
    if (!target.trim()) { setError('Choose a music folder first.'); return; }
    const run = ++generation.current;
    setBusy(true); setError('');
    try {
      const scanned = await invoke<LibraryScan>('scan_library', { path: target });
      if (run !== generation.current) return;
      setResult(scanned);
      setPath(scanned.root);
      // The folder that really scanned is the one the next launch opens.
      writeLibraryRoot(localLibraryStorage(), scanned.root);
    }
    catch (reason) {
      if (run !== generation.current) return;
      setError(String(reason));
      setResult(null);
    }
    finally { if (run === generation.current) setBusy(false); }
  }, [native]);

  // Reopen the last folder once per launch, so the library is there without picking it again.
  // A folder that has since moved reports its error instead of silently starting empty.
  useEffect(() => {
    if (restored.current || !native) return;
    restored.current = true;
    const saved = readLibraryRoot(localLibraryStorage());
    if (!saved) return;
    setPath(saved);
    void scan(saved);
  }, [native, scan]);

  const browse = useCallback(async () => {
    if (!native) { setError('The folder picker runs in the Windows app.'); return; }
    try {
      const picked = await invoke<string>('choose_library_folder');
      if (picked) await scan(picked);
    } catch (reason) { setError(String(reason)); }
  }, [native, scan]);

  const clear = useCallback(() => {
    // A scan still in flight is now stale: it must not repopulate what was just cleared.
    generation.current += 1;
    setResult(null); setPath(''); setError(''); setBusy(false);
    // Clearing the library also forgets it: a cleared app opens with nothing loaded.
    writeLibraryRoot(localLibraryStorage(), '');
  }, []);

  return { native, path, setPath, result, busy, error, scan, browse, clear };
}
