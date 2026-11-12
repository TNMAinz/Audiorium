import type { SynthEnvelope, SynthFilterKind, SynthPatch, SynthWave } from '../../model';

/**
 * The instrument's display, without the instrument.
 *
 * The engine's own trace is what the page draws while the synth runs. A stopped
 * synth still has a patch, so the same maths the Rust engine runs is here: the
 * oscillator shapes, the topology-preserving filter and the envelope recurrence
 * are ports, not sketches, which is why the preview moves when a knob moves and
 * looks like what a note will sound like.
 */

/** The engine's rate. The preview runs at it so the filter is the same filter. */
export const VOICE_RATE = 48_000;
/** The window the preview draws: twelve milliseconds, about five cycles of A4 —
 * wide enough for each cycle to be read, short enough for the shape to be seen. */
export const PREVIEW_SECONDS = 0.012;
/** Columns across the preview trace. */
export const PREVIEW_COLUMNS = 260;

/** Concert pitch: every key and every tuner in the room. */
export const midiHz = (note: number) => 440 * 2 ** ((note - 69) / 12);

/**
 * The correction a saw or a square needs at its own jump, so the harmonics the
 * shape does not have do not fold back into the band as aliasing.
 */
export function polyBlep(t: number, dt: number): number {
  if (dt <= 0) return 0;
  if (t < dt) {
    const x = t / dt;
    return x + x - x * x - 1;
  }
  if (t > 1 - dt) {
    const x = (t - 1) / dt;
    return x * x + x + x + 1;
  }
  return 0;
}

/**
 * One oscillator sample, at a phase in 0..1 and a per-sample step of dt. A table
 * oscillator reads the cycle the engine handed over; without one it draws a sine,
 * because a shape it has not been given is not one it may invent.
 */
export function waveSample(wave: SynthWave, phase: number, dt: number, cycle?: number[]): number {
  switch (wave) {
    case 'saw': return 2 * phase - 1 - polyBlep(phase, dt);
    case 'square': return (phase < 0.5 ? 1 : -1) + polyBlep(phase, dt) - polyBlep((phase + 0.5) % 1, dt);
    // A triangle's harmonics fall as the square of their number: the naive shape
    // is already quiet above the band.
    case 'triangle': return 1 - 4 * Math.abs(phase - 0.5);
    case 'sine': return Math.sin(2 * Math.PI * phase);
    case 'table': {
      if (!cycle || cycle.length < 2) return Math.sin(2 * Math.PI * phase);
      const at = phase * cycle.length;
      const first = Math.floor(at) % cycle.length;
      const second = (first + 1) % cycle.length;
      return cycle[first] + (cycle[second] - cycle[first]) * (at - Math.floor(at));
    }
  }
}

export interface SvfState { ic1: number; ic2: number }

/**
 * The engine's topology-preserving state-variable filter, difference equation for
 * difference equation. The states are the integrators' own, which is what lets a
 * cutoff move every sample without the filter blowing up.
 */
export function svfStep(state: SvfState, input: number, cutoff: number, q: number, kind: SynthFilterKind): number {
  const g = Math.tan((Math.PI * Math.min(Math.max(cutoff, 20), VOICE_RATE * 0.45)) / VOICE_RATE);
  const k = 1 / Math.max(q, 0.2);
  const a1 = 1 / (1 + g * (g + k));
  const a2 = g * a1;
  const a3 = g * a2;
  const v3 = input - state.ic2;
  const v1 = a1 * state.ic1 + a2 * v3;
  const v2 = state.ic2 + a2 * state.ic1 + a3 * v3;
  state.ic1 = 2 * v1 - state.ic1;
  state.ic2 = 2 * v2 - state.ic2;
  if (kind === 'lowpass') return v2;
  if (kind === 'bandpass') return v1;
  return input - k * v1 - v2;
}

/**
 * Where the filter sits once a note has settled: the key it was played on, plus
 * the filter envelope's sustain. The attack sweep is the envelope's job, and it
 * has its own graph.
 */
export function settledCutoff(patch: SynthPatch, note: number): number {
  const key = (patch.filter.keyTrack * (note - 60)) / 12;
  const sweep = patch.filter.envAmount * 4 * patch.filterEnv.sustain;
  return patch.filter.cutoff * 2 ** (key + sweep);
}

