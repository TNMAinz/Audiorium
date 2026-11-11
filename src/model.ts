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

export type Workspace = 'Orbit' | 'Spectrum' | 'Flow' | 'Processors' | 'Eq' | 'Synth' | 'Presets' | 'Master' | 'Spatial' | 'Library' | 'Devices';
export type Theme = 'Ion' | 'Violet' | 'Ember';

export const workspaces: Record<Workspace, { label: string; title: string; description: string }> = {
  Orbit: { label: 'Observatory', title: 'Observatory', description: 'The scene, driven by what you are playing.' },
  Spectrum: { label: 'Analysis', title: 'Analysis', description: 'Levels and spectrum of what leaves the app.' },
  Flow: { label: 'Signal chain', title: 'Signal chain', description: 'Every stage between the decoded file and the output.' },
  Processors: { label: 'Processors', title: 'Processors', description: 'The six processors, in the order the audio reaches them.' },
  Eq: { label: 'Equaliser', title: 'Equaliser', description: 'The parametric bands the audio path is running.' },
  Synth: { label: 'Synth', title: 'Synth', description: 'Two oscillators with unison and wavetables, five filter models, a modulation matrix and the rack, played from a keyboard.' },
  Presets: { label: 'Presets', title: 'Presets', description: 'The factory packs: 536 collections of four patches each, every family crossed with every oscillator shape and filter.' },
  Master: { label: 'Master', title: 'Master', description: 'Export a file through the rack, and measure it before and after.' },
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
export type LimiterDetector = 'both' | 'left' | 'right';

/** Mirror of the Rust `PannerConfig`: a spherical-head binaural position, not an HRTF. */
export interface PannerSettings {
  enabled: boolean; azimuth: number; elevation: number; width: number;
  room: boolean; reflectionLength: number; reflectionLevel: number; timbre: number; focus: number;
}
/** Dry and wet are separate gains, not a crossfade: the room never takes the dry signal away. */
export interface ReverbSettings { enabled: boolean; decay: number; dry: number; wet: number; network: string }
export interface LimiterSettings {
  enabled: boolean; thresholdDb: number;
  /** The level the stage holds. Make-up trims the limited signal to it; turning it
   * down attenuates rather than making the gain work harder. */
  ceilingDb: number;
  release: LimiterRelease; detector: LimiterDetector;
  softClip: boolean; autoMakeup: boolean;
}
/** The six biquads the Rust engine implements, and nothing it does not. */
export type EqKind = 'peaking' | 'lowshelf' | 'highshelf' | 'lowpass' | 'highpass' | 'notch';
/** A band that gives way when its own region is loud: frequency-dependent compression. */
export interface BandDynamics { thresholdDb: number; ratio: number; attackMs: number; releaseMs: number }
export const defaultDynamics: BandDynamics = { thresholdDb: -24, ratio: 3, attackMs: 10, releaseMs: 200 };
export interface EqFilter {
  kind: EqKind; frequency: number; gainDb: number; q: number; enabled: boolean;
  /** Absent on a static band, which is every band saved before dynamics existed. */
  dynamics?: BandDynamics | null;
}
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
  /** The engine's own range for a dynamic band. */
  dynamics: {
    threshold: { min: -60, max: 0, step: 1 },
    ratio: { min: 1, max: 20, step: 0.5 },
    attack: { min: 0.5, max: 200, step: 0.5 },
    release: { min: 10, max: 2000, step: 10 },
  },
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

/** One processor in the rack. The engine runs each of these exactly once. */
export type StageId = 'panner' | 'reverb' | 'eq' | 'compressor' | 'saturator' | 'limiter';
/** The two signals the rack processes: the stereo pair, or its sum and difference. */
export type StereoMode = 'lr' | 'ms';
export const stereoModes: { id: StereoMode; label: string; note: string }[] = [
  { id: 'lr', label: 'L / R', note: 'Left and right, as they arrive.' },
  { id: 'ms', label: 'M / S', note: 'Mid and side, encoded before the first stage and decoded after the last. The panner and the room are left/right tools: leave them out of a true mid/side pass.' },
];
export interface CompressorSettings {
  enabled: boolean; thresholdDb: number; ratio: number;
  attackMs: number; releaseMs: number; kneeDb: number; makeupDb: number;
}
export interface SaturatorSettings { enabled: boolean; driveDb: number; tone: number; mix: number }
export interface EffectSettings { enabled: boolean; amount: number; rate: number; feedback: number; mix: number }
export const effectNames = ['Delay', 'Chorus', 'Flanger', 'Phaser', 'Bitcrusher', 'Distortion', 'Tremolo', 'Auto-filter', 'Stereo widener', 'Ring modulation', 'Frequency shift', 'Noise gate'] as const;
export const defaultEffect: EffectSettings = { enabled: false, amount: 0.5, rate: 0.5, feedback: 0.25, mix: 0.5 };
export interface ChainSettings {
  /** The rack, first processor first: the order the audio actually travels. */
  order: StageId[];
  /** Which pair the rack processes. A chain saved before this switch has none. */
  stereoMode: StereoMode;
  panner: PannerSettings; reverb: ReverbSettings; eq: EqSettings;
  compressor: CompressorSettings; saturator: SaturatorSettings; limiter: LimiterSettings;
  effects: EffectSettings[];
}
export interface ChainMeters {
  gainReductionDb: number; outputPeakDb: number; wetMix: number; compressorReductionDb: number;
  pannerEnabled: boolean; reverbEnabled: boolean; eqEnabled: boolean;
  compressorEnabled: boolean; saturatorEnabled: boolean; limiterEnabled: boolean;
}

/** The order a new install runs, which is the fixed sequence the rack replaced. */
export const defaultOrder: StageId[] = ['panner', 'reverb', 'eq', 'compressor', 'saturator', 'limiter'];
export const stageNames: Record<StageId, string> = {
  panner: 'Binaural panner', reverb: 'Reverb', eq: 'Equaliser',
  compressor: 'Compressor', saturator: 'Saturator', limiter: 'Limiter',
};
export const stageModels: Record<StageId, string> = {
  panner: 'spherical head', reverb: 'seeded delay network', eq: 'parametric cascade',
  compressor: 'stereo-linked', saturator: 'soft saturation', limiter: 'look-ahead true peak',
};

/** True when the list carries every processor exactly once, which is what the engine accepts. */
export const isStageOrder = (order: unknown): order is StageId[] =>
  Array.isArray(order) && order.length === defaultOrder.length
  && defaultOrder.every(stage => order.includes(stage));

/** One move of the rack: earlier or later, never past either end. */
export function moveStage(order: StageId[], from: number, to: number): StageId[] {
  if (from === to || from < 0 || to < 0 || from >= order.length || to >= order.length) return order;
  const next = order.slice();
  const [moved] = next.splice(from, 1);
  next.splice(to, 0, moved);
  return next;
}

export const defaultChain: ChainSettings = {
  order: defaultOrder,
  stereoMode: 'lr',
  panner: { enabled: false, azimuth: 0, elevation: 0, width: 0.25, room: false, reflectionLength: 0.35, reflectionLevel: 0.2, timbre: 0, focus: 0.5 },
  reverb: { enabled: false, decay: 0.55, dry: 1, wet: 0.3, network: 'woven:4211' },
  eq: defaultEq,
  compressor: { enabled: false, thresholdDb: -20, ratio: 3, attackMs: 10, releaseMs: 200, kneeDb: 6, makeupDb: 0 },
  saturator: { enabled: false, driveDb: 6, tone: 0.5, mix: 1 },
  limiter: { enabled: false, thresholdDb: -3, ceilingDb: -0.3, release: 'medium', detector: 'both', softClip: false, autoMakeup: true },
  effects: effectNames.map(() => ({ ...defaultEffect })),
};
export const idleMeters: ChainMeters = {
  gainReductionDb: 0, outputPeakDb: -120, wetMix: 0, compressorReductionDb: 0,
  pannerEnabled: false, reverbEnabled: false, eqEnabled: false,
  compressorEnabled: false, saturatorEnabled: false, limiterEnabled: false,
};

/** What the master page reports while and after a render. */
export type BitDepth = '16' | '24' | '32f';
export const bitDepths: { id: BitDepth; label: string }[] = [
  { id: '16', label: '16-bit PCM' }, { id: '24', label: '24-bit PCM' }, { id: '32f', label: '32-bit float' },
];
/** The container a master is written in: uncompressed WAV, or lossless FLAC. */
export type MasterFormat = 'wav' | 'flac';
export const masterFormats: { id: MasterFormat; label: string }[] = [
  { id: 'wav', label: 'WAV' }, { id: 'flac', label: 'FLAC' },
];
/** FLAC carries integer samples only, so 32-bit float is a WAV answer. */
export const depthsFor = (format: MasterFormat) =>
  format === 'flac' ? bitDepths.filter(option => option.id !== '32f') : bitDepths;
export interface LoudnessReadings {
  integratedLufs: number; shortTermLufs: number; momentaryLufs: number; loudnessRange: number;
  truePeakDb: number; samplePeakDb: number; dcOffsetDb: number; correlation: number; blocks: number;
}
/** The rates a master can be written at. The rack always runs at 48 kHz. */
export type SampleRate = '44.1' | '48' | '96';
export const masterRates: { id: SampleRate; label: string }[] = [
  { id: '44.1', label: '44.1 kHz' }, { id: '48', label: '48 kHz' }, { id: '96', label: '96 kHz' },
];
export const rateLabel = (rate: SampleRate) =>
  masterRates.find(option => option.id === rate)?.label ?? `${rate} kHz`;

export interface RenderStatus {
  running: boolean; phase: string; progress: number; frames: number; seconds: number;
  bytes: number; destination: string; depth: string;
  /** The rate the file is written at, and the track of a queue being written. */
  rate: string; track: string; trackIndex: number; trackTotal: number;
  /** The whole queue's programme loudness, once every track is written. */
  albumLufs: number;
  source: LoudnessReadings; output: LoudnessReadings; error: string;
}
/** A file looked at whole: the pictures the Master page draws it with. */
export interface FileScan {
  frames: number; seconds: number;
  /** Peak per bucket, 0…1, in time order. */
  envelope: number[];
  /** LUFS over time, in time order. */
  loudness: number[];
  columns: number; bands: number;
  /** `columns × bands` values, column by column, 0…1. */
  spectrum: number[];
}
/** What the bypassed rack did to a file: nothing is the answer a render path wants. */
export interface NullReport { frames: number; differing: number; maxDifferenceDb: number }
export const idleLoudness: LoudnessReadings = {
  integratedLufs: -70, shortTermLufs: -70, momentaryLufs: -70, loudnessRange: 0,
  truePeakDb: -120, samplePeakDb: -120, dcOffsetDb: -120, correlation: 0, blocks: 0,
};
export const idleRender: RenderStatus = {
  running: false, phase: 'idle', progress: 0, frames: 0, seconds: 0, bytes: 0,
  destination: '', depth: '', rate: '', track: '', trackIndex: 0, trackTotal: 0, albumLufs: -70,
  source: idleLoudness, output: idleLoudness, error: '',
};
export const idleScan: FileScan = { frames: 0, seconds: 0, envelope: [], loudness: [], columns: 0, bands: 0, spectrum: [] };
/** The structures the Rust parser accepts, for the reverb's network field. */
export const reverbStructures = ['parallel', 'series', 'woven'] as const;
export const reflectionTimbres = ['Studio', 'Wood', 'Glass', 'Stone', 'Hall'] as const;
export const limiterReleases: { id: LimiterRelease; label: string }[] = [
  { id: 'fast', label: 'Fast' }, { id: 'medium', label: 'Medium' }, { id: 'slow', label: 'Slow' },
];
export const limiterDetectors: { id: LimiterDetector; label: string }[] = [
  { id: 'both', label: 'Both' }, { id: 'left', label: 'Left' }, { id: 'right', label: 'Right' },
];

export interface Speaker { azimuth: number; elevation: number; gain: number; source: string }
export interface LibraryTrack { path: string; fileName: string; title: string; artist: string; album: string; format: string; bytes: number; modified: number }
export interface LibraryScan { root: string; tracks: LibraryTrack[]; skipped: number; truncated: boolean }

export const themes: Record<Theme, { primary: string; secondary: string }> = {
  Ion: { primary: '#6fb6ff', secondary: '#9a8cff' },
  Violet: { primary: '#a38bff', secondary: '#75c3ff' },
  Ember: { primary: '#ffb67d', secondary: '#f3707d' },
};

// ---------- the instrument ----------

export type SynthWave = 'saw' | 'square' | 'triangle' | 'sine' | 'table';
export type SynthFilterKind = 'lowpass' | 'bandpass' | 'highpass' | 'ladder' | 'comb' | 'notch' | 'peak';
export type LfoTarget = 'pitch' | 'filter' | 'amp';
export type LfoWave = 'sine' | 'triangle';
export type ModSource = 'lfo1' | 'lfo2' | 'env1' | 'env2' | 'velocity' | 'key' | 'wheel' | 'pressure' | 'timbre'
  | 'macro1' | 'macro2' | 'macro3' | 'macro4';
export type ModTarget = 'pitch' | 'cutoff' | 'resonance' | 'osc1' | 'osc2' | 'sub' | 'noise' | 'pan';
/** How a source's travel is bent before it reaches the destination. */
export type ModCurve = 'linear' | 'exp' | 'log' | 's';

/** One route: a source, a destination, how far it moves and how it is shaped. */
export interface SynthMod { source: ModSource; target: ModTarget; amount: number; curve: ModCurve }

export interface SynthOscillator {
  wave: SynthWave; octave: number; semitone: number; cents: number; level: number;
  /** Detuned copies summed into this oscillator, 1 to 8. */
  unison: number; detune: number; spread: number;
  /** Which table a `table` oscillator reads, and where in it the morph sits. */
  table: number; morph: number;
}
/** Seconds, except sustain, which is a share of the envelope's height. */
export interface SynthEnvelope { attack: number; decay: number; sustain: number; release: number }
export interface SynthFilter { kind: SynthFilterKind; cutoff: number; resonance: number; envAmount: number; keyTrack: number; drive: number }
export interface SynthLfo { wave: LfoWave; rate: number; depth: number; target: LfoTarget }

/** A patch. The Rust `SynthConfig` is this shape exactly, field for field. */
export interface SynthPatch {
  level: number; glideMs: number;
  osc1: SynthOscillator; osc2: SynthOscillator;
  sub: number; noise: number;
  filter: SynthFilter;
  amp: SynthEnvelope; filterEnv: SynthEnvelope;
  lfo: SynthLfo; lfo2: SynthLfo;
  /** The matrix: summed routes into the patch's own parameters. */
  mods: SynthMod[];
  /** Four performance knobs the matrix can read, so one gesture is several moves. */
  macros: [number, number, number, number];
  /** Cents of offset from equal temperament, one per key. Empty is equal. */
  tuning: number[];
}

export const defaultSynth: SynthPatch = {
  level: 0.7, glideMs: 0,
  osc1: { wave: 'saw', octave: 0, semitone: 0, cents: 0, level: 0.8, unison: 1, detune: 0, spread: 0, table: 0, morph: 0 },
  osc2: { wave: 'saw', octave: -1, semitone: 0, cents: 7, level: 0.3, unison: 1, detune: 0, spread: 0, table: 0, morph: 0 },
  sub: 0, noise: 0,
  filter: { kind: 'lowpass', cutoff: 3200, resonance: 0.15, envAmount: 0.35, keyTrack: 0.3, drive: 0 },
  amp: { attack: 0.004, decay: 0.25, sustain: 0.7, release: 0.35 },
  filterEnv: { attack: 0.002, decay: 0.4, sustain: 0.15, release: 0.3 },
  lfo: { wave: 'sine', rate: 5, depth: 0, target: 'filter' },
  lfo2: { wave: 'triangle', rate: 0.8, depth: 0, target: 'filter' },
  mods: [],
  macros: [0, 0, 0, 0],
  tuning: [],
};

/** The engine's own ranges, so a control cannot ask for a value it would refuse. */
export const synthLimits = {
  level: { min: 0, max: 1, step: 0.01 },
  glide: { min: 0, max: 500, step: 5 },
  octave: { min: -2, max: 2 },
  semitone: { min: -12, max: 12 },
  cents: { min: -100, max: 100, step: 1 },
  oscLevel: { min: 0, max: 1, step: 0.01 },
  sub: { min: 0, max: 1, step: 0.01 },
  noise: { min: 0, max: 1, step: 0.01 },
  cutoff: { min: 20, max: 20000, step: 1 },
  resonance: { min: 0, max: 1, step: 0.01 },
  envAmount: { min: -4, max: 4, step: 0.05 },
  keyTrack: { min: 0, max: 1, step: 0.01 },
  drive: { min: 0, max: 1, step: 0.01 },
  macro: { min: 0, max: 1, step: 0.01 },
  attack: { min: 0.001, max: 5, step: 0.001 },
  decay: { min: 0.001, max: 10, step: 0.001 },
  sustain: { min: 0, max: 1, step: 0.01 },
  release: { min: 0.005, max: 10, step: 0.001 },
  rate: { min: 0.05, max: 20, step: 0.05 },
  depth: { min: 0, max: 1, step: 0.01 },
  unison: { min: 1, max: 8, step: 1 },
  detune: { min: 0, max: 50, step: 1 },
  spread: { min: 0, max: 1, step: 0.01 },
  morph: { min: 0, max: 1, step: 0.01 },
  modAmount: { min: -1, max: 1, step: 0.01 },
} as const;

export const synthWaves: { id: SynthWave; label: string }[] = [
  { id: 'saw', label: 'Saw' }, { id: 'square', label: 'Square' },
  { id: 'triangle', label: 'Triangle' }, { id: 'sine', label: 'Sine' },
  { id: 'table', label: 'Table' },
];
export const synthFilterKinds: { id: SynthFilterKind; label: string }[] = [
  { id: 'lowpass', label: 'Low pass' }, { id: 'bandpass', label: 'Band pass' }, { id: 'highpass', label: 'High pass' },
  { id: 'ladder', label: 'Ladder' }, { id: 'comb', label: 'Comb' },
  { id: 'notch', label: 'Notch' }, { id: 'peak', label: 'Peak' },
];
export const modSources: { id: ModSource; label: string }[] = [
  { id: 'lfo1', label: 'LFO 1' }, { id: 'lfo2', label: 'LFO 2' },
  { id: 'env1', label: 'Amp env' }, { id: 'env2', label: 'Filter env' },
  { id: 'velocity', label: 'Velocity' }, { id: 'key', label: 'Key' }, { id: 'wheel', label: 'Wheel' },
  { id: 'pressure', label: 'Pressure' }, { id: 'timbre', label: 'MPE timbre' },
  { id: 'macro1', label: 'Macro 1' }, { id: 'macro2', label: 'Macro 2' },
  { id: 'macro3', label: 'Macro 3' }, { id: 'macro4', label: 'Macro 4' },
];
export const modCurves: { id: ModCurve; label: string }[] = [
  { id: 'linear', label: 'Linear' }, { id: 'exp', label: 'Exp' },
  { id: 'log', label: 'Log' }, { id: 's', label: 'S' },
];
export const modTargets: { id: ModTarget; label: string }[] = [
  { id: 'pitch', label: 'Pitch' }, { id: 'cutoff', label: 'Cutoff' }, { id: 'resonance', label: 'Resonance' },
  { id: 'osc1', label: 'Osc 1 level' }, { id: 'osc2', label: 'Osc 2 level' },
  { id: 'sub', label: 'Sub level' }, { id: 'noise', label: 'Noise level' }, { id: 'pan', label: 'Pan' },
];
export const lfoTargets: { id: LfoTarget; label: string }[] = [
  { id: 'pitch', label: 'Pitch' }, { id: 'filter', label: 'Filter' }, { id: 'amp', label: 'Amp' },
];
export const lfoWaves: { id: LfoWave; label: string }[] = [
  { id: 'sine', label: 'Sine' }, { id: 'triangle', label: 'Triangle' },
];

/** What the engine reports while it is running. */
export interface SynthStatus {
  running: boolean; voices: number; peakDb: number; device: string; midi: string;
  recording: boolean; recordedSeconds: number; error: string | null;
  /** The output's own trace, oldest column first, as low/high pairs in frames. */
  scope: number[];
}
export const idleSynthStatus: SynthStatus = {
  running: false, voices: 0, peakDb: -120, device: '', midi: '',
  recording: false, recordedSeconds: 0, error: null, scope: [],
};

export interface SynthScale { name: string; cents: number[] }

export interface SynthPresetSummary { id: string; name: string; builtin: boolean }
/** One patch of a pack: an id to load, and the name a card prints. */
export interface SynthPackPatch { id: string; name: string }
export interface SynthPack { id: string; name: string; family: string; blurb: string; patches: SynthPackPatch[] }
/** One axis of the shelf: how much of it the catalogue actually holds. */
export interface SynthCoverageRow { label: string; covered: number; total: number }
/**
 * What the shelf covers, counted by the engine from the packs it ships rather than
 * from a number written into the page. The grid is the systematic half: every family
 * crossed with every oscillator pair and every filter.
 */
export interface SynthCoverage { rows: SynthCoverageRow[]; gridPacks: number; gridPatches: number }
export interface SynthPackCatalogue { families: string[]; packs: SynthPack[]; coverage: SynthCoverage }
export const emptyCoverage: SynthCoverage = { rows: [], gridPacks: 0, gridPatches: 0 };
export const emptyPackCatalogue: SynthPackCatalogue = { families: [], packs: [], coverage: emptyCoverage };
export interface MpeSettings { lowerMembers: number; upperMembers: number; memberBend: number; masterBend: number }
export const defaultMpe: MpeSettings = { lowerMembers: 0, upperMembers: 0, memberBend: 48, masterBend: 2 };
export interface SynthPatchList { patch: SynthPatch; active: string | null; presets: SynthPresetSummary[]; tables: string[]; mpe: MpeSettings }
export const emptySynthPatches: SynthPatchList = { patch: defaultSynth, active: null, presets: [], tables: [], mpe: defaultMpe };

/** "C4": the name every keyboard uses, from the MIDI number. */
export function noteName(note: number): string {
  const names = ['C', 'C♯', 'D', 'D♯', 'E', 'F', 'F♯', 'G', 'G♯', 'A', 'A♯', 'B'];
  return `${names[((note % 12) + 12) % 12]}${Math.floor(note / 12) - 1}`;
}

/** An envelope in one line: "A 4 ms · D 250 ms · S 70 % · R 350 ms". */
export const envelopeLine = (envelope: SynthEnvelope) => {
  const ms = (seconds: number) => (seconds >= 1 ? `${seconds.toFixed(2)} s` : `${Math.round(seconds * 1000)} ms`);
  return `A ${ms(envelope.attack)} · D ${ms(envelope.decay)} · S ${Math.round(envelope.sustain * 100)} % · R ${ms(envelope.release)}`;
};

/** What the instrument is doing, in one line a header can hold. */
export function describeSynth(patch: SynthPatch, status: SynthStatus) {
  const name = `${synthWaves.find(wave => wave.id === patch.osc1.wave)?.label ?? patch.osc1.wave} + ${synthWaves.find(wave => wave.id === patch.osc2.wave)?.label ?? patch.osc2.wave}`;
  const filter = patch.filter.kind === 'lowpass' ? 'LP' : patch.filter.kind === 'bandpass' ? 'BP'
    : patch.filter.kind === 'highpass' ? 'HP' : patch.filter.kind === 'ladder' ? 'Ladder' : 'Comb';
  const stack = patch.osc1.unison > 1 || patch.osc2.unison > 1
    ? ` · ${Math.max(patch.osc1.unison, patch.osc2.unison)}× unison`
    : '';
  const where = status.running ? `${status.device || 'output'} · ${status.voices} ${status.voices === 1 ? 'voice' : 'voices'}` : 'instrument idle';
  return `${name}${stack} · ${filter} ${formatHz(patch.filter.cutoff)} · ${where}`;
}
