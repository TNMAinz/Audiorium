//! The instrument: the voice architecture of the synth, and the figures it holds.
//!
//! This module is the generator, and it is deliberately separate from the player.
//! The player reads a file at whatever rate the device asks for; an instrument is a
//! clock fed by note events. They share the rack and nothing else, so a live note
//! never touches the path a master is rendered by — the offline render here is a
//! pure function of a patch and a list of events, which is also what makes it
//! testable without an ear.
//!
//! Everything below is one sample at a time and allocates nothing: the voices, the
//! filter states and the envelope states are all held up front.

use serde::{Deserialize, Serialize};

/// The engine rate. One rate everywhere, as with the rack: an instrument that
/// changed rate per device would have to re-derive every filter mid-note.
pub const RATE: f32 = 48_000.0;
/// The voices the pool holds, which is the widest chord a patch can be played in.
pub const VOICES: usize = 16;
/// The detuned copies one oscillator can hold. Eight is the stack a pad is built
/// from; beyond that the copies stop sounding like one instrument and the cost per
/// sample climbs without being heard.
pub const MAX_UNISON: usize = 8;
/// The routes the matrix holds. Eight is more than a patch is read with, and a
/// fixed ceiling keeps the per-sample loop bounded.
pub const MAX_MODS: usize = 8;
/// The shipped wavetables, each a stack of frames.
pub const TABLES: usize = 4;
/// Frames per table, which is what the morph position travels across.
pub const FRAMES: usize = 8;
/// Samples in a table frame at the finest level, and how many halvings the mip
/// chain holds: 2048 >> 7 is 16 samples, enough for the top of the keyboard.
const TABLE_SIZE: usize = 2048;

/// The samples in one frame of a table, public because an import has to cut a file
/// into frames of exactly this many samples before it can be turned into harmonics.
pub const TABLE_SAMPLES: usize = TABLE_SIZE;

/// The most frames a table can hold: the morph walks the stack, so a thousand
/// frames would be a thousand linear reads of harmonics for one sample.
pub const MAX_FRAMES: usize = FRAMES;
const TABLE_LEVELS: usize = 8;

const CUTOFF_MIN: f32 = 20.0;
const CUTOFF_MAX: f32 = 20_000.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Wave {
    Saw,
    Square,
    Triangle,
    Sine,
    /// A table of frames, read at the oscillator's own position: the sound is
    /// chosen by the table and the morph, not by the shape of the wave.
    Table,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FilterKind {
    Lowpass,
    Bandpass,
    Highpass,
    /// Four one-poles in a ladder, saturated at the input: the sound a resonant
    /// analog filter makes when it is pushed, not the clean state variable.
    Ladder,
    /// A tuned delay fed back on itself. The cutoff is the pitch of the comb, so
    /// the filter is audible as a note rather than as a slope.
    Comb,
    /// The band the state variable rejects, and the band it lifts.
    Notch,
    Peak,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LfoTarget {
    Pitch,
    Filter,
    Amp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LfoWave {
    Sine,
    Triangle,
}

fn one() -> u8 {
    1
}

/// One oscillator's tuning and level. The tuning is written the way the front
/// panel of an instrument writes it: octaves, semitones and cents.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Oscillator {
    pub wave: Wave,
    pub octave: i32,
    pub semitone: i32,
    pub cents: f32,
    pub level: f32,
    /// The detuned copies summed into this oscillator, 1 to 8.
    #[serde(default = "one")]
    pub unison: u8,
    /// How far apart the copies are spread, in cents from edge to edge.
    #[serde(default)]
    pub detune: f32,
    /// How far across the image the copies sit, 0 all in the middle, 1 edge to edge.
    #[serde(default)]
    pub spread: f32,
    /// Which table a `Table` oscillator reads, and where in it the reading sits.
    #[serde(default)]
    pub table: u8,
    #[serde(default)]
    pub morph: f32,
}

impl Default for Oscillator {
    fn default() -> Self {
        Self {
            wave: Wave::Saw,
            octave: 0,
            semitone: 0,
            cents: 0.0,
            level: 0.8,
            unison: 1,
            detune: 0.0,
            spread: 0.0,
            table: 0,
            morph: 0.0,
        }
    }
}

/// What can move a parameter besides the two LFOs' own destinations. The list is
/// closed: every source is one number the engine already has per voice or per
/// frame, so a route costs an add and never a lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModSource {
    Lfo1,
    Lfo2,
    Env1,
    Env2,
    Velocity,
    Key,
    Wheel,
    /// Per-note aftertouch, from an MPE controller or a channel's pressure.
    Pressure,
    /// Per-note MPE slide (CC74).
    Timbre,
    Macro1,
    Macro2,
    Macro3,
    Macro4,
}

/// How a source's travel is bent before it reaches the destination. A curve is a
/// shape on the source, not a second amount: two routes can share a source and read
/// it differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModCurve {
    #[default]
    Linear,
    /// Slow at the start, fast at the end.
    Exp,
    /// Fast at the start, slow at the end.
    Log,
    /// Both ends slow, the middle steep.
    S,
}

impl ModCurve {
    /// The shaped value. The curve keeps the sign, so a bipolar source stays
    /// bipolar and a unipolar one never crosses zero.
    fn shape(self, value: f32) -> f32 {
        let magnitude = value.abs().clamp(0.0, 1.0);
        let shaped = match self {
            ModCurve::Linear => magnitude,
            ModCurve::Exp => magnitude * magnitude,
            ModCurve::Log => magnitude.sqrt(),
            ModCurve::S => magnitude * magnitude * (3.0 - 2.0 * magnitude),
        };
        shaped.copysign(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModTarget {
    Pitch,
    Cutoff,
    Resonance,
    Osc1,
    Osc2,
    Sub,
    Noise,
    Pan,
}

/// One route: a source, a destination and how far the destination moves at the
/// source's full travel. The amount is in the destination's own units — octaves
/// for pitch and cutoff, a share of the parameter for everything else —
/// so a route reads the way the knob it moves does.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mod {
    pub source: ModSource,
    pub target: ModTarget,
    pub amount: f32,
    #[serde(default)]
    pub curve: ModCurve,
}

impl Default for Mod {
    fn default() -> Self {
        Self {
            source: ModSource::Lfo1,
            target: ModTarget::Cutoff,
            amount: 0.0,
            curve: ModCurve::Linear,
        }
    }
}

/// A four-stage envelope, in seconds except for the sustain level.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

impl Default for Envelope {
    fn default() -> Self {
        Self {
            attack: 0.005,
            decay: 0.2,
            sustain: 0.75,
            release: 0.3,
        }
    }
}

/// The filter, with the two modulations a subtractive instrument needs: the
/// envelope's amount, in octaves, and how much the key position opens it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    pub kind: FilterKind,
    pub cutoff: f32,
    pub resonance: f32,
    /// Octaves of cutoff the filter envelope sweeps at its peak.
    pub env_amount: f32,
    /// Octaves per octave of keyboard: 1 follows the keys, 0 stays put.
    pub key_track: f32,
    /// How hard the filter is driven. The ladder's saturation is what hears it;
    /// the linear models are compensated so the knob does not simply add level.
    #[serde(default)]
    pub drive: f32,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            kind: FilterKind::Lowpass,
            cutoff: 3_200.0,
            resonance: 0.15,
            env_amount: 0.35,
            key_track: 0.3,
            drive: 0.0,
        }
    }
}

/// One LFO, shared by every voice: a per-voice LFO would drift out of phase and
/// turn a vibrato into a chorus.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lfo {
    pub wave: LfoWave,
    pub rate: f32,
    pub depth: f32,
    pub target: LfoTarget,
}

impl Default for Lfo {
    fn default() -> Self {
        Self {
            wave: LfoWave::Sine,
            rate: 5.0,
            depth: 0.0,
            target: LfoTarget::Filter,
        }
    }
}

/// Everything a patch is. The frontend mirrors this type exactly; the engine's
/// ranges are the only ones `validate` accepts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SynthConfig {
    pub level: f32,
    pub glide_ms: f32,
    pub osc1: Oscillator,
    pub osc2: Oscillator,
    /// The square an octave under oscillator one, at its own level. Zero is off:
    /// a sub that cannot be switched off is not a patch, it is a colour cast.
    pub sub: f32,
    pub noise: f32,
    pub filter: Filter,
    pub amp: Envelope,
    pub filter_env: Envelope,
    pub lfo: Lfo,
    /// A second LFO, because one is a movement and two are a shape. It has its own
    /// destination like the first, and the matrix can also send either anywhere.
    #[serde(default)]
    pub lfo2: Lfo,
    /// The routes. They are summed, so two routes to one destination add up, and an
    /// empty list is the patch that has not used the matrix yet.
    #[serde(default)]
    pub mods: Vec<Mod>,
    /// Four performance knobs the matrix can read, so one gesture moves several
    /// destinations at once.
    #[serde(default)]
    pub macros: [f32; 4],
    /// A tuning as cents of offset from equal temperament, one per key. Empty is
    /// equal temperament, which is what every patch starts on.
    #[serde(default)]
    pub tuning: Vec<f32>,
}

impl Default for SynthConfig {
    fn default() -> Self {
        Self {
            level: 0.7,
            glide_ms: 0.0,
            osc1: Oscillator {
                level: 0.8,
                ..Oscillator::default()
            },
            osc2: Oscillator {
                wave: Wave::Saw,
                octave: -1,
                cents: 7.0,
                level: 0.3,
                ..Oscillator::default()
            },
            sub: 0.0,
            noise: 0.0,
            filter: Filter::default(),
            amp: Envelope {
                attack: 0.004,
                decay: 0.25,
                sustain: 0.7,
                release: 0.35,
            },
            filter_env: Envelope {
                attack: 0.002,
                decay: 0.4,
                sustain: 0.15,
                release: 0.3,
            },
            lfo: Lfo::default(),
            lfo2: Lfo {
                wave: LfoWave::Triangle,
                rate: 0.8,
                depth: 0.0,
                target: LfoTarget::Filter,
            },
            mods: Vec::new(),
            macros: [0.0; 4],
            tuning: Vec::new(),
        }
    }
}

fn finite(value: f32, name: &str) -> Result<(), String> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(format!("{name} is not a number"))
    }
}

fn range(value: f32, low: f32, high: f32, name: &str) -> Result<(), String> {
    finite(value, name)?;
    if value < low || value > high {
        return Err(format!("{name} is outside {low} to {high}"));
    }
    Ok(())
}

impl Envelope {
    fn validate(&self, name: &str) -> Result<(), String> {
        range(self.attack, 0.001, 5.0, &format!("{name} attack"))?;
        range(self.decay, 0.001, 10.0, &format!("{name} decay"))?;
        range(self.sustain, 0.0, 1.0, &format!("{name} sustain"))?;
        range(self.release, 0.005, 10.0, &format!("{name} release"))
    }
}

impl SynthConfig {
    /// The engine refuses a patch whose numbers a filter or an envelope cannot
    /// hold. The message names the field, so a control can show why it reverted.
    pub fn validate(&self) -> Result<(), String> {
        range(self.level, 0.0, 1.0, "Level")?;
        range(self.glide_ms, 0.0, 500.0, "Glide")?;
        for (name, osc) in [("Oscillator 1", &self.osc1), ("Oscillator 2", &self.osc2)] {
            if osc.octave < -2 || osc.octave > 2 {
                return Err(format!("{name} octave is outside -2 to 2"));
            }
            if osc.semitone < -12 || osc.semitone > 12 {
                return Err(format!("{name} semitone is outside -12 to 12"));
            }
            range(osc.cents, -100.0, 100.0, &format!("{name} fine"))?;
            range(osc.level, 0.0, 1.0, &format!("{name} level"))?;
            if osc.unison < 1 || osc.unison as usize > MAX_UNISON {
                return Err(format!("{name} unison is outside 1 to {MAX_UNISON}"));
            }
            range(osc.detune, 0.0, 50.0, &format!("{name} detune"))?;
            range(osc.spread, 0.0, 1.0, &format!("{name} spread"))?;
            if osc.table as usize >= table_count() {
                return Err(format!("{name} names a table that is not loaded"));
            }
            range(osc.morph, 0.0, 1.0, &format!("{name} morph"))?;
        }
        range(self.sub, 0.0, 1.0, "Sub level")?;
        range(self.noise, 0.0, 1.0, "Noise")?;
        range(self.filter.cutoff, CUTOFF_MIN, CUTOFF_MAX, "Cutoff")?;
        range(self.filter.resonance, 0.0, 1.0, "Resonance")?;
        // Octaves, so four is four octaves of sweep: the range a filter envelope
        // is actually asked for on a pluck.
        range(self.filter.env_amount, -4.0, 4.0, "Filter envelope amount")?;
        range(self.filter.key_track, 0.0, 1.0, "Key tracking")?;
        range(self.filter.drive, 0.0, 1.0, "Filter drive")?;
        for (index, value) in self.macros.iter().enumerate() {
            range(*value, 0.0, 1.0, &format!("Macro {}", index + 1))?;
        }
        if !self.tuning.is_empty() {
            if self.tuning.len() != 128 {
                return Err("A tuning is one offset per key, 128 of them".into());
            }
            for value in &self.tuning {
                range(*value, -100.0, 100.0, "Tuning offset")?;
            }
        }
        self.amp.validate("Amplitude envelope")?;
        self.filter_env.validate("Filter envelope")?;
        range(self.lfo.rate, 0.05, 20.0, "LFO rate")?;
        range(self.lfo.depth, 0.0, 1.0, "LFO depth")?;
        range(self.lfo2.rate, 0.05, 20.0, "Second LFO rate")?;
        range(self.lfo2.depth, 0.0, 1.0, "Second LFO depth")?;
        if self.mods.len() > MAX_MODS {
            return Err(format!("The matrix holds {MAX_MODS} routes"));
        }
        for route in &self.mods {
            range(route.amount, -1.0, 1.0, "Modulation amount")
                .map_err(|reason| format!("{reason} ({:?} to {:?})", route.source, route.target))?;
        }
        Ok(())
    }
}

