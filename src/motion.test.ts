import { describe, expect, it } from 'vitest';
import { MOTION_KEY, readReducedMotion, writeReducedMotion } from './motion';

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

describe('reduced motion preference', () => {
  it('starts quiet when nothing has been chosen yet', () => {
    expect(readReducedMotion(memory())).toBe(true);
    expect(readReducedMotion(null)).toBe(true);
    expect(readReducedMotion(undefined)).toBe(true);
  });
  it('remembers a user who asked for motion', () => {
    expect(readReducedMotion(memory({ [MOTION_KEY]: 'off' }))).toBe(false);
  });
  it('remembers a user who asked for stillness', () => {
    expect(readReducedMotion(memory({ [MOTION_KEY]: 'on' }))).toBe(true);
  });
  it('treats an unreadable value as quiet instead of guessing', () => {
    expect(readReducedMotion(memory({ [MOTION_KEY]: 'maybe' }))).toBe(true);
  });
  it('survives a storage that throws on read or write', () => {
    expect(readReducedMotion(locked)).toBe(true);
    expect(() => writeReducedMotion(locked, false)).not.toThrow();
    expect(() => writeReducedMotion(null, true)).not.toThrow();
  });
  it('round-trips the choice through storage', () => {
    const storage = memory();
    writeReducedMotion(storage, false);
    expect(storage.read(MOTION_KEY)).toBe('off');
    expect(readReducedMotion(storage)).toBe(false);
    writeReducedMotion(storage, true);
    expect(readReducedMotion(storage)).toBe(true);
  });
});
