import { invoke, isTauri } from '@tauri-apps/api/core';
import { useEffect, useMemo, useState } from 'react';

/**
 * Track lengths for the shelf's hover labels. The scanner deliberately does not read
 * them, so they are probed on demand for the sleeves actually mounted, one request per
 * new path, and answered files are remembered for the session.
 */
export function useDurations(paths: string[]) {
  const [, refresh] = useState(0);
  const store = useMemo(() => ({ known: new Map<string, number>(), asked: new Set<string>(), alive: true }), []);

  useEffect(() => {
    store.alive = true;
    return () => { store.alive = false; };
  }, [store]);

  useEffect(() => {
    const missing = paths.filter(path => !store.known.has(path) && !store.asked.has(path));
    if (!missing.length || !isTauri()) return;
    for (const path of missing) store.asked.add(path);
    void invoke<(number | null)[]>('track_durations', { paths: missing })
      .then(durations => {
        if (!store.alive) return;
        missing.forEach((path, at) => {
          const seconds = durations[at];
          if (typeof seconds === 'number' && Number.isFinite(seconds) && seconds > 0) store.known.set(path, seconds);
        });
        refresh(value => value + 1);
      })
      .catch(() => {
        if (store.alive) refresh(value => value + 1);
      });
  }, [paths, store]);

  return store.known;
}
