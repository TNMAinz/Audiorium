import type { LoudnessReadings } from './model';

/**
 * The delivery a master is aimed at, and whether the file the engine measured meets
 * it. Nothing here normalises anything: the check reads the figures the render
 * produced and says how far they are from the target, so a master that misses is
 * reported as missing instead of being quietly re-rendered to fit.
 */
export type TargetId = 'streaming' | 'podcast' | 'broadcast' | 'off';

export interface MasterTarget {
  id: TargetId;
  label: string;
  /** Integrated loudness the delivery expects, in LUFS. Null when nothing is judged. */
  lufs: number | null;
  /** The true peak the delivery allows, in dBTP. */
  ceilingDb: number;
}

/**
 * The usual delivery figures. A platform normalises on playback, so these are what a
 * file should carry to survive that: a loudness the normaliser will not have to move
 * far, and a true peak that leaves room for lossy encoding.
 */
export const masterTargets: MasterTarget[] = [
  { id: 'streaming', label: 'Streaming', lufs: -14, ceilingDb: -1 },
  { id: 'podcast', label: 'Podcast', lufs: -16, ceilingDb: -1 },
  { id: 'broadcast', label: 'Broadcast', lufs: -23, ceilingDb: -1 },
  { id: 'off', label: 'None', lufs: null, ceilingDb: -1 },
];

/** A loudness within half a unit of the target is the target, not a near miss. */
export const TARGET_TOLERANCE_LU = 0.5;
/** A true peak has to be under the ceiling; float noise is not a violation. */
export const CEILING_TOLERANCE_DB = 0.05;

export interface SpecCheck {
  /** False for the "None" target: the figures are measured and nothing is judged. */
  judged: boolean;
  /** Measured integrated loudness minus the target, in LU. Negative is quieter. */
  deltaLu: number;
  /** Measured true peak minus the allowed ceiling, in dB. Positive is over. */
  peakOverDb: number;
  loudnessHolds: boolean;
  peakHolds: boolean;
  /** True when every rule this target states is met. */
  meets: boolean;
}

/** Null until the render has measured something: a dash, never a pass. */
export function checkMaster(target: MasterTarget, readings: LoudnessReadings): SpecCheck | null {
  if (!readings || readings.blocks <= 0) return null;
  const lufs = readings.integratedLufs;
  const peak = readings.truePeakDb;
  if (!Number.isFinite(lufs) || lufs <= -69.9 || !Number.isFinite(peak) || peak <= -119.9) return null;
  const judged = target.lufs !== null;
  const deltaLu = judged ? lufs - (target.lufs as number) : 0;
  const peakOverDb = peak - target.ceilingDb;
  const loudnessHolds = !judged || Math.abs(deltaLu) <= TARGET_TOLERANCE_LU;
  // "None" judges nothing at all, ceiling included: a target that is switched off
  // must not fail a file for a figure it never asked for.
  const peakHolds = !judged || peakOverDb <= CEILING_TOLERANCE_DB;
  return {
    judged,
    deltaLu,
    peakOverDb,
    loudnessHolds,
    peakHolds,
    meets: loudnessHolds && peakHolds,
  };
}

/** The gain that would move the measured master onto its target loudness, in dB. */
export function gainToTarget(target: MasterTarget, readings: LoudnessReadings): number | null {
  const check = checkMaster(target, readings);
  if (!check || !check.judged) return null;
  return -check.deltaLu;
}
