import { describe, expect, it } from 'vitest';
import { LIBRARY_KEY, readLibraryRoot, writeLibraryRoot } from './libraryMemory';

function memory(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value); },
    removeItem: (key: string) => { values.delete(key); },
    read: (key: string) => values.get(key) ?? null,
  };
}

const locked = {
  getItem: () => { throw new Error('storage blocked'); },
  setItem: () => { throw new Error('quota exceeded'); },
  removeItem: () => { throw new Error('storage blocked'); },
};

describe('remembered library folder', () => {
  it('starts with no folder when nothing was scanned', () => {
    expect(readLibraryRoot(memory())).toBe('');
    expect(readLibraryRoot(null)).toBe('');
    expect(readLibraryRoot(undefined)).toBe('');
  });
  it('returns the folder that last scanned', () => {
    expect(readLibraryRoot(memory({ [LIBRARY_KEY]: 'D:\\Music' }))).toBe('D:\\Music');
  });
  it('trims a padded value instead of passing it to the scanner', () => {
    expect(readLibraryRoot(memory({ [LIBRARY_KEY]: '  D:\\Music  ' }))).toBe('D:\\Music');
  });
  it('survives a storage that throws on read or write', () => {
    expect(readLibraryRoot(locked)).toBe('');
    expect(() => writeLibraryRoot(locked, 'D:\\Music')).not.toThrow();
    expect(() => writeLibraryRoot(null, 'D:\\Music')).not.toThrow();
  });
  it('round-trips the folder and forgets it when cleared', () => {
    const storage = memory();
    writeLibraryRoot(storage, 'D:\\Music');
    expect(storage.read(LIBRARY_KEY)).toBe('D:\\Music');
    expect(readLibraryRoot(storage)).toBe('D:\\Music');
    writeLibraryRoot(storage, '  ');
    expect(storage.read(LIBRARY_KEY)).toBeNull();
    expect(readLibraryRoot(storage)).toBe('');
  });
});
