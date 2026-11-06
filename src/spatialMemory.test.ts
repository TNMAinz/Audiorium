import { describe, expect, it } from 'vitest';
import { defaultSpatial } from './model';
import { SPATIAL_KEY, normalizeSpatial, readSpatial, writeSpatial } from './spatialMemory';

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

describe('the remembered spatial field', () => {
  it('starts on the factory field when nothing was saved', () => {
    expect(readSpatial(memory())).toEqual(defaultSpatial);
    expect(readSpatial(null)).toEqual(defaultSpatial);
    expect(readSpatial(locked)).toEqual(defaultSpatial);
  });

  it('comes back the way it was left', () => {
    const storage = memory();
    const field = { mode: 'binaural' as const, width: 0.2, crossfeed: 0.9, height: 0.75, rotation: -33 };
    writeSpatial(storage, field);
    expect(readSpatial(storage)).toEqual(field);
    // And it is a plain saved value, not something the page has to re-derive.
    expect(JSON.parse(storage.read(SPATIAL_KEY)!)).toMatchObject({ mode: 'binaural', rotation: -33 });
  });

  it('falls back per field rather than discarding the whole choice', () => {
    const partly = normalizeSpatial({ mode: 'surround71', width: 4, crossfeed: 0.5, height: Number.NaN });
    expect(partly).toEqual({
      mode: 'surround71',
      width: defaultSpatial.width,
      crossfeed: 0.5,
      height: defaultSpatial.height,
      rotation: defaultSpatial.rotation,
    });
  });

  it('rejects a mode this build no longer offers, and keeps the rest', () => {
    const saved = readSpatial(memory({
      [SPATIAL_KEY]: JSON.stringify({ mode: 'tetrahedral', width: 0.3, crossfeed: 0.1, height: 0.2, rotation: 15 }),
    }));
    expect(saved.mode).toBe(defaultSpatial.mode);
    expect(saved.width).toBe(0.3);
    expect(saved.rotation).toBe(15);
  });

  it('treats unparsable or hostile storage as no field at all', () => {
    expect(readSpatial(memory({ [SPATIAL_KEY]: 'not json' }))).toEqual(defaultSpatial);
    expect(readSpatial(memory({ [SPATIAL_KEY]: 'null' }))).toEqual(defaultSpatial);
    expect(readSpatial(memory({ [SPATIAL_KEY]: '{"rotation": 1e9, "width": -1}' }))).toEqual(defaultSpatial);
    expect(() => writeSpatial(locked, defaultSpatial)).not.toThrow();
  });
});