export interface VoiceTrace {
  /** One sample a column, in frames. */
  osc1: number[];
  osc2: number[];
  /** The lowest and highest frame of each column, after the filter. */
  low: number[];
  high: number[];
}

/**
 * One note of a patch, frame by frame, exactly as `Voice::frame` builds it: both
 * oscillators with their own phases, the sub an octave under oscillator one, the
 * noise the engine's own generator makes, then the filter, at the level the amp
 * envelope is holding.
 *
 * A voice that is held sits at its sustain, so the preview is the note once the
 * attack is over — the shape a listener would call the sound of the patch.
 */
/** The state every filter model the patch can ask for keeps, per voice channel. */
export interface ChannelFilterState {
  svf: SvfState;
  ladder: number[];
  comb: Float32Array;
  write: number;
}

export const channelFilterState = (): ChannelFilterState => ({
  svf: { ic1: 0, ic2: 0 },
  ladder: [0, 0, 0, 0],
  comb: new Float32Array(2_401),
  write: 0,
});

/** The ladder's soft clip: linear near zero, saturating at the ends. */
const softClip = (value: number) => {
  const clamped = Math.max(-3, Math.min(3, value));
  return clamped / (1 + Math.abs(clamped) * 0.5);
};

/**
 * One sample of any filter model: the state variable as it is, the four-pole ladder
 * with its saturating feedback at twice the rate, and the comb whose delay is tuned
 * by the cutoff. The engine runs these; the preview draws them.
 */
export function channelFilterStep(
  state: ChannelFilterState,
  input: number,
  cutoff: number,
  q: number,
  resonance: number,
  kind: SynthFilterKind,
): number {
  if (kind === 'ladder') {
    const g = 1 - Math.exp((-2 * Math.PI * Math.min(Math.max(cutoff, 20), VOICE_RATE * 0.4)) / (VOICE_RATE * 2));
    const k = Math.max(0, Math.min(1, resonance)) * 3.8;
    let out = 0;
    for (let pass = 0; pass < 2; pass += 1) {
      let value = softClip(input - state.ladder[3] * k);
      for (let stage = 0; stage < 4; stage += 1) {
        state.ladder[stage] += g * (value - state.ladder[stage]);
        value = state.ladder[stage];
      }
      out = state.ladder[3];
    }
    return out;
  }
  if (kind === 'comb') {
    const length = state.comb.length;
    const delay = Math.max(2, Math.min(length - 2, VOICE_RATE / Math.max(30, Math.min(8_000, cutoff))));
    const read = (state.write + length - delay) % length;
    const first = Math.floor(read) % length;
    const second = (first + 1) % length;
    const delayed = state.comb[first] + (state.comb[second] - state.comb[first]) * (read - Math.floor(read));
    state.comb[state.write] = input + delayed * Math.max(0, Math.min(1, resonance)) * 0.94;
    state.write = (state.write + 1) % length;
    return input * 0.5 + delayed * 0.5;
  }
  return svfStep(state.svf, input, cutoff, q, kind);
}

