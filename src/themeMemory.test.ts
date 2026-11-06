import { describe, expect, it } from 'vitest';
import { THEME_KEY, readTheme, writeTheme } from './themeMemory';

function memory(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value); },
    read: (key: string) => values.get(key) ?? null,
  };
}

const locked = {
  getItem: () => { throw new Error('storage blocked'); },
  setItem: () => { throw new Error('quota exceeded'); },
};

describe('the remembered theme', () => {
  it('starts in the theme the app ships with', () => {
    expect(readTheme(memory())).toBe('Ion');
    expect(readTheme(null)).toBe('Ion');
    expect(readTheme(undefined)).toBe('Ion');
    expect(readTheme(locked)).toBe('Ion');
  });

  it('survives a relaunch once it has been chosen', () => {
    const storage = memory();
    writeTheme(storage, 'Ember');
    expect(storage.read(THEME_KEY)).toBe('Ember');
    expect(readTheme(storage)).toBe('Ember');
    writeTheme(storage, 'Violet');
    expect(readTheme(storage)).toBe('Violet');
  });

  it('ignores a value this build does not have', () => {
    expect(readTheme(memory({ [THEME_KEY]: 'Chartreuse' }))).toBe('Ion');
    expect(readTheme(memory({ [THEME_KEY]: '  ' }))).toBe('Ion');
    // A theme that is not offered is never written, so the stored one stays valid.
    const storage = memory({ [THEME_KEY]: 'Violet' });
    writeTheme(storage, 'Chartreuse' as never);
    expect(readTheme(storage)).toBe('Violet');
  });

  it('never throws on a locked store', () => {
    expect(() => writeTheme(locked, 'Violet')).not.toThrow();
    expect(() => writeTheme(null, 'Violet')).not.toThrow();
  });
});