// ---------- envelopes ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Copy, Debug)]
struct EnvelopeState {
    level: f32,
    stage: Stage,
}

impl EnvelopeState {
    fn new() -> Self {
        Self {
            level: 0.0,
            stage: Stage::Idle,
        }
    }

    fn start(&mut self) {
        self.stage = Stage::Attack;
    }

    fn release(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    fn active(&self) -> bool {
        self.stage != Stage::Idle
    }

    /// Linear attack, exponential decay and release. A linear release would fade
    /// away in a straight line the ear reads as a cut; the exponential is what a
    /// capacitor does, and what every instrument's tail does.
    fn advance(&mut self, config: &Envelope) {
        match self.stage {
            Stage::Idle => self.level = 0.0,
            Stage::Attack => {
                let step = 1.0 / (config.attack.max(0.001) * RATE);
                self.level += step;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                let k = (-1.0 / (config.decay.max(0.001) * 0.35 * RATE)).exp();
                self.level = config.sustain + (self.level - config.sustain) * k;
                if (self.level - config.sustain).abs() < 0.001 {
                    self.level = config.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => self.level = config.sustain,
            Stage::Release => {
                let k = (-1.0 / (config.release.max(0.005) * 0.35 * RATE)).exp();
                self.level *= k;
                if self.level < 0.0005 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
    }
}

// ---------- filter ----------

/// A topology-preserving state-variable filter. The states are the integrators'
/// own, so a cutoff that moves every sample — which an envelope and an LFO both
/// make it do — stays stable instead of blowing up like a direct-form biquad.
#[derive(Clone, Copy, Debug, Default)]
struct Svf {
    ic1: f32,
    ic2: f32,
}

impl Svf {
    fn process(&mut self, input: f32, cutoff: f32, q: f32, kind: FilterKind) -> f32 {
        let cutoff = cutoff.clamp(CUTOFF_MIN, RATE * 0.45);
        let g = (std::f32::consts::PI * cutoff / RATE).tan();
        let k = 1.0 / q.max(0.2);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = input - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        // The band-pass is v1, the low-pass v2, and the high-pass is what the
        // input has left once those are taken out.
        match kind {
            FilterKind::Lowpass => v2,
            FilterKind::Bandpass => v1,
            FilterKind::Highpass => input - k * v1 - v2,
            // The band the low pass and the high pass leave out, and the difference
            // between them: a peak, which is a bell without a slope of its own.
            FilterKind::Notch => input - k * v1,
            FilterKind::Peak => v2 - (input - k * v1 - v2),
            // The ladder and the comb keep their own state; a call that reaches
            // this one with either of them is answered with the low pass rather
            // than panicking on a note.
            FilterKind::Ladder | FilterKind::Comb => v2,
        }
    }

    fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }
}

/// A soft clip without a transcendental: linear near zero, saturating at the ends.
/// The ladder's feedback is what a real filter's transistors do to it, but an
/// `f32::tanh` twice per sample per channel is a price a running instrument pays
/// on every note whether it is pushed or not.
fn soft(value: f32) -> f32 {
    let value = value.clamp(-3.0, 3.0);
    value / (1.0 + value.abs() * 0.5)
}

/// Four one-poles in a ladder, with the last stage fed back through a saturator.
/// The pass is run twice per sample, so the resonant peak at the top of the
/// keyboard is not a shape sampled once per cycle.
#[derive(Clone, Copy, Debug, Default)]
struct Ladder {
    stage: [f32; 4],
}

impl Ladder {
    fn process(&mut self, input: f32, cutoff: f32, resonance: f32) -> f32 {
        let cutoff = cutoff.clamp(CUTOFF_MIN, RATE * 0.4);
        // Two passes per sample are a doubled rate: each one-pole takes a step sized
        // for the rate it is being run at, so the cascade is a four-pole ladder at
        // twice the rate rather than an eight-pole one at this rate.
        let g = 1.0 - (-std::f32::consts::TAU * cutoff / (RATE * 2.0)).exp();
        let k = resonance.clamp(0.0, 1.0) * 3.8;
        let mut out = 0.0;
        for _ in 0..2 {
            let mut value = soft(input - self.stage[3] * k);
            for stage in self.stage.iter_mut() {
                *stage += g * (value - *stage);
                value = *stage;
            }
            out = self.stage[3];
        }
        out
    }

    fn reset(&mut self) {
        self.stage = [0.0; 4];
    }
}

/// The longest delay a comb can be tuned to: 20 Hz at 48 kHz, the bottom of the
/// filter's own range.
const COMB_LENGTH: usize = 2_401;

/// A tuned delay fed back on itself. The cutoff is the delay's pitch, so the filter
/// is heard as a note rather than as a slope, and the resonance is how long the
/// note rings.
#[derive(Clone)]
struct Comb {
    buffer: Vec<f32>,
    index: usize,
}

impl Default for Comb {
    fn default() -> Self {
        Self {
            buffer: vec![0.0; COMB_LENGTH],
            index: 0,
        }
    }
}

impl Comb {
    fn process(&mut self, input: f32, cutoff: f32, resonance: f32) -> f32 {
        let delay = (RATE / cutoff.clamp(30.0, 8_000.0)).clamp(2.0, (COMB_LENGTH - 2) as f32);
        let read = (self.index as f32 + COMB_LENGTH as f32 - delay) % COMB_LENGTH as f32;
        let first = read.floor() as usize % COMB_LENGTH;
        let second = (first + 1) % COMB_LENGTH;
        let frac = read - read.floor();
        let delayed = self.buffer[first] + (self.buffer[second] - self.buffer[first]) * frac;
        self.buffer[self.index] = input + delayed * resonance.clamp(0.0, 1.0) * 0.94;
        self.index = (self.index + 1) % COMB_LENGTH;
        input * 0.5 + delayed * 0.5
    }

    fn reset(&mut self) {
        self.buffer.iter_mut().for_each(|sample| *sample = 0.0);
        self.index = 0;
    }
}

/// One channel's filter, holding the state of every model the patch can ask for.
/// A model that is not selected keeps its state at rest, so switching to it mid-note
/// starts from silence rather than from another model's tail.
#[derive(Clone, Default)]
struct ChannelFilter {
    svf: Svf,
    ladder: Ladder,
    comb: Comb,
}

impl ChannelFilter {
    fn process(&mut self, input: f32, cutoff: f32, q: f32, resonance: f32, kind: FilterKind) -> f32 {
        match kind {
            FilterKind::Lowpass
            | FilterKind::Bandpass
            | FilterKind::Highpass
            | FilterKind::Notch
            | FilterKind::Peak => self.svf.process(input, cutoff, q, kind),
            FilterKind::Ladder => self.ladder.process(input, cutoff, resonance),
            FilterKind::Comb => self.comb.process(input, cutoff, resonance),
        }
    }

    fn reset(&mut self) {
        self.svf.reset();
        self.ladder.reset();
        self.comb.reset();
    }
}

// ---------- modulation ----------

/// Every source the matrix can read, already reduced to one number: the LFOs' own
/// values, the envelopes' heights, the note's velocity and position, and the wheel.
#[derive(Clone, Copy)]
struct Sources {
    lfo1: f32,
    lfo2: f32,
    env1: f32,
    env2: f32,
    velocity: f32,
    key: f32,
    wheel: f32,
    pressure: f32,
    timbre: f32,
    macros: [f32; 4],
}

impl Sources {
    fn read(&self, source: ModSource) -> f32 {
        match source {
            ModSource::Lfo1 => self.lfo1,
            ModSource::Lfo2 => self.lfo2,
            ModSource::Env1 => self.env1,
            ModSource::Env2 => self.env2,
            ModSource::Velocity => self.velocity,
            ModSource::Key => self.key,
            ModSource::Wheel => self.wheel,
            ModSource::Pressure => self.pressure,
            ModSource::Timbre => self.timbre,
            ModSource::Macro1 => self.macros[0],
            ModSource::Macro2 => self.macros[1],
            ModSource::Macro3 => self.macros[2],
            ModSource::Macro4 => self.macros[3],
        }
    }
}

/// The routes summed by destination. Every route lands in the destination's own
/// units, so the engine applies one number per parameter instead of walking the
/// list again inside the sample loop.
#[derive(Clone, Copy, Default)]
struct Modulation {
    pitch: f32,
    cutoff: f32,
    resonance: f32,
    osc1: f32,
    osc2: f32,
    sub: f32,
    noise: f32,
    pan: f32,
}

impl Modulation {
    fn of(config: &SynthConfig, sources: &Sources) -> Self {
        let mut summed = Self::default();
        for route in &config.mods {
            if route.amount == 0.0 {
                continue;
            }
            let value = route.amount * route.curve.shape(sources.read(route.source));
            *match route.target {
                ModTarget::Pitch => &mut summed.pitch,
                ModTarget::Cutoff => &mut summed.cutoff,
                ModTarget::Resonance => &mut summed.resonance,
                ModTarget::Osc1 => &mut summed.osc1,
                ModTarget::Osc2 => &mut summed.osc2,
                ModTarget::Sub => &mut summed.sub,
                ModTarget::Noise => &mut summed.noise,
                ModTarget::Pan => &mut summed.pan,
            } += value;
        }
        summed
    }
}

// ---------- voices ----------

// ---------- wavetables ----------

/// One table frame at every resolution it is read at. A table of `n` samples can
/// hold `n / 2` harmonics and no more, so a note that moves the reading faster
/// reads a shorter copy instead of aliasing the one it cannot represent.
#[derive(Clone)]
struct Frame {
    levels: Vec<Vec<f32>>,
}

impl Frame {
    /// The frame is built once at full length from its harmonics, then halved: each
    /// shorter copy is low-passed to half the band before it is decimated, so the
    /// copies never hold a harmonic their length cannot carry.
    fn build(harmonics: &[f32]) -> Self {
        let mut samples = synthesize(harmonics, TABLE_SIZE);
        let mut levels = Vec::with_capacity(TABLE_LEVELS);
        levels.push(standardise(&samples));
        for _ in 1..TABLE_LEVELS {
            samples = decimate(&levels[levels.len() - 1]);
            levels.push(standardise(&samples));
        }
        Self { levels }
    }

    /// Reconstruct only the supported harmonics, retaining their phases.
    fn from_samples(samples: &[f32]) -> Self {
        let mut reconstructed = vec![0.0f32; TABLE_SIZE];
        let scale = 2.0 / samples.len() as f64;
        for harmonic in 1..=128 {
            let mut sine = 0.0f64;
            let mut cosine = 0.0f64;
            for (index, sample) in samples.iter().enumerate() {
                let phase = std::f64::consts::TAU * harmonic as f64 * index as f64 / samples.len() as f64;
                sine += *sample as f64 * phase.sin();
                cosine += *sample as f64 * phase.cos();
            }
            for (index, sample) in reconstructed.iter_mut().enumerate() {
                let phase = std::f64::consts::TAU * harmonic as f64 * index as f64 / TABLE_SIZE as f64;
                *sample += ((sine * phase.sin() + cosine * phase.cos()) * scale) as f32;
            }
        }
        let mut levels = Vec::with_capacity(TABLE_LEVELS);
        levels.push(standardise(&reconstructed));
        for _ in 1..TABLE_LEVELS {
            levels.push(standardise(&decimate(&levels[levels.len() - 1])));
        }
        Self { levels }
    }

    fn sample(&self, phase: f32, level: usize) -> f32 {
        let samples = &self.levels[level.min(self.levels.len() - 1)];
        let length = samples.len();
        let x = phase * length as f32;
        let first = (x.floor() as usize) % length;
        let second = (first + 1) % length;
        let frac = x - x.floor();
        samples[first] + (samples[second] - samples[first]) * frac
    }
}

/// A table is a stack of frames; the oscillator's morph walks from the first to
/// the last, which is the axis a wavetable is played along.
#[derive(Clone)]
pub struct Table {
    pub name: String,
    frames: Vec<Frame>,
}

impl Table {
    fn sample(&self, morph: f32, phase: f32, level: usize) -> f32 {
        // The morph walks this table's own stack, not the maximum one: an imported
        // table can hold fewer frames than the shipped eight, and a morph across a
        // shorter stack must stop at its last frame instead of reading past it.
        let last = self.frames.len().saturating_sub(1);
        if self.frames.is_empty() {
            return 0.0;
        }
        let position = morph.clamp(0.0, 1.0) * last as f32;
        let first = position.floor() as usize;
        let second = (first + 1).min(last);
        let frac = position - first as f32;
        let a = self.frames[first].sample(phase, level);
        let b = self.frames[second].sample(phase, level);
        a + (b - a) * frac
    }
}

/// A harmonic series summed with a rotating phasor rather than a sine per sample:
/// the table is built by one multiply-add per harmonic per sample, so a table with
/// a hundred harmonics costs a hundred adds in place of a hundred sines.
fn synthesize(harmonics: &[f32], size: usize) -> Vec<f32> {
    let mut samples = vec![0.0f32; size];
    for (index, amplitude) in harmonics.iter().enumerate() {
        if *amplitude == 0.0 {
            continue;
        }
        let harmonic = (index + 1) as f64;
        let step = 2.0 * std::f64::consts::PI * harmonic / size as f64;
        let (sin_step, cos_step) = step.sin_cos();
        let (mut sine, mut cosine) = (0.0f64, 1.0f64);
        let amplitude = *amplitude as f64;
        for sample in &mut samples {
            *sample += (amplitude * sine) as f32;
            let next_sine = sine * cos_step + cosine * sin_step;
            cosine = cosine * cos_step - sine * sin_step;
            sine = next_sine;
        }
    }
    samples
}

/// A frame is held at one peak, so walking the morph does not walk the level.
fn standardise(samples: &[f32]) -> Vec<f32> {
    let peak = samples.iter().fold(0.0f32, |peak, sample| peak.max(sample.abs()));
    if peak > 0.0 {
        return samples.iter().map(|sample| sample / peak).collect();
    }
    samples.to_vec()
}

/// Halve a frame: filter it to the half of the band the shorter copy can hold,
/// then take every second sample. The table is periodic, so the convolution wraps
/// and the seam at the end of the frame stays as smooth as the rest of it.
fn decimate(samples: &[f32]) -> Vec<f32> {
    const TAPS: usize = 31;
    let cutoff = 0.45f32;
    let mut kernel = [0.0f32; TAPS];
    let middle = (TAPS as f32 - 1.0) * 0.5;
    for (index, tap) in kernel.iter_mut().enumerate() {
        let n = index as f32 - middle;
        let sinc = if n == 0.0 {
            2.0 * cutoff
        } else {
            (std::f32::consts::TAU * cutoff * n).sin() / (std::f32::consts::PI * n)
        };
        let window = 0.5 - 0.5 * (std::f32::consts::TAU * index as f32 / (TAPS as f32 - 1.0)).cos();
        *tap = sinc * window;
    }
    let total: f32 = kernel.iter().sum();
    for tap in &mut kernel {
        *tap /= total.max(1e-9);
    }
    let length = samples.len();
    let mut half = Vec::with_capacity(length / 2);
    for index in 0..length / 2 {
        let centre = index * 2;
        let mut sum = 0.0f32;
        for (tap, weight) in kernel.iter().enumerate() {
            let at = (centre + length + tap - TAPS / 2) % length;
            sum += samples[at] * weight;
        }
        half.push(sum);
    }
    half
}

/// The four tables the instrument ships. Each frame is a set of harmonic levels,
/// so the shapes are band-limited by construction rather than by a filter that
/// would have to be tuned against them.
fn build_tables() -> Vec<Table> {
    let frames = FRAMES;
    let span = (frames - 1) as f32;
    let mut tables = Vec::with_capacity(TABLES);

    let basic = (0..frames).map(|frame| {
        let steps = frame as f32 / span;
        let harmonics: Vec<f32> = (1..=128).map(|harmonic| {
            if harmonic == 1 {
                return 1.0;
            }
            let odd = harmonic % 2 == 1;
            // Sine to saw to square: the upper series arrives, then the even
            // harmonics leave it, which is the difference between the two shapes.
            let even = if odd { 1.0 } else { 1.0 - steps };
            1.0 / harmonic as f32 * steps * even
        }).collect();
        Frame::build(&harmonics)
    }).collect();
    tables.push(Table { name: "Basic".into(), frames: basic });

    let formant = (0..frames).map(|frame| {
        let steps = frame as f32 / span;
        let centre = 1.0 + steps * 26.0;
        let harmonics: Vec<f32> = (1..=128).map(|harmonic| {
            let distance = (harmonic as f32 - centre) / 7.0;
            (1.0 / harmonic as f32) * (-distance * distance).exp() + 0.02 / harmonic as f32
        }).collect();
        Frame::build(&harmonics)
    }).collect();
    tables.push(Table { name: "Formant".into(), frames: formant });

    let bell = (0..frames).map(|frame| {
        let steps = frame as f32 / span;
        // Sparse partials with no dependence on a fundamental: the shape a struck
        // metal makes rather than a string.
        let partials = [1usize, 3, 4, 7, 9, 13, 18, 24, 31];
        let harmonics: Vec<f32> = (1..=128).map(|harmonic| {
            match partials.iter().position(|partial| *partial == harmonic) {
                Some(slot) => {
                    let tilt = 1.0 / (1.0 + slot as f32 * 0.8);
                    tilt * (1.0 - steps * (slot as f32 / partials.len() as f32)) .max(0.05)
                }
                None => 0.0,
            }
        }).collect();
        Frame::build(&harmonics)
    }).collect();
    tables.push(Table { name: "Bell".into(), frames: bell });

    let vocal = (0..frames).map(|frame| {
        let steps = frame as f32 / span;
        // Two formants: the first opens as the second closes, which is the vowel
        // travelling from one shape to another.
        let low = 3.0 + steps * 2.0;
        let high = 16.0 - steps * 8.0;
        let harmonics: Vec<f32> = (1..=128).map(|harmonic| {
            let first = (harmonic as f32 - low) / 2.2;
            let second = (harmonic as f32 - high) / 3.6;
            (0.6 * (-first * first).exp() + 0.4 * (-second * second).exp() + 0.05 / harmonic as f32)
                * 1.0 / (1.0 + harmonic as f32 * 0.03)
        }).collect();
        Frame::build(&harmonics)
    }).collect();
    tables.push(Table { name: "Vocal".into(), frames: vocal });

    tables
}

/// The shipped tables and the user's, held behind one lock that is read once a
/// frame and never inside the sample loop. A snapshot is an `Arc`, so the audio
/// thread takes a reference count rather than a lock.
fn registry() -> &'static std::sync::RwLock<std::sync::Arc<Vec<Table>>> {
    static REGISTRY: std::sync::OnceLock<std::sync::RwLock<std::sync::Arc<Vec<Table>>>> =
        std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| std::sync::RwLock::new(std::sync::Arc::new(build_tables())))
}

// Table writers serialize disk commits without holding the registry lock during I/O.
fn table_writer() -> &'static std::sync::Mutex<()> {
    static WRITER: std::sync::Mutex<()> = std::sync::Mutex::new(());
    &WRITER
}

fn snapshot() -> std::sync::Arc<Vec<Table>> {
    match registry().read() {
        Ok(tables) => std::sync::Arc::clone(&tables),
        Err(poisoned) => std::sync::Arc::clone(&poisoned.into_inner()),
    }
}

/// How many tables a patch can name. It grows when a user imports one.
pub fn table_count() -> usize {
    match registry().read() {
        Ok(tables) => tables.len(),
        Err(poisoned) => poisoned.into_inner().len(),
    }
}

/// The table names, for the interface and for the patch the engine writes back.
pub fn table_names() -> Vec<String> {
    match registry().read() {
        Ok(tables) => tables.iter().map(|table| table.name.clone()).collect(),
        Err(poisoned) => poisoned
            .into_inner()
            .iter()
            .map(|table| table.name.clone())
            .collect(),
    }
}

/// One cycle of a table as the morph draws it, so the page can draw the shape it is
/// about to play instead of guessing at it. The engine reads the same frame while a
/// note sounds; this is that read, sampled once around the cycle.
pub fn table_shape(table: u8, morph: f32, points: usize) -> Vec<f32> {
    let tables = snapshot();
    let Some(table) = tables.get(table as usize) else {
        return Vec::new();
    };
    let points = points.clamp(8, 2_048);
    (0..points)
        .map(|index| table.sample(morph, index as f32 / points as f32, 0))
        .collect()
}

/// A cycle's harmonic levels, by correlation against a sine and a cosine at each
/// harmonic: the analysis a table import needs, and nothing more.
pub fn analyze_cycle(samples: &[f32], harmonics: usize) -> Vec<f32> {
    let length = samples.len().max(1);
    let harmonics = harmonics.clamp(1, 128);
    let mut levels = Vec::with_capacity(harmonics);
    for harmonic in 1..=harmonics {
        let step = std::f32::consts::TAU * harmonic as f32 / length as f32;
        let (mut sine, mut cosine) = (0.0f32, 0.0f32);
        for (index, sample) in samples.iter().enumerate() {
            let phase = step * index as f32;
            sine += sample * phase.sin();
            cosine += sample * phase.cos();
        }
        // The amplitude of one bin, scaled so a unit sine reads as one.
        levels.push(2.0 * (sine * sine + cosine * cosine).sqrt() / length as f32);
    }
    levels
}

/// Adds a table to the registry. The frames are harmonic levels; each is built into
/// its own mip chain, which is the work an import pays for once.
pub fn add_table(name: String, frames: Vec<Vec<f32>>) -> Result<usize, String> {
    if frames.is_empty() || frames.len() > FRAMES {
        return Err(format!("A table is 1 to {FRAMES} frames."));
    }
    if frames.iter().any(|frame| frame.is_empty() || frame.len() > 128) {
        return Err("A frame is 1 to 128 harmonics.".into());
    }
    let built = Table {
        name: name.clone(),
        frames: frames.iter().map(|frame| Frame::build(frame)).collect(),
    };
    let _writer = table_writer().lock().map_err(|_| "The table writer is locked.".to_string())?;
    let mut registry = registry().write().map_err(|_| "The table list is locked.".to_string())?;
    let mut tables = (**registry).clone();
    // Importing the same name again is an update, not a second copy: a patch that
    // points at that table keeps pointing at it, and a gate run twice does not fill
    // the list with duplicates. The shipped four are never replaced.
    if let Some(index) = tables
        .iter()
        .enumerate()
        .skip(TABLES)
        .find(|(_, table)| table.name == name)
        .map(|(index, _)| index)
    {
        tables[index] = built;
        *registry = std::sync::Arc::new(tables);
        return Ok(index);
    }
    tables.push(built);
    *registry = std::sync::Arc::new(tables);
    Ok(registry.len() - 1)
}

/// Return the individual cycles of an imported table for editing.
pub fn editable_table(table: usize) -> Result<Vec<Vec<f32>>, String> {
    if table < TABLES {
        return Err("Only imported tables can be edited.".into());
    }
    let tables = snapshot();
    let table = tables.get(table).ok_or("That table is unavailable.")?;
    Ok(table.frames.iter().map(|frame| frame.levels[0].clone()).collect())
}

fn build_sample_frames(frames: &[Vec<f32>]) -> Result<Vec<Frame>, String> {
    if frames.is_empty() || frames.len() > FRAMES {
        return Err("An imported table must have one to eight frames.".into());
    }
    if frames.iter().any(|frame| frame.len() != TABLE_SIZE || frame.iter().any(|sample| !sample.is_finite() || sample.abs() > 1.0)) {
        return Err("Each frame must contain 2048 finite samples between -1 and 1.".into());
    }
    Ok(frames.iter().map(|frame| Frame::from_samples(frame)).collect())
}

/// Replace an imported table without changing its index; publish only after saving.
pub fn edit_table_and_persist(
    table: usize,
    frames: Vec<Vec<f32>>,
    persist: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(), String> {
    if table < TABLES {
        return Err("Only imported tables can be edited.".into());
    }
    let built = build_sample_frames(&frames)?;
    let _writer = table_writer().lock().map_err(|_| "The table writer is locked.".to_string())?;
    let mut tables = (*snapshot()).clone();
    let target = tables.get_mut(table).ok_or("That table is unavailable.")?;
    target.frames = built;
    persist(&tables_json(&tables))?;
    *registry().write().map_err(|_| "The table list is locked.".to_string())? = std::sync::Arc::new(tables);
    Ok(())
}

#[cfg(test)]
pub fn edit_table(table: usize, frames: Vec<Vec<f32>>) -> Result<(), String> {
    edit_table_and_persist(table, frames, |_| Ok(()))
}

/// Import or replace a table while preserving the source's spectral phase.
pub fn add_sample_table_and_persist(
    name: String,
    frames: Vec<Vec<f32>>,
    persist: impl FnOnce(&str) -> Result<(), String>,
) -> Result<usize, String> {
    let built = build_sample_frames(&frames)?;
    let _writer = table_writer().lock().map_err(|_| "The table writer is locked.".to_string())?;
    let mut tables = (*snapshot()).clone();
    let existing = tables.iter().enumerate().skip(TABLES).find(|(_, table)| table.name == name).map(|(index, _)| index);
    let index = if let Some(index) = existing {
        tables[index].frames = built;
        index
    } else {
        if tables.len() >= 256 { return Err("The table limit has been reached.".into()); }
        let index = tables.len();
        tables.push(Table { name, frames: built });
        index
    };
    persist(&tables_json(&tables))?;
    *registry().write().map_err(|_| "The table list is locked.".to_string())? = std::sync::Arc::new(tables);
    Ok(index)
}

pub fn add_sample_table(name: String, frames: Vec<Vec<f32>>) -> Result<usize, String> {
    add_sample_table_and_persist(name, frames, |_| Ok(()))
}

/// Keep waveform samples for user tables so spectral phase survives a restart.
fn tables_json(tables: &[Table]) -> String {
    let user: Vec<serde_json::Value> = tables
        .iter()
        .skip(TABLES)
        .map(|table| {
            serde_json::json!({
                "name": table.name,
                "samples": table.frames.iter().map(|frame| &frame.levels[0]).collect::<Vec<_>>(),
            })
        })
        .collect();
    serde_json::json!({ "tables": user }).to_string()
}

#[cfg(test)]
pub fn user_tables_json() -> String {
    tables_json(&snapshot())
}

/// Restores the user's tables from their stored harmonic levels and returns how many
/// were added.
pub fn restore_tables(json: &str) -> usize {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return 0;
    };
    let Some(entries) = value.get("tables").and_then(|tables| tables.as_array()) else {
        return 0;
    };
    let mut added = 0;
    for entry in entries {
        let Some(name) = entry.get("name").and_then(|name| name.as_str()) else {
            continue;
        };
        if let Some(samples) = entry.get("samples").and_then(|samples| samples.as_array()) {
            let frames: Option<Vec<Vec<f32>>> = samples.iter().map(|frame| {
                frame.as_array()?.iter().map(|value| value.as_f64().map(|n| n as f32)).collect()
            }).collect();
            if let Some(frames) = frames {
                if frames.is_empty() || frames.len() > FRAMES || frames.iter().any(|frame| frame.len() != TABLE_SIZE || frame.iter().any(|sample| !sample.is_finite() || sample.abs() > 1.0)) {
                    continue;
                }
                // Preserve original order and old saved-patch indices.
                if add_sample_table(name.to_string(), frames).is_ok() { added += 1; }
            }
            continue;
        }
        let Some(frames) = entry.get("frames").and_then(|frames| frames.as_array()) else {
            continue;
        };
        // The stored frame is already a set of harmonic levels — that is what
        // `user_tables_json` analysed it into — so it is handed straight to the
        // builder. Analysing it a second time would read the levels as samples and
        // rebuild a different table.
        let levels: Vec<Vec<f32>> = frames
            .iter()
            .filter_map(|frame| frame.as_array())
            .map(|frame| {
                frame
                    .iter()
                    .filter_map(|value| value.as_f64().map(|value| value as f32))
                    .collect()
            })
            .collect();
        if add_table(name.to_string(), levels).is_ok() {
            added += 1;
        }
    }
    added
}

/// Reads a table at the resolution the step can carry: a table of `n` samples is
/// good for `n / 2` harmonics, so the reading picks the longest copy that does not
/// overrun the band at this pitch.
fn table_sample(table: &Table, morph: f32, phase: f32, dt: f32) -> f32 {
    let want = (TABLE_SIZE as f32 * dt).max(1.0);
    let level = (want.log2().ceil() as usize).min(TABLE_LEVELS - 1);
    table.sample(morph, phase, level)
}

// ---------- oscillators ----------

/// The correction a saw or a square needs at its own jump, so the harmonics the
/// waveform does not have do not fold back into the band as aliasing.
fn poly_blep(t: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        return 0.0;
    }
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

fn oscillator(wave: Wave, phase: f32, dt: f32) -> f32 {
    match wave {
        Wave::Saw => 2.0 * phase - 1.0 - poly_blep(phase, dt),
        Wave::Square => {
            let naive = if phase < 0.5 { 1.0 } else { -1.0 };
            naive + poly_blep(phase, dt) - poly_blep((phase + 0.5) % 1.0, dt)
        }
        // A triangle's harmonics fall as the square of their number, so the
        // naive shape is already quiet above the band.
        Wave::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
        Wave::Sine => (2.0 * std::f32::consts::PI * phase).sin(),
        // A table is read by `table_sample`, which needs the morph and the step to
        // choose its resolution: asking this shape for a sample would lose both.
        Wave::Table => 0.0,
    }
}

// ---------- events ----------

/// What the interface, the keyboard and the MIDI input all send the engine.
/// Every event is plain data, so the audio thread's queue can hold them without
/// touching an allocation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    NoteOn { note: u8, velocity: f32 },
    NoteOff { note: u8 },
    AllNotesOff,
    /// In semitones, ±2 at the wheel's ends.
    Bend(f32),
    /// Scales the LFO's depth, 0 at the bottom of the wheel and 1 at the top.
    ModWheel(f32),
    /// One note's own bend, as MPE sends it: ±48 semitones of travel, though a
    /// controller's default is ±2.
    NoteBend { note: u8, semitones: f32 },
    /// One note's aftertouch, 0 to 1.
    Pressure { note: u8, amount: f32 },
    MpeNoteOn { channel: u8, zone: u8, note: u8, velocity: f32, bend: f32, pressure: f32, timbre: f32 },
    MpeNoteOff { channel: u8, note: u8 },
    MpeBend { channel: u8, semitones: f32 },
    MpePressure { channel: u8, amount: f32 },
    MpeTimbre { channel: u8, amount: f32 },
    MpeMasterBend { zone: u8, semitones: f32 },
    MpeMasterPressure { zone: u8, amount: f32 },
    MpeMasterTimbre { zone: u8, amount: f32 },
    MpeMasterWheel { zone: u8, amount: f32 },
}

