/**
 * What the Observatory says, kept pure so every line can be checked without WebGL.
 *
 * The scene's shapes are decoration unless each one reports something. This module
 * turns the readings the app already has into the words the object is labelled with:
 * one part per thing you can see, each with the engine's own numbers and the page
 * that explains it further. Nothing here invents a value: an idle engine says so,
 * a bypassed stage says so, and a held reading says it is held.
 */
import {
  describeEq, formatDb, formatHz, reverbTime,
  type ChainMeters, type ChainSettings, type PlaybackStatus, type Workspace,
} from '../model';
import type { AnalysisStatus } from '../useAnalysis';

export type ScenePartId =
  | 'programme' | 'spectrum' | 'equaliser' | 'panner' | 'room' | 'limiter' | 'track';

/** The four processors, in the order the signal passes them. */
export const stageIds = ['equaliser', 'panner', 'room', 'limiter'] as const;
export type StageId = (typeof stageIds)[number];

export interface ScenePart {
  id: ScenePartId;
  /** The part's name, as the object shows it. */
  label: string;
  /** The live reading, in one line. */
  value: string;
  /** What this part of the object is measuring, for a reader who wants the why. */
  detail: string;
  /** Where the full story lives; every part has one. */
  action: Workspace;
  /** True for a processor the engine is running, false for a bypassed one. */
  engaged: boolean;
}

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

const percent = (value: number) => `${Math.round(Math.max(0, Math.min(1, value)) * 100)} %`;
const degrees = (value: number) => `${value > 0 ? '+' : value < 0 ? '−' : ''}${Math.round(Math.abs(value))}°`;

export interface SceneInput {
  analysis: AnalysisStatus;
  chain: ChainSettings;
  meters: ChainMeters;
  playback: PlaybackStatus;
}

/**
 * Every part of the object, with the reading it stands for. The order is the order
 * they are listed in the interface, not the drawing order.
 */
export function sceneParts({ analysis, chain, meters, playback }: SceneInput): ScenePart[] {
  const live = Boolean(analysis.playing);
  const held = !live && Boolean(analysis.paused);
  const strongest = live || held ? loudestBand(analysis.spectrum) : null;
  const eq = chain.eq;
  const bands = eq.filters.filter(filter => filter.enabled).length;
  const reduction = Number.isFinite(meters.gainReductionDb) ? meters.gainReductionDb : 0;

  return [
    {
      id: 'programme',
      label: 'Programme',
      value: live
        ? `${formatLevel(analysis.rms)} RMS · peak ${formatLevel(analysis.peak)}`
        : held ? `Held · ${formatLevel(analysis.rms)} RMS` : 'Nothing written',
      detail: live
        ? 'The frames leaving the app after the processors and the playback level, measured on the device side.'
        : held
          ? 'A pause keeps the last measured window: no new frame was written, so nothing moved.'
          : 'The playback engine has written no frame, so there is no level to report.',
      action: 'Spectrum',
      engaged: live,
    },
    {
      id: 'spectrum',
      label: 'Spectrum',
      value: strongest
        ? `loudest ${formatHz(strongest.hz)} · ${analysis.spectrum.length} log bands`
        : held ? 'Held · the window before the pause' : 'Flat · no frames',
      detail: `One tick per logarithmic band, ${SPECTRUM_MIN_HZ} Hz to ${formatHz(SPECTRUM_MAX_HZ)}, folded from a real FFT of the output.`,
      action: 'Spectrum',
      engaged: Boolean(analysis.spectrum.length),
    },
    {
      id: 'equaliser',
      label: 'Equaliser',
      value: meters.eqEnabled ? describeEq({ ...eq, enabled: true }) : describeEq({ ...eq, enabled: false }),
      detail: bands
        ? `${bands} ${bands === 1 ? 'band is' : 'bands are'} set; the curve is the engine's own, not a drawing of the sliders.`
        : 'No band list is loaded; the stage is a straight wire either way.',
      action: 'Eq',
      engaged: meters.eqEnabled,
    },
    {
      id: 'panner',
      label: 'Position',
      value: meters.pannerEnabled
        ? `azimuth ${degrees(chain.panner.azimuth)} · width ${percent(chain.panner.width)}`
        : `Bypassed · width ${percent(chain.panner.width)}`,
      detail: chain.panner.room
        ? 'The source is placed in a spherical head model with its early reflection fed back in.'
        : 'The virtual source position applied to the stereo image, without the reflection.',
      action: 'Flow',
      engaged: meters.pannerEnabled,
    },
    {
      id: 'room',
      label: 'Room',
      value: meters.reverbEnabled
        ? `dry ${percent(chain.reverb.dry)} · wet ${percent(chain.reverb.wet)} · decay ${reverbTime(chain.reverb.decay)}`
        : `Bypassed · wet ${percent(chain.reverb.wet)}`,
      detail: 'The feedback network behind the room, as the engine is running it.',
      action: 'Flow',
      engaged: meters.reverbEnabled,
    },
    {
      id: 'limiter',
      label: 'Limiter',
      value: !meters.limiterEnabled
        ? `Bypassed · threshold ${formatDb(chain.limiter.thresholdDb)}`
        : reduction < -0.05
          ? `${formatDb(reduction)} of reduction`
          : `Not reducing · threshold ${formatDb(chain.limiter.thresholdDb)}`,
      detail: 'The gain the limiter is taking off to keep the output under its threshold.',
      action: 'Flow',
      engaged: meters.limiterEnabled,
    },
    {
      id: 'track',
      label: 'Track',
      value: playback.active
        ? `${formatClock(playback.position)} / ${formatClock(playback.duration)} · ${playback.title}`
        : 'Nothing loaded',
      detail: 'Where the loaded track is, carried around the outermost orbit.',
      action: 'Library',
      engaged: playback.active,
    },
  ];
}

/** The part a hovered or focused id names, or null when nothing is focused. */
export function partById(parts: ScenePart[], id: ScenePartId | null): ScenePart | null {
  return id ? parts.find(part => part.id === id) ?? null : null;
}