export function previewVoice(
  patch: SynthPatch,
  note = 69,
  columns = PREVIEW_COLUMNS,
  cycles: { one?: number[]; two?: number[] } = {},
): VoiceTrace {
  const hz = midiHz(note);
  const samples = Math.round(PREVIEW_SECONDS * VOICE_RATE);
  const perColumn = Math.max(1, Math.ceil(samples / columns));
  const width = Math.max(1, Math.floor(samples / perColumn));
  const osc1: number[] = [];
  const osc2: number[] = [];
  const low: number[] = [];
  const high: number[] = [];

  const ratio1 = 2 ** (patch.osc1.octave + patch.osc1.semitone / 12 + patch.osc1.cents / 1200);
  const ratio2 = 2 ** (patch.osc2.octave + patch.osc2.semitone / 12 + patch.osc2.cents / 1200);
  const dt1 = Math.min(0.45, (hz * ratio1) / VOICE_RATE);
  const dt2 = Math.min(0.45, (hz * ratio2) / VOICE_RATE);
  const subDt = Math.min(0.45, hz * 0.5 / VOICE_RATE);
  const cutoff = settledCutoff(patch, note);
  const resonance = Math.max(0, Math.min(1, patch.filter.resonance));
  const q = 0.5 + resonance * 9.5;
  const gain = patch.amp.sustain;

  let phase1 = 0;
  let phase2 = 0;
  let subPhase = 0;
  // The engine's xorshift, seeded the way a new voice seeds it for this note.
  let noise = (0x9e37_79b9 ^ Math.imul(note, 2_654_435_761)) >>> 0;
  const filter = channelFilterState();

  let columnLow = 0;
  let columnHigh = 0;
  for (let index = 0; index < width * perColumn; index += 1) {
    const one = waveSample(patch.osc1.wave, phase1, dt1, cycles.one);
    const two = waveSample(patch.osc2.wave, phase2, dt2, cycles.two);
    let sample = one * patch.osc1.level + two * patch.osc2.level;
    phase1 = (phase1 + dt1) % 1;
    phase2 = (phase2 + dt2) % 1;
    if (patch.noise > 0) {
      noise = (noise ^ (noise << 13)) >>> 0;
      noise = (noise ^ (noise >>> 17)) >>> 0;
      noise = (noise ^ (noise << 5)) >>> 0;
      sample += ((noise >>> 8) / 8_388_608 - 1) * patch.noise;
    }
    if (patch.sub > 0) {
      sample += waveSample('square', subPhase, subDt) * patch.sub * 0.5;
      subPhase = (subPhase + subDt) % 1;
    }
    const mixed = channelFilterStep(filter, sample, cutoff, q, resonance, patch.filter.kind) * gain;

    if (index % perColumn === 0) {
      const column = index / perColumn;
      osc1[column] = one * patch.osc1.level;
      osc2[column] = two * patch.osc2.level;
      columnLow = mixed;
      columnHigh = mixed;
      low[column] = mixed;
      high[column] = mixed;
    } else {
      const column = Math.floor(index / perColumn);
      columnLow = Math.min(columnLow, mixed);
      columnHigh = Math.max(columnHigh, mixed);
      low[column] = columnLow;
      high[column] = columnHigh;
    }
  }
  return { osc1, osc2, low, high };
}

export interface EnvelopePoint { x: number; level: number }
export interface EnvelopePlot {
  points: EnvelopePoint[];
  /** Where each stage of the envelope ends, as a share of the drawn width. */
  attackEnd: number;
  decayEnd: number;
  releaseStart: number;
}

/**
 * An envelope drawn by running its own recurrence, not by joining four corners:
 * a linear attack, and the exponential decay and release the engine uses, which
 * is what makes a tail look like a tail instead of a staircase.
 */
export function envelopePlot(envelope: SynthEnvelope, hold = 0.55): EnvelopePlot {
  const stride = 96;
  const times: number[] = [];
  const levels: number[] = [];
  let level = 0;
  let elapsed = 0;
  const push = () => { times.push(elapsed); levels.push(level); };

  const attackStep = 1 / (Math.max(envelope.attack, 0.001) * VOICE_RATE);
  while (level < 1) {
    level = Math.min(1, level + attackStep * stride);
    elapsed += stride / VOICE_RATE;
    push();
  }
  const attackEnd = elapsed;
  const decayK = Math.exp(-1 / (Math.max(envelope.decay, 0.001) * 0.35 * VOICE_RATE));
  while (Math.abs(level - envelope.sustain) >= 0.001) {
    level = envelope.sustain + (level - envelope.sustain) * decayK ** stride;
    elapsed += stride / VOICE_RATE;
    push();
  }
  level = envelope.sustain;
  const decayEnd = elapsed;
  for (let held = 0; held < hold; held += stride / VOICE_RATE) {
    elapsed += stride / VOICE_RATE;
    push();
  }
  const releaseStart = elapsed;
  const releaseK = Math.exp(-1 / (Math.max(envelope.release, 0.005) * 0.35 * VOICE_RATE));
  while (level >= 0.0005) {
    level *= releaseK ** stride;
    elapsed += stride / VOICE_RATE;
    push();
  }
  times.push(elapsed);
  levels.push(0);

  const total = elapsed || 1;
  return {
    points: times.map((time, index) => ({ x: time / total, level: levels[index] })),
    attackEnd: attackEnd / total,
    decayEnd: decayEnd / total,
    releaseStart: releaseStart / total,
  };
}
