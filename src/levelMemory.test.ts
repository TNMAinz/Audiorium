import { describe, expect, it } from 'vitest';
import { DEFAULT_LEVEL, LEVEL_KEY, readLevel, writeLevel } from './levelMemory';

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

describe('remembered playback level', () => {
  it('starts at the shipped level when nothing was chosen', () => {
    expect(readLevel(memory())).toBe(DEFAULT_LEVEL);
    expect(readLevel(null)).toBe(DEFAULT_LEVEL);
    expect(readLevel(undefined)).toBe(DEFAULT_LEVEL);
  });
  it('round-trips the level a drag left it at', () => {
    const storage = memory();
    writeLevel(storage, 0.42);
    expect(storage.read(LEVEL_KEY)).toBe('0.42');
    expect(readLevel(storage)).toBeCloseTo(0.42, 6);
  });
  it('keeps silence and full scale as real choices', () => {
    const storage = memory();
    writeLevel(storage, 0);
    expect(readLevel(storage)).toBe(0);
    writeLevel(storage, 1);
    expect(readLevel(storage)).toBe(1);
  });
  it('ignores a value that is not a level, rather than muting the app', () => {
    expect(readLevel(memory({ [LEVEL_KEY]: 'loud' }))).toBe(DEFAULT_LEVEL);
    expect(readLevel(memory({ [LEVEL_KEY]: '2' }))).toBe(DEFAULT_LEVEL);
    expect(readLevel(memory({ [LEVEL_KEY]: '-0.5' }))).toBe(DEFAULT_LEVEL);
    expect(readLevel(memory({ [LEVEL_KEY]: '   ' }))).toBe(DEFAULT_LEVEL);
    // A cut-short write that is not a number is the default too.
    expect(readLevel(memory({ [LEVEL_KEY]: '0.4.2' }))).toBe(DEFAULT_LEVEL);
  });
  it('refuses to store something that is not a level', () => {
    const storage = memory();
    writeLevel(storage, Number.NaN);
    writeLevel(storage, 1.4);
    expect(storage.read(LEVEL_KEY)).toBeNull();
  });
  it('survives a storage that throws on read or write', () => {
    expect(readLevel(locked)).toBe(DEFAULT_LEVEL);
    expect(() => writeLevel(locked, 0.5)).not.toThrow();
    expect(() => writeLevel(null, 0.5)).not.toThrow();
  });
});
