/** A level is a linear multiplier, so a percentage alone hides the dB it actually is. */
export const gainDecibels = (gain: number) => (gain > 0 ? 20 * Math.log10(gain) : -Infinity);
/** What the panels are measuring: the app's own playback, or the chosen input. */
export const sourceName = (source: string) => (source === 'input' ? 'the chosen input' : "the app's output");
/** The same answer as one short title, for a row or a heading. */
export const sourceTitle = (source: string) => (source === 'input' ? 'Input capture' : 'Library playback');

/** The reverb time the engine sweeps: 0.2 s at 0, 16 s at 1, geometrically. */
export const reverbDecay = { short: 0.2, long: 16 } as const;
/** The seconds a decay setting means, so the controls can say them, not a percentage. */
export function reverbSeconds(decay: number): number {
  const swept = Number.isFinite(decay) ? Math.min(1, Math.max(0, decay)) : 0;
  return reverbDecay.short * (reverbDecay.long / reverbDecay.short) ** swept;
}
/** "2.2 s": the same sweep, as the time it is worth at 48 kHz. */
export function reverbTime(decay: number): string {
  const seconds = reverbSeconds(decay);
  return `${seconds < 1 ? seconds.toFixed(2) : seconds.toFixed(1)} s`;
}

/** "40 % · −8.0 dB": the setting, and the level it produces. */
export function formatGain(gain: number) {
  const db = gainDecibels(gain);
  return `${Math.round(gain * 100)} % · ${Number.isFinite(db) ? `${db > 0 ? '+' : '−'}${Math.abs(db).toFixed(1)} dB` : '−∞ dB'}`;
}

export type Workspace = 'Orbit' | 'Spectrum' | 'Flow' | 'Eq' | 'Spatial' | 'Library' | 'Devices';
export type Theme = 'Ion' | 'Violet' | 'Ember';

export const workspaces: Record<Workspace, { label: string; title: string; description: string }> = {
  Orbit: { label: 'Observatory', title: 'Observatory', description: 'The scene, driven by what you are playing.' },
  Spectrum: { label: 'Analysis', title: 'Analysis', description: 'Levels and spectrum of what leaves the app.' },
  Flow: { label: 'Signal chain', title: 'Signal chain', description: 'Every stage between the decoded file and the output.' },
  Eq: { label: 'Equaliser', title: 'Equaliser', description: 'The parametric bands the audio path is running.' },
  Spatial: { label: 'Spatial field', title: 'Spatial field', description: 'The virtual speakers the renderer is using.' },
  Library: { label: 'Library', title: 'Library', description: 'Local files, decoded in the app.' },
  Devices: { label: 'Devices', title: 'Devices', description: 'Output devices and their roles.' },
};

export type SpatialMode = 'stereo' | 'surround51' | 'surround71' | 'atmos' | 'binaural' | 'custom';
export interface SpatialSettings { mode: SpatialMode; width: number; crossfeed: number; height: number; rotation: number }

export const defaultSpatial: SpatialSettings = { mode: 'stereo', width: 0.65, crossfeed: 0.35, height: 0.4, rotation: 0 };

// Every mode derives a virtual speaker layout from the stereo mix. Atmos hands
// its 7.1.4 bed to Windows Spatial Sound, which renders it with the format the
// user selected; no Atmos bitstream is decoded here.
export const spatialModes: { id: SpatialMode; label: string; short: string; note: string }[] = [
  { id: 'stereo', label: 'Stereo 2.0', short: '2.0', note: 'Pass-through. No spatial processing.' },
  { id: 'surround51', label: '5.1 virtual', short: '5.1', note: 'Six virtual speakers from the stereo mix.' },
  { id: 'surround71', label: '7.1 virtual', short: '7.1', note: 'Eight virtual speakers, wider sides and rears.' },
  { id: 'atmos', label: 'Dolby Atmos (Windows spatial)', short: 'Atmos', note: 'A 7.1.4 bed from the mix, rendered by Dolby Atmos for Headphones or Home Theater in Windows.' },
  { id: 'binaural', label: 'Binaural', short: 'Binaural', note: 'Head shadow, interaural delay and crossfeed for headphones.' },
  {
    id: 'custom', label: 'Manual field', short: 'Manual',
    note: 'Two speakers and an optional height pair, placed by hand on the Spatial field page.',
  },
];

