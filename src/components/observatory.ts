/**
 * The readings the Observatory is drawn from, kept pure so every number can be
 * checked without WebGL.
 *
 * The scene's shapes are decoration unless each one reports something, so the
 * conversions the drawing depends on — a level as dBFS, a clock, a band's frequency,
 * the track's place on its orbit, what is running — live here and are tested alone.
 * Nothing here invents a value: an idle engine has a floor, not a reading.
 */
import { formatDb, type ChainMeters } from '../model';

/** The floor the level meter uses, so silence has a number instead of −Infinity. */
export const LEVEL_FLOOR_DB = -60;

/** A 0..1 magnitude as dBFS, floored where the meters floor it. */
export function levelDb(magnitude: number): number {
  if (!Number.isFinite(magnitude) || magnitude <= 0) return LEVEL_FLOOR_DB;
  return Math.max(LEVEL_FLOOR_DB, Math.min(0, 20 * Math.log10(magnitude)));
}

/** "−18.4 dBFS": one unit at every level. */
export function formatLevel(magnitude: number): string {
  return `${formatDb(levelDb(magnitude))}FS`;
}

/** "1:04": minutes and seconds, as the transport shows them. */
export function formatClock(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return '0:00';
  const whole = Math.floor(seconds);
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, '0')}`;
}

/** The engine folds the spectrum into 56 logarithmic bands; these are its edges. */
export const SPECTRUM_BANDS = 56;
export const SPECTRUM_MIN_HZ = 30;
export const SPECTRUM_MAX_HZ = 16_000;

/** The band's own frequency range: a log band, so the centre is the geometric mean. */
export function spectrumHz(band: number, bands = SPECTRUM_BANDS) {
  const ratio = SPECTRUM_MAX_HZ / SPECTRUM_MIN_HZ;
  const at = Math.max(0, Math.min(bands - 1, Math.floor(band)));
  const low = SPECTRUM_MIN_HZ * ratio ** (at / bands);
  const high = SPECTRUM_MIN_HZ * ratio ** ((at + 1) / bands);
  return { low, high, centre: Math.sqrt(low * high) };
}

/** The loudest log band of the last window, or null when nothing was measured. */
export function loudestBand(spectrum: number[]): { band: number; hz: number } | null {
  if (!spectrum.length) return null;
  let band = 0;
  let energy = -Infinity;
  for (let at = 0; at < spectrum.length; at++) {
    const value = spectrum[at];
    if (Number.isFinite(value) && value > energy) { energy = value; band = at; }
  }
  if (!Number.isFinite(energy)) return null;
  return { band, hz: spectrumHz(band, spectrum.length).centre };
}

/** Where the track is on its orbit: 0 at the start, 1 at the end. */
export function trackFraction(position: number, duration: number): number {
  if (!Number.isFinite(position) || !Number.isFinite(duration) || duration <= 0) return 0;
  return Math.max(0, Math.min(1, position / duration));
}

/** How many of the four processors the engine is running right now. */
export function engagedStages(meters: ChainMeters): number {
  return [meters.eqEnabled, meters.pannerEnabled, meters.reverbEnabled, meters.limiterEnabled]
    .filter(Boolean).length;
}
