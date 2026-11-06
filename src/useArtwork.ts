import { invoke, isTauri } from '@tauri-apps/api/core';
import { useEffect, useMemo, useState } from 'react';
import { ART_CACHE_LIMIT } from './components/globeLibrary';

export interface Artwork { url: string; state: 'ready' | 'missing' | 'error' }

/** DOM images own the decoded thumbnails; no unbounded Three.js texture cache. */
export function useArtwork(root: string, paths: string[]) {
  const [, refresh] = useState(0);
  const loader = useMemo(() => ({
    root: '', epoch: 0, active: 0, alive: true,
    wanted: new Set<string>(), queue: [] as string[], inFlight: new Set<string>(),
    cache: new Map<string, Artwork>(),
  }), []);

  useEffect(() => {
    loader.alive = true;
    return () => {
      loader.alive = false; loader.epoch++; loader.queue = [];
      for (const entry of loader.cache.values()) if (entry.url) URL.revokeObjectURL(entry.url);
      loader.cache.clear();
    };
  }, [loader]);

  useEffect(() => {
    if (loader.root !== root) {
      loader.root = root; loader.epoch++;
      for (const entry of loader.cache.values()) if (entry.url) URL.revokeObjectURL(entry.url);
      loader.cache.clear();
    }
    loader.wanted = new Set(paths);
    // Touch visible entries so eviction never removes a cover still on the globe.
    for (const path of paths) {
      const entry = loader.cache.get(path);
      if (entry) { loader.cache.delete(path); loader.cache.set(path, entry); }
    }
    loader.queue = paths.filter(path => !loader.cache.has(path) && !loader.inFlight.has(`${loader.epoch}:${path}`));
    if (!root || !isTauri()) { loader.queue = []; return; }
    const pump = () => {
      while (loader.alive && loader.active < 3 && loader.queue.length) {
        const path = loader.queue.shift()!;
        const epoch = loader.epoch;
        const key = `${epoch}:${path}`;
        loader.inFlight.add(key); loader.active++;
        void invoke<ArrayBuffer | number[]>('track_artwork', { root: loader.root, path })
          .then(raw => {
            if (!loader.alive || epoch !== loader.epoch || !loader.wanted.has(path)) return;
            const bytes = raw instanceof ArrayBuffer ? raw : new Uint8Array(raw).buffer;
            const entry: Artwork = bytes.byteLength
              ? { url: URL.createObjectURL(new Blob([bytes], { type: 'image/png' })), state: 'ready' }
              : { url: '', state: 'missing' };
            loader.cache.set(path, entry);
          })
          .catch(() => {
            if (loader.alive && epoch === loader.epoch && loader.wanted.has(path)) {
              loader.cache.set(path, { url: '', state: 'error' });
            }
          })
          .finally(() => {
            loader.active--; loader.inFlight.delete(key);
            for (const [old, entry] of loader.cache) {
              if (loader.cache.size <= ART_CACHE_LIMIT) break;
              if (loader.wanted.has(old)) continue;
              if (entry.url) URL.revokeObjectURL(entry.url);
              loader.cache.delete(old);
            }
            if (loader.alive) { refresh(value => value + 1); pump(); }
          });
      }
    };
    pump();
  }, [root, paths, loader]);

  return { covers: loader.cache, cached: loader.cache.size, pending: loader.active };
}
