/** The scene is quiet by default; motion is an explicit choice, remembered between launches. */
export const MOTION_KEY = 'audiorium.motion';

interface Readable { getItem(key: string): string | null }
interface Writable { setItem(key: string, value: string): void }

/** Anything other than a remembered "off" leaves the scene still. */
export function readReducedMotion(storage: Readable | null | undefined): boolean {
  try {
    const stored = storage?.getItem(MOTION_KEY);
    if (stored === 'off') return false;
    if (stored === 'on') return true;
  } catch {
    // A locked-down WebView can throw on access; quiet is the safe reading.
  }
  return true;
}

export function writeReducedMotion(storage: Writable | null | undefined, reduced: boolean): void {
  try {
    storage?.setItem(MOTION_KEY, reduced ? 'on' : 'off');
  } catch {
    // Storage can be blocked or full; the session keeps working either way.
  }
}

/** The WebView's own storage, or null when it is unavailable. */
export function localMotionStorage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}
