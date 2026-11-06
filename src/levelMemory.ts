/**
 * The playback level, remembered between launches.
 *
 * The level is a setting rather than a reading — it is applied before the
 * processors — so a launch that forgot it would play at a level the user never
 * chose. A value that is not a number between 0 and 1 (written by another build, or
 * cut short by a shutdown) falls back to the level the app ships with, so a broken
 * memory is a default rather than a muted app or a stream past full scale.
 */
export const LEVEL_KEY = 'audiorium.playback-level';
/** What the app plays at until a level is chosen: 0.7, about −3.1 dB. */
export const DEFAULT_LEVEL = 0.7;

interface Readable { getItem(key: string): string | null }
interface Writable {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

function valid(level: number): boolean {
  return Number.isFinite(level) && level >= 0 && level <= 1;
}

/** The remembered level, or the one the app ships with. Silence is a real level. */
export function readLevel(storage: Readable | null | undefined, fallback = DEFAULT_LEVEL): number {
  try {
    const text = (storage?.getItem(LEVEL_KEY) ?? '').trim();
    if (!text) return fallback;
    const stored = Number(text);
    if (valid(stored)) return stored;
  } catch {
    // A locked-down WebView can throw on access; the shipped level is the safe reading.
  }
  return fallback;
}

/** A level outside 0…1 is not a level: it is dropped rather than stored. */
export function writeLevel(storage: Writable | null | undefined, level: number): void {
  try {
    if (valid(level)) storage?.setItem(LEVEL_KEY, String(level));
  } catch {
    // Storage can be blocked or full; the session keeps working either way.
  }
}

/** The WebView's own storage, or null when it is unavailable. */
export function localLevelStorage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}