export interface PlaybackStatus {
  active: boolean; playing: boolean; finished: boolean;
  title: string; artist: string; album: string; path: string; format: string; device: string;
  position: number; duration: number; volume: number;
  /** Which renderer the open track is using: windows, derived or app. */
  spatial: string; error: string | null;
}

export type LimiterRelease = 'fast' | 'medium' | 'slow';
export type LimiterDetector = 'mid' | 'left' | 'right';

/** Mirror of the Rust `PannerConfig`: a spherical-head binaural position, not an HRTF. */
export interface PannerSettings {
  enabled: boolean; azimuth: number; elevation: number; width: number;
  room: boolean; reflectionLength: number; reflectionLevel: number; timbre: number; focus: number;
}
/** Dry and wet are separate gains, not a crossfade: the room never takes the dry signal away. */
export interface ReverbSettings { enabled: boolean; decay: number; dry: number; wet: number; network: string }
export interface LimiterSettings {
  enabled: boolean; thresholdDb: number; release: LimiterRelease; detector: LimiterDetector;
  softClip: boolean; autoMakeup: boolean;
}
/** The six biquads the Rust engine implements, and nothing it does not. */
export type EqKind = 'peaking' | 'lowshelf' | 'highshelf' | 'lowpass' | 'highpass' | 'notch';
export interface EqFilter { kind: EqKind; frequency: number; gainDb: number; q: number; enabled: boolean }
export interface EqSettings { enabled: boolean; preampDb: number; filters: EqFilter[] }
/** The cascade's gain at each frequency, from the coefficients in the audio path. */
export interface EqResponse { frequencies: number[]; gains: number[]; maxBoostDb: number; headroomDb: number }
/** Where a patch's numbers came from, so the picker can name its evidence. */
export interface EqProvenance { label: string; url: string; terms: string }
/** A saved patch: a preamp and a band list, never the stage's bypass. */
export interface EqPreset {
  id: string; name: string; builtin: boolean; note: string; preampDb: number; filters: EqFilter[];
  /** Set on a patch whose bands are published data; absent on one written here. */
  source?: EqProvenance | null;
}
/** A correction is a measured profile: vendored from AutoEq, or fitted here from a published
 * measurement when AutoEq has no profile for the model. A shape is a starting point written
 * here: the id says which, and the picker groups and labels them differently. */
export const isCorrection = (preset: EqPreset) =>
  preset.id.startsWith('autoeq:') || preset.id.startsWith('fit:');
/** A correction this app fitted itself from a published measurement, not a vendored AutoEq file. */
export const isFit = (preset: EqPreset) => preset.id.startsWith('fit:');
/** What the picker shows, and which patch the running bands belong to. */
export interface EqPresetList { presets: EqPreset[]; active: string | null }
export const emptyPresets: EqPresetList = { presets: [], active: null };
/** A name the engine will accept: trimmed, non-empty, at most 40 characters. */
export const presetNameLimit = 40;

export const eqKinds: { id: EqKind; label: string; usedGain: boolean }[] = [
  { id: 'peaking', label: 'Peaking', usedGain: true },
  { id: 'lowshelf', label: 'Low shelf', usedGain: true },
  { id: 'highshelf', label: 'High shelf', usedGain: true },
  { id: 'lowpass', label: 'Low pass', usedGain: false },
  { id: 'highpass', label: 'High pass', usedGain: false },
  { id: 'notch', label: 'Notch', usedGain: false },
];
export const eqKindUsesGain = (kind: EqKind) => kind === 'peaking' || kind === 'lowshelf' || kind === 'highshelf';

