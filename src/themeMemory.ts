/**
 * The colour theme, remembered between launches.
 *
 * An unknown or unreadable value falls back to the theme the app starts in, so a
 * value written by another build can never leave the window without a palette.
 */
import type { Theme } from './model';

export const THEME_KEY = 'audiorium.theme';
export const THEMES: Theme[] = ['Ion', 'Violet', 'Ember'];

interface Readable { getItem(key: string): string | null }
interface Writable { setItem(key: string, value: string): void }

/** The remembered theme, or the one the app ships with. */
export function readTheme(storage: Readable | null | undefined, fallback: Theme = 'Ion'): Theme {
  try {
    const stored = (storage?.getItem(THEME_KEY) ?? '').trim();
    if ((THEMES as string[]).includes(stored)) return stored as Theme;
  } catch {
    // A locked-down WebView can throw on access; the default palette is the safe reading.
  }
  return fallback;
}

export function writeTheme(storage: Writable | null | undefined, theme: Theme): void {
  try {
    if ((THEMES as string[]).includes(theme)) storage?.setItem(THEME_KEY, theme);
  } catch {
    // Storage can be blocked or full; the session keeps its palette either way.
  }
}

/** The WebView's own storage, or null when it is unavailable. */
export function localThemeStorage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}