// ---------- voices ----------

struct Voice {
    note: u8,
    channel: Option<u8>,
    zone: Option<u8>,
    timbre: f32,
    velocity: f32,
    /// One phase per unison copy per oscillator. The copies drift apart because
    /// their steps differ, which is what makes a stack sound wide rather than loud.
    phase: [[f32; MAX_UNISON]; 2],
    sub_phase: f32,
    noise: u32,
    amp: EnvelopeState,
    filter_env: EnvelopeState,
    filter_l: ChannelFilter,
    filter_r: ChannelFilter,
    /// Where the note is, and where the glide is taking it.
    frequency: f32,
    target: f32,
    pan: f32,
    /// How far the matrix moved the voice across the image this frame.
    pan_offset: f32,
    /// This note's own bend, from an MPE controller or a channel's wheel.
    bend: f32,
    /// This note's own aftertouch.
    pressure: f32,
    age: u64,
}

impl Voice {
    fn new() -> Self {
        Self {
            note: 60,
            channel: None,
            zone: None,
            timbre: 0.0,
            velocity: 0.0,
            phase: [[0.0; MAX_UNISON]; 2],
            sub_phase: 0.0,
            noise: 0x9e37_79b9,
            amp: EnvelopeState::new(),
            filter_env: EnvelopeState::new(),
            filter_l: ChannelFilter::default(),
            filter_r: ChannelFilter::default(),
            frequency: 440.0,
            target: 440.0,
            pan: 0.0,
            pan_offset: 0.0,
            bend: 0.0,
            pressure: 0.0,
            age: 0,
        }
    }