// The engine's own limits, so a control cannot ask for a value it would reject.
export const eqLimits = {
  frequency: { min: 20, max: 20_000 },
  gain: { min: -24, max: 24, step: 0.1 },
  q: { min: 0.1, max: 18 },
  preamp: { min: -36, max: 0, step: 0.5 },
  bands: 24,
};
export const defaultEq: EqSettings = { enabled: false, preampDb: 0, filters: [] };
export const idleEqResponse: EqResponse = { frequencies: [], gains: [], maxBoostDb: 0, headroomDb: 0 };

/** "1.20 kHz" / "240 Hz": one honest unit at every scale. */
export function formatHz(hz: number) {
  if (hz >= 1000) return `${(hz / 1000).toFixed(hz >= 10_000 ? 1 : 2)} kHz`;
  return `${Math.round(hz)} Hz`;
}

/** "+4.5 dB" / "−6.0 dB" / "0.0 dB", with a real minus sign. */
export function formatDb(value: number, places = 1) {
  const magnitude = Math.abs(value).toFixed(places);
  if (Number(magnitude) === 0) return `${magnitude} dB`;
  return `${value > 0 ? '+' : '−'}${magnitude} dB`;
}

/** The patch the running bands belong to, or the working set when they belong to none. */
export function describePatch(presets: EqPresetList) {
  return presets.presets.find(preset => preset.id === presets.active)?.name ?? 'Unsaved bands';
}

/** What the equaliser is doing, in one line a header can hold. */
export function describeEq(eq: EqSettings) {
  const bands = eq.filters.filter(filter => filter.enabled).length;
  if (!eq.enabled) return bands ? `${bands} bands bypassed` : 'Bypassed';
  if (!bands && eq.preampDb === 0) return 'No bands: pass-through';
  return `${bands} ${bands === 1 ? 'band' : 'bands'} · preamp ${formatDb(eq.preampDb)}`;
}

export interface ChainSettings { panner: PannerSettings; reverb: ReverbSettings; eq: EqSettings; limiter: LimiterSettings }
export interface ChainMeters {
  gainReductionDb: number; outputPeakDb: number; wetMix: number;
  pannerEnabled: boolean; reverbEnabled: boolean; eqEnabled: boolean; limiterEnabled: boolean;
}

export const defaultChain: ChainSettings = {
  panner: { enabled: false, azimuth: 0, elevation: 0, width: 0.25, room: false, reflectionLength: 0.35, reflectionLevel: 0.2, timbre: 0, focus: 0.5 },
  reverb: { enabled: false, decay: 0.55, dry: 1, wet: 0.3, network: 'woven:4211' },
  eq: defaultEq,
  limiter: { enabled: false, thresholdDb: -3, release: 'medium', detector: 'mid', softClip: false, autoMakeup: true },
};
export const idleMeters: ChainMeters = {
  gainReductionDb: 0, outputPeakDb: -120, wetMix: 0,
  pannerEnabled: false, reverbEnabled: false, eqEnabled: false, limiterEnabled: false,
};
/** The structures the Rust parser accepts, for the reverb's network field. */
export const reverbStructures = ['parallel', 'series', 'woven'] as const;
export const reflectionTimbres = ['Studio', 'Wood', 'Glass', 'Stone', 'Hall'] as const;
export const limiterReleases: { id: LimiterRelease; label: string }[] = [
  { id: 'fast', label: 'Fast' }, { id: 'medium', label: 'Medium' }, { id: 'slow', label: 'Slow' },
];
export const limiterDetectors: { id: LimiterDetector; label: string }[] = [
  { id: 'mid', label: 'Mid' }, { id: 'left', label: 'Left' }, { id: 'right', label: 'Right' },
];

export interface Speaker { azimuth: number; elevation: number; gain: number; source: string }
export interface LibraryTrack { path: string; fileName: string; title: string; artist: string; album: string; format: string; bytes: number; modified: number }
export interface LibraryScan { root: string; tracks: LibraryTrack[]; skipped: number; truncated: boolean }

export const themes: Record<Theme, { primary: string; secondary: string }> = {
  Ion: { primary: '#6fb6ff', secondary: '#9a8cff' },
  Violet: { primary: '#a38bff', secondary: '#75c3ff' },
  Ember: { primary: '#ffb67d', secondary: '#f3707d' },
};
