import { describe, expect, it } from 'vitest';
import { bandBytes, bandEnergy, geodesicShell } from './sceneModes';

describe('observatory scene model', () => {
  it('places every geodesic vertex on its shell and every edge between two of them', () => {
    for (const detail of [0, 1, 2]) {
      const { positions, edges } = geodesicShell(2.05, detail);
      const count = positions.length / 3;
      expect(count).toBeGreaterThan(11);
      for (let at = 0; at < positions.length; at += 3) {
        const radius = Math.hypot(positions[at], positions[at + 1], positions[at + 2]);
        expect(radius).toBeCloseTo(2.05, 4);
      }
      expect(edges.length % 2).toBe(0);
      expect(edges.length).toBeGreaterThan(0);
      const seen = new Set<string>();
      for (let at = 0; at < edges.length; at += 2) {
        const [from, to] = [edges[at], edges[at + 1]];
        expect(from).toBeLessThan(count);
        expect(to).toBeLessThan(count);
        expect(from).not.toBe(to);
        // An edge appears once: drawing it twice doubles its brightness in the scene.
        const id = from < to ? `${from}:${to}` : `${to}:${from}`;
        expect(seen.has(id)).toBe(false);
        seen.add(id);
      }
    }
  });

  it('adds structure as the detail rises', () => {
    const coarse = geodesicShell(1, 0);
    const fine = geodesicShell(1, 2);
    expect(fine.positions.length).toBeGreaterThan(coarse.positions.length * 8);
    expect(fine.edges.length).toBeGreaterThan(coarse.edges.length * 8);
  });

  it('reads one band of the spectrum without inventing a level', () => {
    expect(bandEnergy([], 0.5)).toBe(0);
    expect(bandEnergy([0, 0, 0, 0], 0.25)).toBe(0);
    const low = [0.8, 0.8, 0, 0, 0, 0, 0, 0];
    expect(bandEnergy(low, 0)).toBeCloseTo(0.8, 6);
    expect(bandEnergy(low, 0.9)).toBe(0);
    // A band always reads at least one bin, so a short spectrum cannot divide by nothing.
    expect(Number.isFinite(bandEnergy([0.5], 1))).toBe(true);
  });
});

describe("the wavy orb's band row", () => {
  it('writes one byte per band and never a level the engine did not measure', () => {
    const row = bandBytes([], new Uint8Array(4));
    expect([...row]).toEqual([0, 0, 0, 0]);
    const mixed = bandBytes([0, 0.5, 1, 2], new Uint8Array(4));
    expect([...mixed]).toEqual([0, 128, 255, 255]);
    const broken = bandBytes([Number.NaN, Number.POSITIVE_INFINITY, -1, 0.25], new Uint8Array(4));
    expect([...broken]).toEqual([0, 0, 0, 64]);
  });
  it('fills exactly the row it is given, so the texture keeps its size', () => {
    const row = new Uint8Array(56).fill(9);
    expect(bandBytes([0.2, 0.4], row)).toHaveLength(56);
    expect(row[0]).toBe(51);
    expect(row[55]).toBe(0);
  });
});