    fn idle(&self) -> bool {
        !self.amp.active()
    }

    fn start(&mut self, note: u8, velocity: f32, pan: f32, age: u64, from: Option<f32>) {
        // A voice that is already sounding keeps its phase, its filter and its
        // envelope height when it is retriggered or stolen: resetting them would
        // step the waveform to zero and be heard as a click on every repeated key.
        let sounding = self.amp.active();
        let level = if sounding { self.amp.level } else { 0.0 };
        let filter_level = if sounding { self.filter_env.level } else { 0.0 };
        self.note = note;
        self.velocity = velocity.clamp(0.0, 1.0);
        if !sounding {
            // Every copy starts somewhere else around the cycle: eight copies all
            // leaving zero sum to one loud transient instead of a stack. The seed
            // is the note, so the same take renders the same frames twice.
            let mut seed = (note as u32).wrapping_mul(2_654_435_761) ^ 0x9e37_79b9;
            for oscillator in 0..2 {
                for copy in 0..MAX_UNISON {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    self.phase[oscillator][copy] = (seed >> 8) as f32 / 16_777_216.0;
                }
            }
            self.sub_phase = 0.0;
            self.filter_l.reset();
            self.filter_r.reset();
        }
        self.bend = 0.0;
        self.pressure = 0.0;
        self.noise = 0x9e37_79b9 ^ (note as u32).wrapping_mul(2_654_435_761);
        self.amp = EnvelopeState {
            level,
            stage: Stage::Attack,
        };
        self.filter_env = EnvelopeState {
            level: filter_level,
            stage: Stage::Attack,
        };
        self.target = midi_hz(note);
        // A glide starts where the last note was; without a note to come from, the
        // voice starts on its own pitch.
        self.frequency = from.unwrap_or(self.target);
        self.pan = pan;
        self.age = age;
        self.amp.start();
        self.filter_env.start();
    }

    fn stop(&mut self) {
        self.amp.release();
        self.filter_env.release();
    }

    fn frame(
        &mut self,
        config: &SynthConfig,
        sources: Sources,
        bend: f32,
        tables: &[Table],
    ) -> [f32; 2] {
        if !self.amp.active() {
            return [0.0; 2];
        }
        self.amp.advance(&config.amp);
        self.filter_env.advance(&config.filter_env);
        if !self.amp.active() {
            return [0.0; 2];
        }
        // The envelopes are the matrix's per-voice sources, so they are read after
        // they advance: a route hears the height the sample is about to use.
        let sources = Sources {
            env1: self.amp.level,
            env2: self.filter_env.level,
            ..sources
        };
        let mods = Modulation::of(config, &sources);
        self.pan_offset = mods.pan;

        let glide = config.glide_ms.max(0.0);
        let coef = if glide <= 0.0 {
            1.0
        } else {
            1.0 - (-1.0 / (glide * 0.001 * RATE)).exp()
        };
        // A tuning is an offset in cents on the key, so it arrives before the glide
        // does: the note is the key's own pitch, wherever the scale puts it.
        let offset = config
            .tuning
            .get(self.note as usize)
            .copied()
            .unwrap_or(0.0);
        let tuned = self.target * (offset / 1200.0).exp2();
        self.frequency += (tuned - self.frequency) * coef;

        // The key's own pitch, moved by the wheel, by either LFO's pitch
        // destination, and by the matrix — all in semitones before the ratio.
        let lfo_pitch = if config.lfo.target == LfoTarget::Pitch {
            config.lfo.depth * 2.0 * sources.lfo1
        } else {
            0.0
        } + if config.lfo2.target == LfoTarget::Pitch {
            config.lfo2.depth * 2.0 * sources.lfo2
        } else {
            0.0
        };
        let pitch = bend + self.bend + lfo_pitch + 12.0 * mods.pitch;
        // Duplicate voices an octave apart would otherwise beat against each other
        // at DC once their phases line up, so each phase keeps its own drift.
        let base = self.frequency * (pitch / 12.0).exp2() / RATE;
        let levels = [
            (config.osc1.level + mods.osc1).clamp(0.0, 1.0),
            (config.osc2.level + mods.osc2).clamp(0.0, 1.0),
        ];

        let mut left = 0.0;
        let mut right = 0.0;
        for (index, osc) in [&config.osc1, &config.osc2].iter().enumerate() {
            let unison = osc.unison.clamp(1, MAX_UNISON as u8) as usize;
            if levels[index] <= 0.0 {
                continue;
            }
            let ratio = (osc.octave as f32 + osc.semitone as f32 / 12.0 + osc.cents / 1200.0).exp2();
            // Eight copies at full level would be eight times as loud, so the stack
            // is divided by its own count's root: wider, not louder.
            let gain = levels[index] / (unison as f32).sqrt();
            for copy in 0..unison {
                // Where this copy sits in the stack, -1 at one edge and 1 at the other.
                let position = if unison == 1 {
                    0.0
                } else {
                    copy as f32 / (unison - 1) as f32 * 2.0 - 1.0
                };
                let detune = (position * osc.detune / 1200.0).exp2();
                let dt = (base * ratio * detune).clamp(0.0, 0.45);
                let phase = self.phase[index][copy];
                let sample = match osc.wave {
                    Wave::Table => {
                        let table = &tables[osc.table as usize % tables.len().max(1)];
                        table_sample(table, osc.morph, phase, dt)
                    }
                    other => oscillator(other, phase, dt),
                };
                self.phase[index][copy] = (phase + dt) % 1.0;
                // Equal power across the image, normalised so a centred copy is
                // heard at the level a mono one was: the stack widens the sound
                // without a dip in the middle.
                let angle = (position * osc.spread + 1.0) * 0.25 * std::f32::consts::PI;
                left += sample * angle.cos() * gain * std::f32::consts::SQRT_2;
                right += sample * angle.sin() * gain * std::f32::consts::SQRT_2;
            }
        }
        let noise = (config.noise + mods.noise).clamp(0.0, 1.0);
        if noise > 0.0 {
            self.noise ^= self.noise << 13;
            self.noise ^= self.noise >> 17;
            self.noise ^= self.noise << 5;
            let white = ((self.noise >> 8) as f32 / 8_388_608.0 - 1.0) * noise;
            left += white;
            right += white;
        }
        // The sub is a square one octave under oscillator one, which is the sound
        // a bass patch is built on. It keeps its own phase so it tracks the note
        // without locking to oscillator one's waveform.
        let sub = (config.sub + mods.sub).clamp(0.0, 1.0);
        if sub > 0.0 {
            let sub_dt = (base * 0.5).clamp(0.0, 0.45);
            let sample = oscillator(Wave::Square, self.sub_phase, sub_dt) * sub * 0.5;
            self.sub_phase = (self.sub_phase + sub_dt) % 1.0;
            left += sample;
            right += sample;
        }

        let lfo_filter = if config.lfo.target == LfoTarget::Filter {
            config.lfo.depth * 4.0 * sources.lfo1
        } else {
            0.0
        } + if config.lfo2.target == LfoTarget::Filter {
            config.lfo2.depth * 4.0 * sources.lfo2
        } else {
            0.0
        } + 4.0 * mods.cutoff;
        let key = config.filter.key_track * (self.note as f32 - 60.0) / 12.0;
        let sweep = config.filter.env_amount * 4.0 * self.filter_env.level;
        let cutoff = config.filter.cutoff * (key + sweep + lfo_filter).exp2();
        let resonance = (config.filter.resonance + mods.resonance).clamp(0.0, 1.0);
        let q = 0.5 + resonance * 9.5;
        // Drive is an input gain the nonlinear model hears and the linear ones are
        // compensated for, so the knob changes character rather than just level.
        let pre = 1.0 + config.filter.drive * 6.0;
        let post = 1.0 / (1.0 + config.filter.drive * 3.0);
        let filtered_left = self
            .filter_l
            .process(left * pre, cutoff, q, resonance, config.filter.kind)
            * post;
        let filtered_right = self
            .filter_r
            .process(right * pre, cutoff, q, resonance, config.filter.kind)
            * post;

        let tremolo = (if config.lfo.target == LfoTarget::Amp {
            config.lfo.depth * (0.5 - 0.5 * sources.lfo1)
        } else {
            0.0
        }) + if config.lfo2.target == LfoTarget::Amp {
            config.lfo2.depth * (0.5 - 0.5 * sources.lfo2)
        } else {
            0.0
        };
        let amp = self.amp.level * self.velocity * (1.0 - tremolo).max(0.0);
        [filtered_left * amp, filtered_right * amp]
    }
}

