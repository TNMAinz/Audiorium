/** The folder the library was last scanned from, remembered between launches. */
export const LIBRARY_KEY = 'audiorium.library-root';

interface Readable { getItem(key: string): string | null }
interface Writable {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

/** An empty or unreadable value means no folder was remembered, never a guess. */
export function readLibraryRoot(storage: Readable | null | undefined): string {
  try {
    return (storage?.getItem(LIBRARY_KEY) ?? '').trim();
  } catch {
    // A locked-down WebView can throw on access; no folder is the safe reading.
    return '';
  }
}

export function writeLibraryRoot(storage: Writable | null | undefined, root: string): void {
  try {
    const value = root.trim();
    if (value) storage?.setItem(LIBRARY_KEY, value);
    else storage?.removeItem?.(LIBRARY_KEY);
  } catch {
    // Storage can be blocked or full; the session keeps working either way.
  }
}

/** The WebView's own storage, or null when it is unavailable. */
export function localLibraryStorage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}
