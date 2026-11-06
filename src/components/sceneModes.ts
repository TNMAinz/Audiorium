/**
 * The observatory's geometry, kept pure so the shapes can be checked without a
 * WebGL context: `OrbitScene` turns these arrays into buffers, it does not compute
 * them. There is one reading — the acoustic core — so nothing here describes a mode.
 */

const PHI = (1 + Math.sqrt(5)) / 2;

/** An icosahedron's twelve corners, normalised, before any subdivision. */
const CORNERS: [number, number, number][] = [
  [-1, PHI, 0], [1, PHI, 0], [-1, -PHI, 0], [1, -PHI, 0],
  [0, -1, PHI], [0, 1, PHI], [0, -1, -PHI], [0, 1, -PHI],
  [PHI, 0, -1], [PHI, 0, 1], [-PHI, 0, -1], [-PHI, 0, 1],
].map(([x, y, z]) => {
  const length = Math.hypot(x, y, z);
  return [x / length, y / length, z / length];
});

const FACES: [number, number, number][] = [
  [0, 11, 5], [0, 5, 1], [0, 1, 7], [0, 7, 10], [0, 10, 11],
  [1, 5, 9], [5, 11, 4], [11, 10, 2], [10, 7, 6], [7, 1, 8],
  [3, 9, 4], [3, 4, 2], [3, 2, 6], [3, 6, 8], [3, 8, 9],
  [4, 9, 5], [2, 4, 11], [6, 2, 10], [8, 6, 7], [9, 8, 1],
];

export interface Shell {
  /** Vertex positions, three floats each, on a sphere of `radius`. */
  positions: Float32Array;
  /** Line segments as pairs of vertex indices, deduplicated. */
  edges: Uint32Array;
}

/**
 * A geodesic shell: the icosahedron subdivided `detail` times and pushed back onto
 * its sphere. Detail 2 is 162 vertices and 480 edges — a structure that reads as a
 * framework rather than a ball of dots, and still costs one draw call per buffer.
 */
export function geodesicShell(radius: number, detail: number): Shell {
  const points: [number, number, number][] = CORNERS.map(corner => [...corner]);
  let faces: [number, number, number][] = FACES.map(face => [...face]);
  const index = new Map<string, number>();
  const key = ([x, y, z]: [number, number, number]) => `${x.toFixed(5)},${y.toFixed(5)},${z.toFixed(5)}`;
  points.forEach((point, at) => index.set(key(point), at));

  const midpoint = (a: number, b: number) => {
    const mixed: [number, number, number] = [
      (points[a][0] + points[b][0]) / 2,
      (points[a][1] + points[b][1]) / 2,
      (points[a][2] + points[b][2]) / 2,
    ];
    const length = Math.hypot(...mixed);
    const unit: [number, number, number] = [mixed[0] / length, mixed[1] / length, mixed[2] / length];
    const found = index.get(key(unit));
    if (found !== undefined) return found;
    points.push(unit);
    index.set(key(unit), points.length - 1);
    return points.length - 1;
  };

  for (let step = 0; step < Math.max(0, Math.min(3, Math.floor(detail))); step++) {
    const next: [number, number, number][] = [];
    for (const [a, b, c] of faces) {
      const ab = midpoint(a, b);
      const bc = midpoint(b, c);
      const ca = midpoint(c, a);
      next.push([a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]);
    }
    faces = next;
  }

  const positions = new Float32Array(points.length * 3);
  points.forEach(([x, y, z], at) => positions.set([x * radius, y * radius, z * radius], at * 3));

  const seen = new Set<string>();
  const segments: number[] = [];
  for (const [a, b, c] of faces) {
    for (const [from, to] of [[a, b], [b, c], [c, a]] as const) {
      const id = from < to ? `${from}:${to}` : `${to}:${from}`;
      if (seen.has(id)) continue;
      seen.add(id);
      segments.push(from, to);
    }
  }

  return { positions, edges: new Uint32Array(segments) };
}

/**
 * The average of the spectrum bins inside one octave band, from 30 Hz to 16 kHz.
 * A band with nothing in it is 0: the scene shows the measurement, never a floor.
 */
export function bandEnergy(spectrum: number[], band: number): number {
  if (!spectrum.length) return 0;
  const share = Math.min(1, Math.max(0, band));
  // A band always reads at least one bin, so a one-bin spectrum cannot divide by nothing.
  const from = Math.min(spectrum.length - 1, Math.floor(share * spectrum.length));
  const to = Math.min(spectrum.length, Math.max(from + 1, Math.round((share + 0.125) * spectrum.length)));
  let total = 0;
  for (let at = from; at < to; at++) total += spectrum[at];
  return total / (to - from);
}

/**
 * The spectrum as one row of bytes for the orb's displacement texture: one byte per
 * band, 0..255, clamped. A band that is missing or not a number writes 0, so a stopped
 * engine leaves the orb round instead of inventing a floor.
 */
export function bandBytes(spectrum: readonly number[], into: Uint8Array): Uint8Array {
  for (let at = 0; at < into.length; at++) {
    const energy = spectrum[at];
    into[at] = Number.isFinite(energy) ? Math.round(Math.min(1, Math.max(0, energy)) * 255) : 0;
  }
  return into;
}