/// One LFO cycle's value at `phase`, in -1 to 1.
fn lfo_value(wave: LfoWave, phase: f32) -> f32 {
    match wave {
        LfoWave::Sine => (2.0 * std::f32::consts::PI * phase).sin(),
        LfoWave::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
    }
}

/// Equal temperament from A440, which is what every keyboard and every tuner in
/// the room is tuned to.
pub fn midi_hz(note: u8) -> f32 {
    440.0 * (2.0_f32).powf((note as f32 - 69.0) / 12.0)
}

// ---------- the instrument ----------

pub struct Synth {
    voices: Vec<Voice>,
    lfo_phase: f32,
    lfo2_phase: f32,
    bend: f32,
    wheel: f32,
    zone_bend: [f32; 2],
    zone_pressure: [f32; 2],
    zone_timbre: [f32; 2],
    zone_wheel: [f32; 2],
    age: u64,
}

impl Default for Synth {
    fn default() -> Self {
        Self::new()
    }
}

impl Synth {
    pub fn new() -> Self {
        Self {
            voices: (0..VOICES).map(|_| Voice::new()).collect(),
            lfo_phase: 0.0,
            lfo2_phase: 0.0,
            bend: 0.0,
            wheel: 1.0,
            zone_bend: [0.0; 2],
            zone_pressure: [0.0; 2],
            zone_timbre: [0.0; 2],
            zone_wheel: [1.0; 2],
            age: 0,
        }
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|voice| !voice.idle()).count()
    }

    pub fn note_on(&mut self, note: u8, velocity: f32) {
        self.start_note(note, velocity, None, None);
    }

    fn start_note(&mut self, note: u8, velocity: f32, channel: Option<u8>, zone: Option<u8>) {
        let note = note.min(127);
        // A repeated note retriggers the voice that already holds it rather than
        // stacking a second one, so holding a key twice is not twice as loud.
        let reuse = self
            .voices
            .iter()
            .position(|voice| !voice.idle() && voice.note == note && voice.channel == channel);
        let index = reuse.unwrap_or_else(|| {
            let free = self.voices.iter().position(Voice::idle);
            let oldest_release = self
                .voices
                .iter()
                .enumerate()
                .filter(|(_, voice)| voice.amp.stage == Stage::Release)
                .min_by_key(|(_, voice)| voice.age)
                .map(|(index, _)| index);
            let oldest = self
                .voices
                .iter()
                .enumerate()
                .min_by_key(|(_, voice)| voice.age)
                .map(|(index, _)| index)
                .unwrap_or(0);
            free.or(oldest_release).unwrap_or(oldest)
        });
        let pan = if index % 2 == 0 { -0.22 } else { 0.22 };
        let from = self
            .voices
            .iter()
            .filter(|voice| !voice.idle())
            .max_by_key(|voice| voice.age)
            .map(|voice| voice.frequency);
        self.age += 1;
        let age = self.age;
        self.voices[index].start(note, velocity, pan, age, from);
        self.voices[index].channel = channel;
        self.voices[index].zone = zone;
        self.voices[index].timbre = 0.0;
    }

    pub fn note_off(&mut self, note: u8) {
        for voice in &mut self.voices {
            if !voice.idle() && voice.note == note && voice.channel.is_none() {
                voice.stop();
            }
        }
    }

    pub fn all_notes_off(&mut self) {
        for voice in &mut self.voices {
            voice.stop();
        }
    }

    pub fn set_bend(&mut self, semitones: f32) {
        self.bend = semitones.clamp(-2.0, 2.0);
    }

    pub fn set_mod(&mut self, amount: f32) {
        self.wheel = amount.clamp(0.0, 1.0);
    }

    /// A bend that belongs to one note, which is what MPE is: every finger has its
    /// own pitch, not the channel's.
    pub fn note_bend(&mut self, note: u8, semitones: f32) {
        for voice in &mut self.voices {
            if !voice.idle() && voice.note == note {
                voice.bend = semitones.clamp(-48.0, 48.0);
            }
        }
    }

    /// Aftertouch for one note, 0 to 1.
    pub fn set_pressure(&mut self, note: u8, amount: f32) {
        for voice in &mut self.voices {
            if !voice.idle() && voice.note == note {
                voice.pressure = amount.clamp(0.0, 1.0);
            }
        }
    }

    /// Apply the same event path for the live output and offline bounce.
    pub fn apply_event(&mut self, event: Event) {
        match event {
            Event::NoteOn { note, velocity } => self.note_on(note, velocity),
            Event::NoteOff { note } => self.note_off(note),
            Event::AllNotesOff => { self.all_notes_off(); self.zone_bend = [0.0; 2]; self.zone_pressure = [0.0; 2]; self.zone_timbre = [0.0; 2]; self.zone_wheel = [1.0; 2]; }
            Event::Bend(value) => self.set_bend(value),
            Event::ModWheel(value) => self.set_mod(value),
            Event::NoteBend { note, semitones } => self.note_bend(note, semitones),
            Event::Pressure { note, amount } => self.set_pressure(note, amount),
            Event::MpeNoteOn { channel, zone, note, velocity, bend, pressure, timbre } if channel < 16 && zone < 2 => {
                for voice in &mut self.voices {
                    if voice.channel == Some(channel) && !voice.idle() { voice.stop(); }
                }
                self.start_note(note, velocity, Some(channel), Some(zone));
                if let Some(voice) = self.voices.iter_mut().filter(|voice| voice.channel == Some(channel) && voice.note == note).max_by_key(|voice| voice.age) {
                    voice.bend = bend.clamp(-48.0, 48.0);
                    voice.pressure = pressure.clamp(0.0, 1.0);
                    voice.timbre = timbre.clamp(0.0, 1.0);
                }
            }
            Event::MpeNoteOff { channel, note } => {
                for voice in &mut self.voices { if voice.channel == Some(channel) && voice.note == note && !voice.idle() { voice.stop(); } }
            }
            Event::MpeBend { channel, semitones } => {
                for voice in &mut self.voices { if voice.channel == Some(channel) && !voice.idle() && voice.amp.stage != Stage::Release { voice.bend = semitones.clamp(-48.0, 48.0); } }
            }
            Event::MpePressure { channel, amount } => {
                for voice in &mut self.voices { if voice.channel == Some(channel) && !voice.idle() && voice.amp.stage != Stage::Release { voice.pressure = amount.clamp(0.0, 1.0); } }
            }
            Event::MpeTimbre { channel, amount } => {
                for voice in &mut self.voices { if voice.channel == Some(channel) && !voice.idle() && voice.amp.stage != Stage::Release { voice.timbre = amount.clamp(0.0, 1.0); } }
            }
            Event::MpeMasterBend { zone, semitones } if zone < 2 => self.zone_bend[zone as usize] = semitones.clamp(-48.0, 48.0),
            Event::MpeMasterPressure { zone, amount } if zone < 2 => self.zone_pressure[zone as usize] = amount.clamp(0.0, 1.0),
            Event::MpeMasterTimbre { zone, amount } if zone < 2 => self.zone_timbre[zone as usize] = amount.clamp(0.0, 1.0),
            Event::MpeMasterWheel { zone, amount } if zone < 2 => self.zone_wheel[zone as usize] = amount.clamp(0.0, 1.0),
            _ => {}
        }
    }

    /// One stereo frame. The voices are summed, panned across two positions so a
    /// chord has width, and held at full scale: louder is the limiter's job.
    pub fn frame(&mut self, config: &SynthConfig) -> [f32; 2] {
        // One snapshot a frame: the audio thread never waits on a table import.
        let tables = snapshot();
        let increment = config.lfo.rate.max(0.05) / RATE;
        self.lfo_phase = (self.lfo_phase + increment) % 1.0;
        let second = config.lfo2.rate.max(0.05) / RATE;
        self.lfo2_phase = (self.lfo2_phase + second) % 1.0;
        let lfo1 = lfo_value(config.lfo.wave, self.lfo_phase);
        let lfo2 = lfo_value(config.lfo2.wave, self.lfo2_phase);
        let shared = Sources {
            lfo1: lfo1 * self.wheel,
            lfo2: lfo2 * self.wheel,
            env1: 0.0,
            env2: 0.0,
            velocity: 1.0,
            key: 0.0,
            wheel: self.wheel,
            pressure: 0.0,
            timbre: 0.0,
            macros: config.macros,
        };
        let mut left = 0.0;
        let mut right = 0.0;
        for voice in &mut self.voices {
            let wheel = voice.zone.map(|zone| self.zone_wheel[zone as usize]).unwrap_or(self.wheel);
            let per_voice = Sources {
                lfo1: lfo1 * wheel,
                lfo2: lfo2 * wheel,
                velocity: voice.velocity,
                key: (voice.note as f32 - 60.0) / 36.0,
                pressure: (voice.pressure + voice.zone.map(|zone| self.zone_pressure[zone as usize]).unwrap_or(0.0)).clamp(0.0, 1.0),
                timbre: (voice.timbre + voice.zone.map(|zone| self.zone_timbre[zone as usize]).unwrap_or(0.0)).clamp(0.0, 1.0),
                wheel,
                ..shared
            };
            let bend = voice.zone.map(|zone| self.zone_bend[zone as usize]).unwrap_or(self.bend);
            let sample = voice.frame(config, per_voice, bend, &tables);
            if sample[0] == 0.0 && sample[1] == 0.0 {
                continue;
            }
            let angle = (voice.pan + voice.pan_offset + 1.0).clamp(0.0, 2.0) * 0.25 * std::f32::consts::PI;
            left += sample[0] * angle.cos();
            right += sample[1] * angle.sin();
        }
        let gain = config.level * 0.35;
        [
            (left * gain).clamp(-1.0, 1.0),
            (right * gain).clamp(-1.0, 1.0),
        ]
    }

    #[cfg(test)]
    pub fn render(&mut self, frames: usize, config: &SynthConfig) -> Vec<[f32; 2]> {
        let mut output = Vec::with_capacity(frames);
        for _ in 0..frames {
            output.push(self.frame(config));
        }
        output
    }
}

/// A whole take, rendered from a list of events and their frame positions. This
/// is the same code the live stream runs, which is what lets a bounce be the
/// sound that was heard rather than a second guess at it.
pub fn render_events(
    config: &SynthConfig,
    events: &[(u64, Event)],
    frames: usize,
) -> Vec<[f32; 2]> {
    let mut synth = Synth::new();
    let mut output = Vec::with_capacity(frames);
    let mut next = 0usize;
    for frame in 0..frames {
        while let Some((at, event)) = events.get(next) {
            if *at > frame as u64 {
                break;
            }
            synth.apply_event(*event);
            next += 1;
        }
        output.push(synth.frame(config));
    }
    output
}

