/**
 * The spatial field — mode, width, crossfeed, height and rotation — remembered
 * between launches, so reopening the app does not silently put the renderer back
 * to its factory setting.
 *
 * Anything that does not validate falls back per field rather than throwing the
 * whole field away: a build that drops a mode must still be able to read the rest.
 */
import { defaultSpatial, spatialModes, type SpatialSettings } from './model';

export const SPATIAL_KEY = 'audiorium.spatial';

interface Readable { getItem(key: string): string | null }
interface Writable { setItem(key: string, value: string): void }

const MODES = spatialModes.map(mode => mode.id);

/** A share of full scale: a finite number inside 0…1, or the fallback. */
function unit(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 && value <= 1 ? value : fallback;
}

/** Degrees around the listener, kept inside the half turn the pad offers. */
function degrees(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) && Math.abs(value) <= 180 ? value : fallback;
}

/** One saved field, with every part that no longer validates replaced. */
export function normalizeSpatial(value: Partial<SpatialSettings> | null | undefined): SpatialSettings {
  const mode = MODES.includes(value?.mode as SpatialSettings['mode']) && value?.mode
    ? value.mode
    : defaultSpatial.mode;
  return {
    mode,
    width: unit(value?.width, defaultSpatial.width),
    crossfeed: unit(value?.crossfeed, defaultSpatial.crossfeed),
    height: unit(value?.height, defaultSpatial.height),
    rotation: degrees(value?.rotation, defaultSpatial.rotation),
  };
}

/** The remembered field, or the factory one when nothing usable was saved. */
export function readSpatial(storage: Readable | null | undefined): SpatialSettings {
  try {
    const text = storage?.getItem(SPATIAL_KEY);
    if (!text) return defaultSpatial;
    return normalizeSpatial(JSON.parse(text) as Partial<SpatialSettings>);
  } catch {
    // Unparsable JSON is no field at all, not a reason to fail a launch.
    return defaultSpatial;
  }
}

export function writeSpatial(storage: Writable | null | undefined, spatial: SpatialSettings): void {
  try {
    storage?.setItem(SPATIAL_KEY, JSON.stringify(normalizeSpatial(spatial)));
  } catch {
    // Storage can be blocked or full; the session keeps the field either way.
  }
}

/** The WebView's own storage, or null when it is unavailable. */
export function localSpatialStorage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}
