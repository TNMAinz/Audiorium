/**
 * Geometry and value mapping for the instrument controls. Kept free of React so
 * the maths can be tested without a DOM, and so a knob, a pad and a meter all
 * map the same value to the same place.
 */

/** A rotary control sweeps 270°, from lower-left to lower-right, like hardware. */
export const KNOB_START = -135;
export const KNOB_SWEEP = 270;

export const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

export function quantize(value: number, min: number, max: number, step: number) {
  if (!Number.isFinite(value) || step <= 0) return clamp(value, min, max);
  const snapped = Math.round((value - min) / step) * step + min;
  // Steps such as 0.01 accumulate float error; six decimals is below any audio use.
  return clamp(Number(snapped.toFixed(6)), min, max);
}

/** Fraction of the sweep, 0 at the minimum and 1 at the maximum. */
export function knobFraction(value: number, min: number, max: number) {
  if (max === min) return 0;
  return clamp((value - min) / (max - min), 0, 1);
}

export function knobAngle(value: number, min: number, max: number) {
  return KNOB_START + KNOB_SWEEP * knobFraction(value, min, max);
}

export function polar(cx: number, cy: number, radius: number, angleDeg: number) {
  const radians = ((angleDeg - 90) * Math.PI) / 180;
  return { x: cx + radius * Math.cos(radians), y: cy + radius * Math.sin(radians) };
}

/** SVG arc path from one angle to another, clockwise, in screen coordinates. */
export function arcPath(cx: number, cy: number, radius: number, fromDeg: number, toDeg: number) {
  const start = polar(cx, cy, radius, fromDeg);
  const end = polar(cx, cy, radius, toDeg);
  const large = Math.abs(toDeg - fromDeg) > 180 ? 1 : 0;
  const sweep = toDeg >= fromDeg ? 1 : 0;
  return `M ${start.x.toFixed(2)} ${start.y.toFixed(2)} A ${radius} ${radius} 0 ${large} ${sweep} ${end.x.toFixed(2)} ${end.y.toFixed(2)}`;
}

export interface DragInput {
  start: number;
  dy: number;
  min: number;
  max: number;
  step: number;
  /** Pixels for the full range; a taller gesture is finer. */
  pixels?: number;
  fine?: boolean;
}

/**
 * Vertical drag: up increases. The whole range travels in `pixels` (180 by
 * default), and holding the fine modifier takes a fifth of that, which is how
 * hardware-style controls behave when you need one dB.
 */
export function dragToValue({ start, dy, min, max, step, pixels = 180, fine = false }: DragInput) {
  const span = max - min;
  const scale = (fine ? 5 : 1) * pixels;
  return quantize(start + (-dy / scale) * span, min, max, step);
}

export function stepValue(value: number, direction: 1 | -1, step: number, min: number, max: number, multiplier = 1) {
  return quantize(value + direction * step * multiplier, min, max, step);
}

export interface PadGeometry {
  /** Half the width of the pad in the same units as x and y. */
  radius: number;
  /** Azimuth at the pad's left and right edges. */
  span?: number;
}

/**
 * The pad is a top view of the head: x is azimuth (left negative, right
 * positive), y is elevation (up positive). The pad edge is ±span azimuth, so
 * 180 reaches behind the listener at the far left and right edges.
 */
export function padToValue(x: number, y: number, geometry: PadGeometry) {
  const span = geometry.span ?? 180;
  const radius = Math.max(1, geometry.radius);
  const azimuth = clamp((x / radius) * span, -180, 180);
  const elevation = clamp((-y / radius) * 90, -90, 90);
  return { azimuth, elevation };
}

export function valueToPad(azimuth: number, elevation: number, geometry: PadGeometry) {
  const span = geometry.span ?? 180;
  const radius = Math.max(1, geometry.radius);
  return {
    x: clamp((azimuth / span) * radius, -radius, radius),
    y: clamp((-elevation / 90) * radius, -radius, radius),
  };
}

/** Early-reflection taps the panner draws: 7 to 23 ms, scaled by the length control. */
export function reflectionTaps(length: number, level: number) {
  const base = [7, 11.5, 17, 23];
  const scale = 0.35 + length * 0.65;
  return base.map((ms, index) => ({
    ms: Number((ms * scale).toFixed(2)),
    level: Number((level * (1 - index * 0.18)).toFixed(3)),
  }));
}

/** A meter scale label at a fraction of the width, for a dB range. */
/**
 * Keeps only the ticks far enough apart to carry a legible label; the rest are
 * still drawn, just unlabelled, so a crowded scale never overlaps itself.
 * The outermost tick wins when two would collide.
 */
export function spacedLabels<T extends { fraction: number }>(ticks: T[], minGap = 0.09): T[] {
  const sorted = [...ticks].sort((a, b) => a.fraction - b.fraction);
  const kept: T[] = [];
  for (const tick of sorted) {
    const last = kept[kept.length - 1];
    if (last && tick.fraction - last.fraction < minGap) {
      if (tick.fraction === 1) kept[kept.length - 1] = tick;
      continue;
    }
    kept.push(tick);
  }
  return kept;
}

export function dbTicks(maxDb: number, minDb: number, ticks: number[]) {
  return ticks.map(db => ({ db, fraction: clamp((db - minDb) / (maxDb - minDb), 0, 1) }));
}