/// The patches a new install starts with. They are written here rather than read
/// from a file so a fresh install has something to play before anything is saved.
pub fn builtin_presets() -> Vec<(&'static str, &'static str, SynthConfig)> {
    let init = SynthConfig::default();
    let mut sub_bass = SynthConfig {
        level: 0.75,
        filter: Filter {
            kind: FilterKind::Lowpass,
            cutoff: 420.0,
            resonance: 0.25,
            env_amount: 0.6,
            key_track: 0.2,
            drive: 0.0,
        },
        amp: Envelope {
            attack: 0.002,
            decay: 0.3,
            sustain: 0.85,
            release: 0.18,
        },
        filter_env: Envelope {
            attack: 0.001,
            decay: 0.25,
            sustain: 0.1,
            release: 0.15,
        },
        ..init.clone()
    };
    sub_bass.osc1 = Oscillator {
        wave: Wave::Saw,
        octave: -2,
        level: 0.9,
        ..Oscillator::default()
    };
    sub_bass.osc2 = Oscillator {
        wave: Wave::Square,
        octave: -2,
        cents: -6.0,
        level: 0.35,
        ..Oscillator::default()
    };
    sub_bass.sub = 0.6;

    let mut pad = SynthConfig {
        level: 0.55,
        glide_ms: 60.0,
        filter: Filter {
            kind: FilterKind::Lowpass,
            cutoff: 900.0,
            resonance: 0.2,
            env_amount: 0.9,
            key_track: 0.45,
            drive: 0.0,
        },
        amp: Envelope {
            attack: 0.9,
            decay: 1.4,
            sustain: 0.8,
            release: 2.2,
        },
        filter_env: Envelope {
            attack: 1.6,
            decay: 2.0,
            sustain: 0.45,
            release: 2.5,
        },
        lfo: Lfo {
            wave: LfoWave::Sine,
            rate: 0.35,
            depth: 0.18,
            target: LfoTarget::Filter,
        },
        ..SynthConfig::default()
    };
    pad.osc1 = Oscillator {
        wave: Wave::Saw,
        cents: -9.0,
        level: 0.55,
        ..Oscillator::default()
    };
    pad.osc2 = Oscillator {
        wave: Wave::Saw,
        octave: 0,
        cents: 9.0,
        level: 0.55,
        ..Oscillator::default()
    };

    let mut pluck = SynthConfig {
        level: 0.7,
        filter: Filter {
            kind: FilterKind::Lowpass,
            cutoff: 1_200.0,
            resonance: 0.45,
            env_amount: 1.6,
            key_track: 0.7,
            drive: 0.0,
        },
        amp: Envelope {
            attack: 0.001,
            decay: 0.28,
            sustain: 0.0,
            release: 0.22,
        },
        filter_env: Envelope {
            attack: 0.001,
            decay: 0.2,
            sustain: 0.0,
            release: 0.2,
        },
        ..SynthConfig::default()
    };
    pluck.osc1 = Oscillator {
        wave: Wave::Square,
        level: 0.7,
        ..Oscillator::default()
    };
    pluck.osc2 = Oscillator {
        wave: Wave::Triangle,
        semitone: 7,
        level: 0.3,
        ..Oscillator::default()
    };

    let mut lead = SynthConfig {
        level: 0.62,
        glide_ms: 45.0,
        filter: Filter {
            kind: FilterKind::Lowpass,
            cutoff: 4_800.0,
            resonance: 0.35,
            env_amount: 0.5,
            key_track: 0.6,
            drive: 0.0,
        },
        amp: Envelope {
            attack: 0.008,
            decay: 0.12,
            sustain: 0.9,
            release: 0.25,
        },
        lfo: Lfo {
            wave: LfoWave::Sine,
            rate: 5.6,
            depth: 0.12,
            target: LfoTarget::Pitch,
        },
        ..SynthConfig::default()
    };
    lead.osc1 = Oscillator {
        wave: Wave::Saw,
        level: 0.7,
        ..Oscillator::default()
    };
    lead.osc2 = Oscillator {
        wave: Wave::Saw,
        octave: 1,
        cents: 4.0,
        level: 0.22,
        ..Oscillator::default()
    };

    let mut sweep = SynthConfig {
        level: 0.5,
        noise: 0.7,
        filter: Filter {
            kind: FilterKind::Bandpass,
            cutoff: 400.0,
            resonance: 0.55,
            env_amount: 0.0,
            key_track: 0.0,
            drive: 0.0,
        },
        amp: Envelope {
            attack: 1.2,
            decay: 2.0,
            sustain: 0.6,
            release: 1.8,
        },
        lfo: Lfo {
            wave: LfoWave::Triangle,
            rate: 0.12,
            depth: 0.55,
            target: LfoTarget::Filter,
        },
        ..SynthConfig::default()
    };
    sweep.osc1.level = 0.15;
    sweep.osc2.level = 0.0;

    // The stack is what unison is for: seven copies, detuned and spread, on one
    // note. It is a patch no single oscillator can make.
    let mut stack = SynthConfig {
        level: 0.5,
        filter: Filter {
            kind: FilterKind::Ladder,
            cutoff: 2_400.0,
            resonance: 0.25,
            env_amount: 0.5,
            key_track: 0.4,
            drive: 0.0,
        },
        amp: Envelope {
            attack: 0.35,
            decay: 1.2,
            sustain: 0.85,
            release: 1.6,
        },
        ..SynthConfig::default()
    };
    stack.osc1 = Oscillator {
        wave: Wave::Saw,
        unison: 7,
        detune: 18.0,
        spread: 0.9,
        level: 0.7,
        ..Oscillator::default()
    };
    stack.osc2 = Oscillator {
        wave: Wave::Saw,
        octave: -1,
        unison: 5,
        detune: 11.0,
        spread: 0.6,
        level: 0.35,
        ..Oscillator::default()
    };
    stack.mods = vec![
        Mod { source: ModSource::Env2, target: ModTarget::Cutoff, amount: 0.6, curve: ModCurve::Log },
        Mod { source: ModSource::Lfo2, target: ModTarget::Pan, amount: 0.25, curve: ModCurve::Linear },
    ];

    // A table reads its own frames, so a bell is a shape the oscillator does not
    // have to be able to draw.
    let mut bell = SynthConfig {
        level: 0.55,
        filter: Filter {
            kind: FilterKind::Lowpass,
            cutoff: 6_500.0,
            resonance: 0.2,
            env_amount: 0.4,
            key_track: 0.5,
            drive: 0.0,
        },
        amp: Envelope {
            attack: 0.004,
            decay: 1.6,
            sustain: 0.05,
            release: 1.4,
        },
        lfo2: Lfo {
            wave: LfoWave::Sine,
            rate: 0.25,
            depth: 0.2,
            target: LfoTarget::Filter,
        },
        ..SynthConfig::default()
    };
    bell.osc1 = Oscillator {
        wave: Wave::Table,
        table: 2,
        morph: 0.25,
        level: 0.7,
        ..Oscillator::default()
    };
    bell.osc2 = Oscillator {
        wave: Wave::Table,
        table: 2,
        morph: 0.6,
        octave: 1,
        level: 0.2,
        ..Oscillator::default()
    };

    vec![
        ("builtin:init", "Init saw", init),
        ("builtin:stack", "Wide stack", stack),
        ("builtin:bell", "Table bell", bell),
        ("builtin:sub", "Sub bass", sub_bass),
        ("builtin:pad", "Warm pad", pad),
        ("builtin:pluck", "Pluck", pluck),
        ("builtin:lead", "Glass lead", lead),
        ("builtin:noise", "Noise sweep", sweep),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(config: &SynthConfig, note: u8, frames: usize) -> Vec<[f32; 2]> {
        render_events(
            config,
            &[(0, Event::NoteOn { note, velocity: 1.0 })],
            frames,
        )
    }

    fn mono(frames: &[[f32; 2]]) -> Vec<f32> {
        frames.iter().map(|frame| (frame[0] + frame[1]) * 0.5).collect()
    }

    /// Upward zero crossings over the window, which is the frequency for a
    /// waveform with one crossing per cycle.
    fn crossings(samples: &[f32]) -> usize {
        samples
            .windows(2)
            .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
            .count()
    }

    /// The frequency a window holds. The count is scaled by the window's own
    /// length: a slice of audio is not a second, and counting it as one is how a
    /// correct engine looks broken.
    fn hz(samples: &[f32]) -> f32 {
        let seconds = samples.len() as f32 / RATE;
        crossings(samples) as f32 / seconds.max(1e-6)
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|value| value * value).sum::<f32>() / samples.len() as f32).sqrt()
    }

    /// How much of a signal sits at one frequency, by Goertzel: one bin of a
    /// transform is enough to say whether a filter lifted the band it was tuned to.
    fn energy_at(samples: &[f32], frequency: f32) -> f32 {
        let omega = std::f32::consts::TAU * frequency / RATE;
        let coefficient = 2.0 * omega.cos();
        let (mut first, mut second) = (0.0f32, 0.0f32);
        for sample in samples {
            let value = sample + coefficient * first - second;
            second = first;
            first = value;
        }
        (first * first + second * second - coefficient * first * second).max(0.0).sqrt()
            / samples.len() as f32
    }

    #[test]
    fn mpe_keeps_same_pitch_notes_and_released_tails_separate_by_channel() {
        let mut synth = Synth::new();
        let note = |channel, note| Event::MpeNoteOn { channel, zone: 0, note, velocity: 1.0, bend: 0.0, pressure: 0.0, timbre: 0.0 };
        synth.apply_event(note(1, 60));
        synth.apply_event(note(2, 60));
        synth.apply_event(Event::MpeBend { channel: 1, semitones: 12.0 });
        synth.apply_event(Event::MpePressure { channel: 1, amount: 0.5 });
        synth.apply_event(Event::MpeTimbre { channel: 1, amount: 0.7 });
        let first = synth.voices.iter().find(|voice| voice.channel == Some(1)).unwrap();
        let second = synth.voices.iter().find(|voice| voice.channel == Some(2)).unwrap();
        assert_eq!((first.bend, first.pressure, first.timbre), (12.0, 0.5, 0.7));
        assert_eq!((second.bend, second.pressure, second.timbre), (0.0, 0.0, 0.0));
        synth.apply_event(Event::MpeNoteOff { channel: 1, note: 60 });
        synth.apply_event(note(1, 62));
        synth.apply_event(Event::MpeBend { channel: 1, semitones: -4.0 });
        synth.apply_event(Event::MpePressure { channel: 1, amount: 0.9 });
        let tail = synth.voices.iter().find(|voice| voice.channel == Some(1) && voice.note == 60).unwrap();
        let held = synth.voices.iter().find(|voice| voice.channel == Some(1) && voice.note == 62).unwrap();
        assert_eq!(tail.amp.stage, Stage::Release);
        assert_eq!((tail.bend, tail.pressure), (12.0, 0.5));
        assert_eq!((held.bend, held.pressure), (-4.0, 0.9));
        assert_ne!(synth.voices.iter().find(|voice| voice.channel == Some(2)).unwrap().amp.stage, Stage::Release);
    }

    #[test]
    fn mpe_expression_reaches_the_modulation_matrix_and_offline_render() {
        let mut config = SynthConfig::default();
        config.mods = vec![
            Mod { source: ModSource::Pressure, target: ModTarget::Pitch, amount: 0.5, curve: ModCurve::Linear },
            Mod { source: ModSource::Timbre, target: ModTarget::Cutoff, amount: 0.5, curve: ModCurve::Linear },
        ];
        let render = |pressure, timbre| render_events(&config, &[
            (0, Event::MpeMasterPressure { zone: 0, amount: pressure }),
            (0, Event::MpeNoteOn { channel: 1, zone: 0, note: 69, velocity: 1.0, bend: 0.0, pressure: 0.0, timbre }),
        ], 4000);
        let baseline = render(0.0, 0.0);
        assert_ne!(baseline, render(1.0, 0.0));
        assert_ne!(baseline, render(0.0, 1.0));
        assert!(baseline.iter().flatten().any(|value| value.abs() > 0.001));
    }

    #[test]
    fn mpe_zone_bend_and_lfo_wheel_are_not_affected_by_the_ordinary_keyboard() {
        let mut config = SynthConfig::default();
        config.lfo.target = LfoTarget::Pitch;
        config.lfo.depth = 1.0;
        let render = |global_wheel, global_bend, zone_wheel| {
            render_events(&config, &[
                (0, Event::ModWheel(global_wheel)),
                (0, Event::Bend(global_bend)),
                (0, Event::MpeMasterWheel { zone: 0, amount: zone_wheel }),
                (0, Event::MpeNoteOn { channel: 1, zone: 0, note: 69, velocity: 1.0, bend: 0.0, pressure: 0.0, timbre: 0.0 }),
            ], 4000)
        };
        let baseline = render(0.0, 0.0, 1.0);
        assert_eq!(baseline, render(1.0, 2.0, 1.0));
        assert_ne!(baseline, render(0.0, 0.0, 0.0));
    }

    #[test]
    fn a_held_a_is_four_hundred_and_forty_hertz() {
        let mut config = SynthConfig::default();
        config.osc2.level = 0.0;
        config.noise = 0.0;
        config.filter.cutoff = 18_000.0;
        config.filter.env_amount = 0.0;
        config.amp.attack = 0.001;
        let rendered = tone(&config, 69, 24_000);
        let samples = mono(&rendered);
        // A second of audio, counted after the attack has settled.
        let counted = hz(&samples[4_800..]);
        assert!(
            (439.0..=441.0).contains(&counted),
            "A4 came out at about {counted} Hz"
        );
    }

    #[test]
    fn an_octave_up_is_twice_the_frequency() {
        let mut config = SynthConfig::default();
        config.osc2.level = 0.0;
        config.filter.cutoff = 18_000.0;
        config.filter.env_amount = 0.0;
        let low = hz(&mono(&tone(&config, 69, 24_000))[4_800..]);
        let high = hz(&mono(&tone(&config, 81, 24_000))[4_800..]);
        assert!(
            (high - low * 2.0).abs() <= 4.0,
            "A5 was {high} Hz against A4's {low} Hz"
        );
    }

    #[test]
    fn a_tuning_moves_the_key_it_names() {
        let mut config = SynthConfig::default();
        config.osc2.level = 0.0;
        config.filter.cutoff = 18_000.0;
        config.filter.env_amount = 0.0;
        let equal = hz(&mono(&tone(&config, 69, 24_000))[4_800..]);

        // A4 raised a semitone: 440 × 2^(100/1200) = 466.16 Hz, and no other key
        // has moved, so B4 is still where it started.
        config.tuning = vec![0.0; 128];
        config.tuning[69] = 100.0;
        let tuned = hz(&mono(&tone(&config, 69, 24_000))[4_800..]);
        assert!(
            (tuned - equal * 2f32.powf(100.0 / 1200.0)).abs() < 2.0,
            "the tuned A4 came out at {tuned} Hz against {equal} Hz"
        );
        // B4 is not the tuned key, so its pitch is the one it had untuned.
        let plain = {
            let mut untuned = SynthConfig::default();
            untuned.osc2.level = 0.0;
            untuned.filter.cutoff = 18_000.0;
            untuned.filter.env_amount = 0.0;
            hz(&mono(&tone(&untuned, 70, 24_000))[4_800..])
        };
        let neighbour = hz(&mono(&tone(&config, 70, 24_000))[4_800..]);
        assert!(
            (neighbour - plain).abs() < 0.5,
            "B4 moved from {plain} Hz to {neighbour} Hz"
        );
    }

    #[test]
    fn a_macro_is_read_by_the_matrix() {
        let mut config = SynthConfig::default();
        config.osc2.level = 0.0;
        config.filter.cutoff = 18_000.0;
        config.filter.env_amount = 0.0;
        // Macro 2 to pitch, an octave at full travel. At zero the note is A4; at
        // full it is an octave above, which is the gesture the knob promises.
        config.mods = vec![Mod {
            source: ModSource::Macro2,
            target: ModTarget::Pitch,
            // The pitch's unit is octaves, so one is twelve semitones.
            amount: 1.0,
            curve: ModCurve::Linear,
        }];
        let low = hz(&mono(&tone(&config, 69, 24_000))[4_800..]);
        config.macros[1] = 1.0;
        let high = hz(&mono(&tone(&config, 69, 24_000))[4_800..]);
        assert!(
            (high - low * 2.0).abs() <= 5.0,
            "the macro moved A4 from {low} Hz to {high} Hz"
        );
    }

    #[test]
    fn a_route_s_curve_shapes_the_value_it_carries() {
        let mut config = SynthConfig::default();
        config.osc2.level = 0.0;
        config.filter.cutoff = 18_000.0;
        config.filter.env_amount = 0.0;
        config.amp.sustain = 1.0;
        config.mods = vec![Mod {
            source: ModSource::Velocity,
            target: ModTarget::Osc1,
            // Downwards, so a curve that makes the value smaller makes the note louder.
            amount: -1.0,
            curve: ModCurve::Linear,
        }];
        let render = |curve: ModCurve| {
            let mut patched = config.clone();
            patched.mods[0].curve = curve;
            let frames = render_events(&patched, &[(0, Event::NoteOn { note: 69, velocity: 0.5 })], 12_000);
            rms(&mono(&frames)[4_800..])
        };
        let linear = render(ModCurve::Linear);
        let exponential = render(ModCurve::Exp);
        // At half velocity the exp curve carries a quarter where linear carries a
        // half, so the note it subtracts from is left louder.
        assert!(
            exponential > linear * 1.2,
            "exp gave {exponential} against linear's {linear}"
        );
    }

    #[test]
    fn a_lowpass_takes_the_high_note_away() {
        let mut config = SynthConfig::default();
        config.osc1.wave = Wave::Saw;
        config.osc2.level = 0.0;
        config.filter.env_amount = 0.0;
        config.filter.key_track = 0.0;
        config.filter.cutoff = 18_000.0;
        let open = rms(&mono(&tone(&config, 96, 12_000)));
        config.filter.cutoff = 380.0;
        let closed = rms(&mono(&tone(&config, 96, 12_000)));
        assert!(
            closed < open * 0.35,
            "the closed filter left {closed} against {open}"
        );
    }

    #[test]
    fn a_released_note_falls_silent_and_frees_its_voice() {
        let mut synth = Synth::new();
        let config = SynthConfig::default();
        synth.note_on(60, 1.0);
        assert_eq!(synth.active_voices(), 1);
        let _ = synth.render(2_400, &config);
        synth.note_off(60);
        let tail = synth.render(48_000 * 2, &config);
        assert_eq!(synth.active_voices(), 0, "the voice never freed");
        let last = tail.last().copied().unwrap_or([0.0; 2]);
        assert!(last[0].abs() < 1e-6 && last[1].abs() < 1e-6);
    }

    #[test]
    fn the_pool_never_holds_more_than_it_has() {
        let mut synth = Synth::new();
        let config = SynthConfig::default();
        for note in 24..72u8 {
            synth.note_on(note, 0.8);
            let frame = synth.frame(&config);
            assert!(frame[0].is_finite() && frame[1].is_finite());
        }
        assert_eq!(synth.active_voices(), VOICES);
    }

    #[test]
    fn the_same_take_renders_the_same_frames() {
        let config = SynthConfig {
            noise: 0.3,
            ..SynthConfig::default()
        };
        let events = [
            (0, Event::NoteOn { note: 55, velocity: 0.9 }),
            (9_600, Event::NoteOn { note: 62, velocity: 0.7 }),
            (19_200, Event::NoteOff { note: 55 }),
            (24_000, Event::AllNotesOff),
        ];
        let first = render_events(&config, &events, 72_000);
        let second = render_events(&config, &events, 72_000);
        assert_eq!(first, second, "the render is not deterministic");
    }

    #[test]
    fn the_envelope_opens_over_its_attack() {
        let mut config = SynthConfig::default();
        config.amp.attack = 0.25;
        config.amp.sustain = 1.0;
        config.filter.cutoff = 18_000.0;
        config.filter.env_amount = 0.0;
        let mut synth = Synth::new();
        synth.note_on(69, 1.0);
        let early = rms(&mono(&synth.render(480, &config)));
        let late = rms(&mono(&synth.render(9_600, &config)));
        assert!(late > early * 2.0, "the attack did not open: {early} then {late}");
    }

    #[test]
    fn the_glide_arrives_at_the_new_note() {
        let mut config = SynthConfig::default();
        config.glide_ms = 120.0;
        config.osc2.level = 0.0;
        config.filter.cutoff = 18_000.0;
        config.filter.env_amount = 0.0;
        let mut synth = Synth::new();
        synth.note_on(57, 1.0);
        let _ = synth.render(4_800, &config);
        synth.note_on(69, 1.0);
        // The new note starts at the old one and climbs to its own pitch, so the
        // first window is still near A3 while the settled window is A3 and A4.
        let onset = hz(&mono(&synth.render(1_200, &config)));
        let settled = mono(&synth.render(24_000, &config));
        let arrived = hz(&settled[2_400..]);
        assert!(
            onset < arrived * 0.85,
            "the glide did not sweep: {onset} Hz then {arrived} Hz"
        );
        assert!(arrived > 600.0, "the glide never left A3: {arrived} Hz");
    }

    #[test]
    fn a_repeated_note_keeps_the_waveform_where_it_was() {
        let mut config = SynthConfig::default();
        config.osc1.wave = Wave::Sine;
        config.osc2.level = 0.0;
        config.filter.cutoff = 18_000.0;
        config.filter.env_amount = 0.0;
        config.filter.key_track = 0.0;
        config.amp.attack = 0.001;
        config.amp.decay = 10.0;
        config.amp.sustain = 1.0;
        let mut synth = Synth::new();
        synth.note_on(69, 1.0);
        let mut samples = mono(&synth.render(9_600, &config));
        // The same key again while it is still sounding: the phase and the
        // envelope carry on, so no step larger than the sine's own slope appears.
        synth.note_on(69, 1.0);
        samples.extend(mono(&synth.render(960, &config)));
        let largest = samples
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(largest < 0.05, "the retrigger stepped the waveform by {largest}");
    }

    #[test]
    fn a_patch_out_of_range_is_refused_with_its_field_named() {
        let mut config = SynthConfig::default();
        config.filter.cutoff = 30_000.0;
        let reason = config.validate().expect_err("a cutoff above the band");
        assert!(reason.contains("Cutoff"), "{reason}");
        config.filter.cutoff = 1_000.0;
        config.amp.release = 0.0;
        let reason = config.validate().expect_err("a release that cannot be heard");
        assert!(reason.contains("release"), "{reason}");
        assert!(SynthConfig::default().validate().is_ok());
    }

    #[test]
    fn every_builtin_patch_is_one_the_engine_accepts() {
        for (id, name, patch) in builtin_presets() {
            patch
                .validate()
                .unwrap_or_else(|reason| panic!("{id} ({name}) was refused: {reason}"));
            let mut synth = Synth::new();
            synth.note_on(60, 1.0);
            let rendered = synth.render(9_600, &patch);
            assert!(
                rendered.iter().flatten().all(|value| value.is_finite()),
                "{name} produced a non-finite sample"
            );
        }
    }

    #[test]
    fn the_bend_moves_the_pitch_and_no_further_than_two_semitones() {
        let mut config = SynthConfig::default();
        config.osc2.level = 0.0;
        config.filter.cutoff = 18_000.0;
        config.filter.env_amount = 0.0;
        let mut synth = Synth::new();
        synth.set_bend(2.0);
        synth.note_on(69, 1.0);
        let bent = hz(&mono(&synth.render(24_000, &config))[4_800..]);
        assert!(
            (bent - 493.9).abs() < 3.0,
            "a two-semitone bend gave {bent} Hz"
        );
    }

    #[test]
    fn a_unison_stack_is_wider_not_louder() {
        let mut single = SynthConfig::default();
        single.osc2.level = 0.0;
        single.filter.cutoff = 18_000.0;
        single.filter.env_amount = 0.0;
        single.osc1.detune = 0.0;
        single.osc1.spread = 0.0;
        let mut stack = single.clone();
        stack.osc1.unison = 7;
        stack.osc1.detune = 20.0;
        stack.osc1.spread = 0.9;
        let one = mono(&tone(&single, 57, 24_000));
        let many = mono(&tone(&stack, 57, 24_000));
        let ratio = rms(&many[4_800..]) / rms(&one[4_800..]).max(1e-6);
        assert!(
            (0.5..=2.0).contains(&ratio),
            "seven copies came out {ratio} times as loud as one"
        );
        assert!(stack.validate().is_ok());
    }

    #[test]
    fn spread_puts_the_copies_across_the_image() {
        let mut config = SynthConfig::default();
        config.osc2.level = 0.0;
        config.filter.cutoff = 18_000.0;
        config.filter.env_amount = 0.0;
        config.osc1.unison = 6;
        config.osc1.detune = 12.0;
        let centred = tone(&config, 57, 24_000);
        let spread = {
            let mut spread = config.clone();
            spread.osc1.spread = 1.0;
            tone(&spread, 57, 24_000)
        };
        let difference = |frames: &[[f32; 2]]| {
            let sum: f32 = frames[4_800..]
                .iter()
                .map(|frame| (frame[0] - frame[1]).abs())
                .sum();
            sum / frames[4_800..].len() as f32
        };
        // The voice's own pan already gives both sides a little difference, so the
        // test asks the spread to more than double it rather than to invent it.
        assert!(
            difference(&spread) > difference(&centred) * 1.8,
            "spread did not widen the stack: {} against {}",
            difference(&spread),
            difference(&centred)
        );
    }

    #[test]
    fn a_detuned_stack_beats_against_itself() {
        let mut config = SynthConfig::default();
        config.osc2.level = 0.0;
        config.filter.cutoff = 18_000.0;
        config.filter.env_amount = 0.0;
        config.amp.attack = 0.001;
        config.amp.decay = 10.0;
        config.amp.sustain = 1.0;
        config.osc1.unison = 4;
        let steady = tone(&config, 57, 24_000);
        config.osc1.detune = 30.0;
        let beating = tone(&config, 57, 24_000);
        let swing = |frames: &[[f32; 2]]| {
            let windows: Vec<f32> = frames[4_800..]
                .chunks(2_400)
                .map(|window| rms(&mono(window)))
                .collect();
            let high = windows.iter().fold(0.0f32, |high, level| high.max(*level));
            let low = windows.iter().fold(f32::MAX, |low, level| low.min(*level));
            high / low.max(1e-6)
        };
        assert!(
            swing(&beating) > swing(&steady) * 1.05,
            "thirty cents did not beat: {} against {}",
            swing(&beating),
            swing(&steady)
        );
    }

    #[test]
    fn the_matrix_moves_what_it_is_pointed_at() {
        let mut config = SynthConfig::default();
        config.osc2.level = 0.0;
        config.filter.cutoff = 18_000.0;
        config.filter.env_amount = 0.0;
        config.mods = vec![Mod {
            source: ModSource::Velocity,
            target: ModTarget::Pitch,
            amount: 0.5,
            curve: ModCurve::Linear,
        }];
        // Half the route's twelve semitones, at full velocity: a fifth up.
        let lifted = hz(&mono(&tone(&config, 69, 24_000))[4_800..]);
        assert!(
            (lifted - 622.3).abs() < 6.0,
            "a velocity route to pitch gave {lifted} Hz"
        );

        let mut dark = SynthConfig::default();
        dark.osc2.level = 0.0;
        dark.filter.cutoff = 3_000.0;
        dark.filter.env_amount = 0.0;
        dark.filter.key_track = 0.0;
        let open = rms(&mono(&tone(&dark, 84, 12_000)));
        dark.mods = vec![Mod {
            source: ModSource::Velocity,
            target: ModTarget::Cutoff,
            amount: -1.0,
            curve: ModCurve::Linear,
        }];
        let closed = rms(&mono(&tone(&dark, 84, 12_000)));
        assert!(
            closed < open * 0.4,
            "a route to cutoff left {closed} against {open}"
        );
    }

    #[test]
    fn two_routes_to_one_destination_add_up() {
        let mut config = SynthConfig::default();
        config.osc2.level = 0.0;
        config.osc1.level = 0.2;
        config.filter.cutoff = 18_000.0;
        config.filter.env_amount = 0.0;
        let quiet = rms(&mono(&tone(&config, 69, 12_000))[4_800..]);
        config.mods = vec![
            Mod { source: ModSource::Velocity, target: ModTarget::Osc1, amount: 0.3, curve: ModCurve::Linear },
            Mod { source: ModSource::Key, target: ModTarget::Osc1, amount: 0.3, curve: ModCurve::Linear },
        ];
        let louder = rms(&mono(&tone(&config, 69, 12_000))[4_800..]);
        assert!(
            louder > quiet * 1.3,
            "two routes gave {louder} against one patch's {quiet}"
        );
    }

    #[test]
    fn a_table_reads_without_aliasing_at_the_top_of_the_keyboard() {
        let mut config = SynthConfig::default();
        config.osc2.level = 0.0;
        config.filter.cutoff = 20_000.0;
        config.filter.env_amount = 0.0;
        config.osc1.wave = Wave::Table;
        // Frame zero of the first table is a sine, so anything above the note's own
        // harmonic is aliasing: a sine has nothing else to show.
        config.osc1.table = 0;
        config.osc1.morph = 0.0;
        let rendered = mono(&tone(&config, 96, 24_000));
        let settled = &rendered[4_800..];
        let counted = hz(settled);
        assert!(
            (counted - 2_093.0).abs() < 30.0,
            "a table sine at C7 came out at {counted} Hz"
        );
        let peak = settled.iter().fold(0.0f32, |peak, sample| peak.max(sample.abs()));
        let parts = rms(settled) / peak.max(1e-6);
        assert!(
            (0.62..=0.75).contains(&parts),
            "the table sine is not a sine: {parts} of its peak"
        );
    }

    #[test]
    fn a_morph_walks_the_table() {
        let mut config = SynthConfig::default();
        config.osc2.level = 0.0;
        config.filter.cutoff = 18_000.0;
        config.filter.env_amount = 0.0;
        config.osc1.wave = Wave::Table;
        config.osc1.table = 0;
        config.osc1.morph = 0.0;
        let sine = mono(&tone(&config, 60, 12_000));
        config.osc1.morph = 1.0;
        let bright = mono(&tone(&config, 60, 12_000));
        // The last frame is the square end of the table: same note, more harmonics,
        // so the two renders cannot be the same samples.
        let difference: f32 = sine
            .iter()
            .zip(&bright)
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(difference > 10.0, "the morph did nothing: {difference}");
    }

    #[test]
    fn the_ladder_and_the_comb_are_stable_at_full_resonance() {
        for kind in [FilterKind::Ladder, FilterKind::Comb] {
            let mut config = SynthConfig::default();
            config.noise = 0.6;
            config.osc2.level = 0.4;
            config.filter.kind = kind;
            config.filter.cutoff = 220.0;
            config.filter.resonance = 1.0;
            let mut synth = Synth::new();
            synth.note_on(48, 1.0);
            let rendered = synth.render(48_000 * 3, &config);
            assert!(
                rendered.iter().flatten().all(|sample| sample.is_finite()),
                "{kind:?} produced a non-finite sample"
            );
            let peak = rendered
                .iter()
                .flatten()
                .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
            assert!(peak <= 1.0, "{kind:?} ran away to {peak}");
        }
    }

    #[test]
    fn the_ladder_gets_brighter_as_it_resonates() {
        let mut config = SynthConfig::default();
        config.osc1.level = 0.0;
        config.osc2.level = 0.0;
        config.noise = 0.8;
        config.filter.kind = FilterKind::Ladder;
        config.filter.cutoff = 800.0;
        config.filter.env_amount = 0.0;
        config.filter.key_track = 0.0;
        config.filter.resonance = 0.05;
        let flat = energy_at(&mono(&tone(&config, 45, 24_000))[9_600..], 800.0);
        config.filter.resonance = 0.9;
        let resonant = energy_at(&mono(&tone(&config, 45, 24_000))[9_600..], 800.0);
        assert!(
            resonant > flat * 1.5,
            "the ladder's resonance did not lift its band: {resonant} against {flat}"
        );
    }

    /// The one lock every test that changes the registry holds: the tables are a
    /// process-wide list, so two tests adding one at once would disagree about the
    /// index and the count.
    static TABLE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static NEXT_TABLE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn lock_tables() -> std::sync::MutexGuard<'static, ()> {
        TABLE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn unique_name(label: &str) -> String {
        format!("{label} {}", NEXT_TABLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }

    /// A frame's harmonics: one partial at full level, the rest silent, so a frame
    /// made from it is a sine at that partial and nothing else.
    fn one_harmonic(harmonic: usize) -> Vec<f32> {
        (1..=128)
            .map(|index| if index == harmonic { 1.0 } else { 0.0 })
            .collect()
    }

    fn distinct(values: &[f32]) -> usize {
        let mut bits: Vec<u32> = values.iter().map(|value| value.to_bits()).collect();
        bits.sort_unstable();
        bits.dedup();
        bits.len()
    }

    #[test]
    fn the_analysis_of_a_sine_reads_one_harmonic_and_nothing_above_it() {
        let samples: Vec<f32> = (0..TABLE_SIZE)
            .map(|index| (std::f32::consts::TAU * index as f32 / TABLE_SIZE as f32).sin())
            .collect();
        let levels = analyze_cycle(&samples, 8);
        assert_eq!(levels.len(), 8);
        assert!(
            (levels[0] - 1.0).abs() < 0.01,
            "a unit sine read {} as its fundamental",
            levels[0]
        );
        for (index, level) in levels.iter().enumerate().skip(1) {
            assert!(
                level.abs() < 0.005,
                "harmonic {} read {} on a sine",
                index + 1,
                level
            );
        }
    }

    #[test]
    fn an_added_table_is_named_and_drawn_by_the_engine() {
        let _guard = lock_tables();
        let before = table_count();
        let name = unique_name("Test sine");
        let index = add_table(name.clone(), vec![one_harmonic(1)]).expect("a table of one frame");
        assert_eq!(index, before, "a table takes the next index");
        assert_eq!(table_count(), before + 1);
        assert_eq!(table_names()[index], name);
        let shape = table_shape(index as u8, 0.0, 256);
        assert_eq!(shape.len(), 256);
        assert!(
            shape.iter().all(|value| value.is_finite() && value.abs() <= 1.01),
            "the drawn cycle left the frame's range"
        );
        assert!(
            distinct(&shape) > 100,
            "the drawn cycle has only {} values",
            distinct(&shape)
        );
    }

    #[test]
    fn edited_table_preserves_phase_and_rejects_invalid_frames() {
        let _guard = lock_tables();
        let index = add_table(unique_name("Phase edit"), vec![one_harmonic(1)]).unwrap();
        let cycle: Vec<f32> = (0..TABLE_SIZE).map(|i| (std::f32::consts::TAU * i as f32 / TABLE_SIZE as f32).cos()).collect();
        edit_table(index, vec![cycle]).unwrap();
        let before = table_shape(index as u8, 0.0, 256);
        assert!(before[0] > 0.99, "cosine phase was lost");
        let stored = user_tables_json();
        restore_tables(&stored);
        let after = table_shape(index as u8, 0.0, 256);
        assert!(before.iter().zip(after).all(|(a, b)| (a - b).abs() < 0.001));
        assert!(edit_table(index, vec![vec![f32::NAN; TABLE_SIZE]]).is_err());
        assert!(edit_table(0, vec![vec![0.0; TABLE_SIZE]]).is_err());
    }

    #[test]
    fn failed_table_save_does_not_change_the_playing_table() {
        let _guard = lock_tables();
        let index = add_table(unique_name("Failed save"), vec![one_harmonic(1)]).unwrap();
        let before = table_shape(index as u8, 0.0, 256);
        let cycle: Vec<f32> = (0..TABLE_SIZE).map(|i| (std::f32::consts::TAU * i as f32 / TABLE_SIZE as f32).cos()).collect();
        let error = edit_table_and_persist(index, vec![cycle.clone()], |_| Err("disk full".into())).unwrap_err();
        assert_eq!(error, "disk full");
        assert_eq!(before, table_shape(index as u8, 0.0, 256));
        let count = table_count();
        assert!(add_sample_table_and_persist(unique_name("Failed import"), vec![cycle], |_| Err("disk full".into())).is_err());
        assert_eq!(count, table_count());
    }

    #[test]
    fn a_one_frame_table_morphs_to_itself_instead_of_past_its_end() {
        let _guard = lock_tables();
        let index = add_table(unique_name("One frame"), vec![one_harmonic(1)])
            .expect("a table of one frame");
        let start = table_shape(index as u8, 0.0, 64);
        let end = table_shape(index as u8, 1.0, 64);
        assert_eq!(start.len(), 64);
        assert_eq!(end.len(), 64);
        // One frame is the whole stack, so the morph has nowhere to travel: the read
        // at the end must be the read at the start, not a silence or a panic.
        assert!(start.iter().zip(&end).all(|(a, b)| (a - b).abs() < 1e-6));
        assert!(end.iter().any(|value| value.abs() > 0.9));
    }

    #[test]
    fn a_frame_with_too_many_harmonics_or_no_frames_is_refused() {
        assert!(add_table(unique_name("Empty"), Vec::new()).is_err());
        assert!(add_table(unique_name("Wide"), vec![vec![0.0; 129]]).is_err());
        // Nine frames is one more than the morph can walk.
        assert!(add_table(unique_name("Tall"), vec![one_harmonic(1); 9]).is_err());
    }

    #[test]
    fn importing_the_same_name_twice_updates_the_table_in_place() {
        let _guard = TABLE_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let name = format!(
            "Gate duplicate {}",
            NEXT_TABLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let first = add_table(name.clone(), vec![vec![1.0, 0.0]]).expect("a table");
        let before = table_count();
        let second = add_table(name.clone(), vec![vec![0.0, 1.0]]).expect("the same table");
        assert_eq!(second, first, "the update took a new index");
        assert_eq!(table_count(), before, "the update added a second copy");
        // The frames are the new ones: the second harmonic is now the loud one.
        let shape = table_shape(second as u8, 0.0, 64);
        assert!(shape.iter().all(|value| value.is_finite()));
    }

    #[test]
    fn the_user_tables_survive_being_written_and_read_back() {
        let _guard = lock_tables();
        let name = unique_name("Round trip");
        let index = add_table(name.clone(), vec![one_harmonic(1), one_harmonic(3)])
            .expect("a table of two frames");
        let drawn = table_shape(index as u8, 1.0, 128);
        let stored = user_tables_json();
        let entries = serde_json::from_str::<serde_json::Value>(&stored)
            .expect("the store is JSON")
            .get("tables")
            .and_then(|tables| tables.as_array())
            .map(Vec::len)
            .unwrap_or(0);
        assert!(entries >= 1, "the store forgot the table it was given");
        let count = table_count();
        let added = restore_tables(&stored);
        assert_eq!(added, entries, "every stored table is read back");
        let names = table_names();
        let restored = names
            .iter()
            .rposition(|found| found == &name)
            .expect("the table came back under its own name");
        // A restore is the same import: it puts the table back where it was instead
        // of appending a copy at every launch.
        assert_eq!(restored, index, "the restore added a second copy");
        assert_eq!(table_count(), count, "the restore changed the count");
        // The stored frame is its harmonics, so the last frame comes back as the
        // third-harmonic sine it was saved as.
        let again = table_shape(restored as u8, 1.0, 128);
        let difference: f32 = drawn
            .iter()
            .zip(&again)
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>()
            / 128.0;
        assert!(
            difference < 0.01,
            "the restored table drew differently: {difference}"
        );
    }
}

