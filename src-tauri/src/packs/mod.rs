//! The factory packs: two hundred small collections of four patches each, so a new
//! install opens a library rather than six starting points.
//!
//! A pack is a seed, not four hand-typed patches: a family, one character value and
//! four names. The builder below shapes the family's voice with that character and
//! then moves it four ways — the pack's own, a softer take, a harder one, and one
//! that moves — so the four patches belong together without being copies. It is
//! deterministic, so a pack sounds the same on every launch, and every number it
//! writes is inside the engine's own ranges (a test plays every one through
//! `validate`).
//!
//! The seeds themselves are written in four files beside this one, one per group of
//! families, because two hundred of them in one place is a wall nobody can read.

mod grid;
mod set_a;
mod set_b;
mod set_c;
mod set_d;

use serde::Serialize;

use crate::synth::{
    Envelope, Filter, FilterKind, Lfo, LfoTarget, LfoWave, Mod, ModCurve, ModSource, ModTarget,
    Oscillator, SynthConfig, Wave, MAX_UNISON,
};

/// The families the catalogue is sorted by. A family is the skeleton a pack is built
/// on: which waves, which filter, how long a note is held and whether it is played
/// short or left ringing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Bass,
    Sub,
    Lead,
    Pluck,
    Keys,
    Pad,
    Drone,
    Brass,
    Stab,
    Mallet,
    Bells,
    Texture,
}

impl Family {
    pub const ALL: [Family; 12] = [
        Family::Bass,
        Family::Sub,
        Family::Lead,
        Family::Pluck,
        Family::Keys,
        Family::Pad,
        Family::Drone,
        Family::Brass,
        Family::Stab,
        Family::Mallet,
        Family::Bells,
        Family::Texture,
    ];

    /// What the page prints, in the order it prints it.
    pub fn label(self) -> &'static str {
        match self {
            Family::Bass => "Bass",
            Family::Sub => "Sub",
            Family::Lead => "Lead",
            Family::Pluck => "Pluck",
            Family::Keys => "Keys",
            Family::Pad => "Pad",
            Family::Drone => "Drone",
            Family::Brass => "Brass",
            Family::Stab => "Stab",
            Family::Mallet => "Mallet",
            Family::Bells => "Bells",
            Family::Texture => "Texture",
        }
    }
}

/// One pack's card: what it is called, who it is for, and the four patches in it.
pub struct Seed {
    pub id: &'static str,
    pub name: &'static str,
    pub family: Family,
    /// Where this pack sits in its family, 0 dark and soft to 1 bright and hard.
    pub character: f32,
    pub blurb: &'static str,
    pub patches: [&'static str; 4],
}

/// The family's own voice, before a pack's character is applied to it.
struct Voice {
    wave1: Wave,
    wave2: Wave,
    filter: FilterKind,
    octave1: i32,
    octave2: i32,
    semitone2: i32,
    cents2: f32,
    /// The cutoff the character sweeps between, dark to bright.
    cutoff: (f32, f32),
    resonance: (f32, f32),
    amp: Envelope,
    filter_env: Envelope,
    env_amount: f32,
    key_track: f32,
    unison: u8,
    detune: f32,
    spread: f32,
    sub: f32,
    noise: f32,
    lfo_rate: f32,
    lfo_depth: f32,
    lfo_target: LfoTarget,
    level: f32,
    glide_ms: f32,
    drive: f32,
}

fn envelope(attack: f32, decay: f32, sustain: f32, release: f32) -> Envelope {
    Envelope {
        attack,
        decay,
        sustain,
        release,
    }
}

/// The skeleton of a family. The numbers are the family's own extremes rather than
/// anything a pack can move: a pad stays slow here, a stab stays short here, and the
/// character only decides where inside the family the pack sits.
fn voice(family: Family) -> Voice {
    match family {
        Family::Bass => Voice {
            wave1: Wave::Saw,
            wave2: Wave::Square,
            filter: FilterKind::Ladder,
            octave1: -1,
            octave2: -1,
            semitone2: 0,
            cents2: -7.0,
            cutoff: (170.0, 1_500.0),
            resonance: (0.10, 0.38),
            amp: envelope(0.004, 0.22, 0.78, 0.16),
            filter_env: envelope(0.002, 0.18, 0.16, 0.14),
            env_amount: 1.1,
            key_track: 0.25,
            unison: 1,
            detune: 0.0,
            spread: 0.0,
            sub: 0.4,
            noise: 0.0,
            lfo_rate: 0.4,
            lfo_depth: 0.0,
            lfo_target: LfoTarget::Filter,
            level: 0.72,
            glide_ms: 0.0,
            drive: 0.2,
        },
        Family::Sub => Voice {
            wave1: Wave::Square,
            wave2: Wave::Sine,
            filter: FilterKind::Lowpass,
            octave1: -2,
            octave2: -2,
            semitone2: 0,
            cents2: 0.0,
            cutoff: (90.0, 520.0),
            resonance: (0.02, 0.18),
            amp: envelope(0.012, 0.3, 0.92, 0.22),
            filter_env: envelope(0.006, 0.2, 0.3, 0.2),
            env_amount: 0.35,
            key_track: 0.15,
            unison: 1,
            detune: 0.0,
            spread: 0.0,
            sub: 0.55,
            noise: 0.0,
            lfo_rate: 0.25,
            lfo_depth: 0.0,
            lfo_target: LfoTarget::Filter,
            level: 0.76,
            glide_ms: 12.0,
            drive: 0.08,
        },
        Family::Lead => Voice {
            wave1: Wave::Saw,
            wave2: Wave::Saw,
            filter: FilterKind::Lowpass,
            octave1: 0,
            octave2: 0,
            semitone2: 0,
            cents2: 6.0,
            cutoff: (2_200.0, 9_000.0),
            resonance: (0.12, 0.45),
            amp: envelope(0.008, 0.14, 0.9, 0.26),
            filter_env: envelope(0.004, 0.2, 0.4, 0.24),
            env_amount: 0.55,
            key_track: 0.55,
            unison: 2,
            detune: 9.0,
            spread: 0.3,
            sub: 0.0,
            noise: 0.0,
            lfo_rate: 5.4,
            lfo_depth: 0.1,
            lfo_target: LfoTarget::Pitch,
            level: 0.62,
            glide_ms: 40.0,
            drive: 0.12,
        },
        Family::Pluck => Voice {
            wave1: Wave::Square,
            wave2: Wave::Triangle,
            filter: FilterKind::Lowpass,
            octave1: 0,
            octave2: 0,
            semitone2: 0,
            cents2: 0.0,
            cutoff: (900.0, 4_200.0),
            resonance: (0.2, 0.55),
            amp: envelope(0.002, 0.26, 0.0, 0.24),
            filter_env: envelope(0.001, 0.2, 0.0, 0.2),
            env_amount: 1.7,
            key_track: 0.65,
            unison: 1,
            detune: 0.0,
            spread: 0.0,
            sub: 0.0,
            noise: 0.0,
            lfo_rate: 4.2,
            lfo_depth: 0.0,
            lfo_target: LfoTarget::Filter,
            level: 0.7,
            glide_ms: 0.0,
            drive: 0.1,
        },
        Family::Keys => Voice {
            wave1: Wave::Triangle,
            wave2: Wave::Saw,
            filter: FilterKind::Lowpass,
            octave1: 0,
            octave2: 0,
            semitone2: 0,
            cents2: 4.0,
            cutoff: (1_600.0, 6_500.0),
            resonance: (0.08, 0.3),
            amp: envelope(0.006, 0.5, 0.42, 0.5),
            filter_env: envelope(0.004, 0.4, 0.25, 0.4),
            env_amount: 0.7,
            key_track: 0.45,
            unison: 1,
            detune: 0.0,
            spread: 0.0,
            sub: 0.1,
            noise: 0.0,
            lfo_rate: 4.8,
            lfo_depth: 0.05,
            lfo_target: LfoTarget::Pitch,
            level: 0.66,
            glide_ms: 0.0,
            drive: 0.05,
        },
        Family::Pad => Voice {
            wave1: Wave::Saw,
            wave2: Wave::Saw,
            filter: FilterKind::Lowpass,
            octave1: 0,
            octave2: 0,
            semitone2: 0,
            cents2: 9.0,
            cutoff: (600.0, 2_600.0),
            resonance: (0.08, 0.3),
            amp: envelope(0.9, 1.4, 0.82, 2.2),
            filter_env: envelope(1.5, 2.0, 0.45, 2.4),
            env_amount: 0.9,
            key_track: 0.4,
            unison: 4,
            detune: 14.0,
            spread: 0.6,
            sub: 0.0,
            noise: 0.0,
            lfo_rate: 0.35,
            lfo_depth: 0.14,
            lfo_target: LfoTarget::Filter,
            level: 0.52,
            glide_ms: 60.0,
            drive: 0.0,
        },
        Family::Drone => Voice {
            wave1: Wave::Saw,
            wave2: Wave::Triangle,
            filter: FilterKind::Lowpass,
            octave1: -1,
            octave2: 0,
            semitone2: 7,
            cents2: 11.0,
            cutoff: (300.0, 1_900.0),
            resonance: (0.05, 0.28),
            amp: envelope(2.4, 3.0, 0.9, 4.5),
            filter_env: envelope(3.0, 3.5, 0.6, 4.0),
            env_amount: 0.6,
            key_track: 0.2,
            unison: 3,
            detune: 20.0,
            spread: 0.75,
            sub: 0.2,
            noise: 0.04,
            lfo_rate: 0.08,
            lfo_depth: 0.22,
            lfo_target: LfoTarget::Filter,
            level: 0.46,
            glide_ms: 180.0,
            drive: 0.0,
        },
        Family::Brass => Voice {
            wave1: Wave::Saw,
            wave2: Wave::Square,
            filter: FilterKind::Ladder,
            octave1: 0,
            octave2: 0,
            semitone2: 0,
            cents2: 5.0,
            cutoff: (700.0, 3_400.0),
            resonance: (0.1, 0.35),
            amp: envelope(0.06, 0.3, 0.8, 0.3),
            filter_env: envelope(0.09, 0.5, 0.5, 0.3),
            env_amount: 1.6,
            key_track: 0.5,
            unison: 2,
            detune: 7.0,
            spread: 0.25,
            sub: 0.0,
            noise: 0.0,
            lfo_rate: 5.0,
            lfo_depth: 0.08,
            lfo_target: LfoTarget::Pitch,
            level: 0.62,
            glide_ms: 18.0,
            drive: 0.25,
        },
        Family::Stab => Voice {
            wave1: Wave::Square,
            wave2: Wave::Saw,
            filter: FilterKind::Lowpass,
            octave1: 0,
            octave2: 0,
            semitone2: 12,
            cents2: 3.0,
            cutoff: (1_200.0, 5_500.0),
            resonance: (0.25, 0.6),
            amp: envelope(0.002, 0.16, 0.0, 0.2),
            filter_env: envelope(0.001, 0.14, 0.0, 0.16),
            env_amount: 2.1,
            key_track: 0.4,
            unison: 3,
            detune: 6.0,
            spread: 0.4,
            sub: 0.0,
            noise: 0.0,
            lfo_rate: 6.0,
            lfo_depth: 0.0,
            lfo_target: LfoTarget::Filter,
            level: 0.66,
            glide_ms: 0.0,
            drive: 0.2,
        },
        Family::Mallet => Voice {
            wave1: Wave::Triangle,
            wave2: Wave::Sine,
            filter: FilterKind::Lowpass,
            octave1: 0,
            octave2: 1,
            semitone2: 0,
            cents2: 0.0,
            cutoff: (2_600.0, 11_000.0),
            resonance: (0.05, 0.22),
            amp: envelope(0.002, 0.4, 0.0, 0.5),
            filter_env: envelope(0.001, 0.25, 0.0, 0.3),
            env_amount: 1.2,
            key_track: 0.7,
            unison: 1,
            detune: 0.0,
            spread: 0.0,
            sub: 0.0,
            noise: 0.0,
            lfo_rate: 3.5,
            lfo_depth: 0.0,
            lfo_target: LfoTarget::Filter,
            level: 0.6,
            glide_ms: 0.0,
            drive: 0.0,
        },
        Family::Bells => Voice {
            wave1: Wave::Sine,
            wave2: Wave::Triangle,
            filter: FilterKind::Bandpass,
            octave1: 1,
            octave2: 1,
            semitone2: 7,
            cents2: 2.0,
            cutoff: (1_800.0, 7_500.0),
            resonance: (0.2, 0.5),
            amp: envelope(0.003, 1.1, 0.0, 1.4),
            filter_env: envelope(0.002, 0.8, 0.0, 1.0),
            env_amount: 1.4,
            key_track: 0.6,
            unison: 2,
            detune: 4.0,
            spread: 0.5,
            sub: 0.0,
            noise: 0.0,
            lfo_rate: 0.9,
            lfo_depth: 0.1,
            lfo_target: LfoTarget::Amp,
            level: 0.52,
            glide_ms: 0.0,
            drive: 0.0,
        },
        Family::Texture => Voice {
            wave1: Wave::Saw,
            wave2: Wave::Square,
            filter: FilterKind::Bandpass,
            octave1: 0,
            octave2: 0,
            semitone2: 0,
            cents2: 14.0,
            cutoff: (500.0, 4_800.0),
            resonance: (0.25, 0.62),
            amp: envelope(0.6, 1.2, 0.7, 1.6),
            filter_env: envelope(1.0, 1.6, 0.4, 1.8),
            env_amount: 1.0,
            key_track: 0.25,
            unison: 3,
            detune: 24.0,
            spread: 0.8,
            sub: 0.0,
            noise: 0.45,
            lfo_rate: 0.2,
            lfo_depth: 0.3,
            lfo_target: LfoTarget::Filter,
            level: 0.46,
            glide_ms: 90.0,
            drive: 0.1,
        },
    }
}

fn lerp(low: f32, high: f32, t: f32) -> f32 {
    low + (high - low) * t
}

/// A cutoff moves by octaves, not by hertz: halfway between dark and bright is a
/// factor, not a difference, or every family's centre would sit at the top.
fn log_lerp(low: f32, high: f32, t: f32) -> f32 {
    low * (high / low).powf(t)
}

fn clamp(value: f32, low: f32, high: f32) -> f32 {
    if value < low {
        low
    } else if value > high {
        high
    } else {
        value
    }
}

/// One of a curated pack's four patches. `variant` is 0 to 3: the pack's own voice,
/// then a softer take, a harder one, and one that moves.
pub fn patch(seed: &Seed, variant: usize) -> SynthConfig {
    shape(voice(seed.family), seed.character, variant)
}

/// A cell of the coverage grid: the family's skeleton wearing the cell's waves and its
/// filter, placed on the dark-to-bright axis by the cell itself. It is the same
/// builder as a curated pack, which is what makes the grid a promise about the shelf
/// rather than a second, parallel way of making patches.
pub fn grid_patch(cell: &grid::Cell, variant: usize) -> SynthConfig {
    shape(grid_voice(cell), grid::character(cell), variant)
}

/// The family's voice with this cell's oscillator shape and filter. The family keeps
/// its own second oscillator, so a cell is a family in a new colour rather than a
/// family replaced by a coordinate. Two copies of one shape at one pitch add up to a
/// louder oscillator rather than a pair, so a cell that lands on the family's own
/// second wave moves that copy an octave up: it is then heard as two oscillators.
fn grid_voice(cell: &grid::Cell) -> Voice {
    let mut voice = voice(cell.family);
    voice.wave1 = cell.wave;
    voice.filter = cell.filter;
    if voice.wave1 == voice.wave2 {
        voice.octave2 = (voice.octave1 + 1).clamp(-2, 2);
        voice.cents2 = 5.0;
    }
    voice
}

/// The one builder: a family's voice, where the pack sits inside it, and which of the
/// four takes. Every patch in the catalogue comes out of here.
fn shape(voice: Voice, character: f32, variant: usize) -> SynthConfig {
    let t = clamp(character, 0.0, 1.0);
    let dark = variant == 1;
    let hard = variant == 2;
    let moving = variant >= 3;

    let mut cutoff = log_lerp(voice.cutoff.0, voice.cutoff.1, t);
    let mut resonance = lerp(voice.resonance.0, voice.resonance.1, t);
    let mut amp = voice.amp.clone();
    let mut filter_env = voice.filter_env.clone();
    let mut env_amount = voice.env_amount;
    let mut unison = voice.unison;
    let mut detune = voice.detune;
    let mut lfo_depth = voice.lfo_depth;
    let mut glide = voice.glide_ms;
    let mut drive = voice.drive;
    let noise = voice.noise;
    let mut osc1_level = 0.8;
    let mut osc2_level = 0.42;
    let mut level = voice.level;
    let mut lfo2 = Lfo {
        wave: LfoWave::Triangle,
        rate: 0.8,
        depth: 0.0,
        target: LfoTarget::Filter,
    };
    let mut mods: Vec<Mod> = Vec::new();

    // The character is the pack's own colour: a slower attack and a longer release
    // for the warm end, a shorter decay for the bright one, all inside the family.
    amp.attack *= 0.75 + 0.5 * t;
    amp.decay *= 1.15 - 0.35 * t;
    amp.release *= 0.85 + 0.45 * t;
    filter_env.attack *= 0.8 + 0.45 * t;
    filter_env.release *= 0.9 + 0.3 * t;

    if dark {
        // Softer: darker, slower, wider, and a copy or two more in the stack.
        cutoff *= 0.62;
        amp.attack *= 2.4;
        filter_env.attack *= 2.0;
        amp.release *= 1.5;
        resonance -= 0.08;
        unison = (unison + 2).min(MAX_UNISON as u8);
        detune += 4.0;
        lfo_depth *= 1.15;
        level *= 0.92;
        osc2_level *= 0.8;
    }
    if hard {
        // Harder: brighter, shorter, more resonant, and velocity opens the filter.
        cutoff *= 1.9;
        amp.decay *= 0.55;
        filter_env.decay *= 0.6;
        resonance += 0.1;
        env_amount += 0.35;
        drive += 0.2;
        unison = unison.max(2);
        detune += 2.0;
        osc2_level += 0.1;
        mods.push(Mod {
            source: ModSource::Velocity,
            target: ModTarget::Cutoff,
            amount: 0.42,
            curve: ModCurve::S,
        });
    }
    if moving {
        // Moving: the shape is the sound, so the second LFO reads the filter and a
        // macro leans the second oscillator in.
        lfo_depth = lfo_depth * 2.2 + 0.08;
        amp.release *= 1.8;
        glide += 24.0;
        lfo2.depth = 0.18;
        lfo2.rate = 0.23;
        mods.push(Mod {
            source: ModSource::Lfo2,
            target: ModTarget::Cutoff,
            amount: 0.3,
            curve: ModCurve::Linear,
        });
        mods.push(Mod {
            source: ModSource::Macro1,
            target: ModTarget::Osc2,
            amount: 0.4,
            curve: ModCurve::Linear,
        });
        osc1_level *= 0.92;
    }
    if variant == 1 {
        osc1_level *= 0.85;
    }

    let cents2 = voice.cents2 + variant as f32 * 1.5;
    SynthConfig {
        level: clamp(level, 0.0, 1.0),
        glide_ms: clamp(glide, 0.0, 500.0),
        osc1: Oscillator {
            wave: voice.wave1,
            octave: voice.octave1,
            level: clamp(osc1_level, 0.0, 1.0),
            unison: unison.max(1),
            detune: clamp(detune, 0.0, 50.0),
            spread: clamp(voice.spread, 0.0, 1.0),
            ..Oscillator::default()
        },
        osc2: Oscillator {
            wave: voice.wave2,
            octave: voice.octave2,
            semitone: voice.semitone2,
            cents: clamp(cents2, -100.0, 100.0),
            level: clamp(osc2_level, 0.0, 1.0),
            unison: 1,
            detune: 0.0,
            spread: clamp(voice.spread * 0.6, 0.0, 1.0),
            ..Oscillator::default()
        },
        sub: clamp(voice.sub, 0.0, 1.0),
        noise: clamp(noise, 0.0, 1.0),
        filter: Filter {
            kind: voice.filter,
            cutoff: clamp(cutoff, 20.0, 20_000.0),
            resonance: clamp(resonance, 0.0, 1.0),
            env_amount: clamp(env_amount, -4.0, 4.0),
            key_track: clamp(voice.key_track, 0.0, 1.0),
            drive: clamp(drive, 0.0, 1.0),
        },
        amp: Envelope {
            attack: clamp(amp.attack, 0.001, 5.0),
            decay: clamp(amp.decay, 0.001, 10.0),
            sustain: clamp(amp.sustain, 0.0, 1.0),
            release: clamp(amp.release, 0.005, 10.0),
        },
        filter_env: Envelope {
            attack: clamp(filter_env.attack, 0.001, 5.0),
            decay: clamp(filter_env.decay, 0.001, 10.0),
            sustain: clamp(filter_env.sustain, 0.0, 1.0),
            release: clamp(filter_env.release, 0.005, 10.0),
        },
        lfo: Lfo {
            wave: LfoWave::Sine,
            rate: clamp(voice.lfo_rate, 0.05, 20.0),
            depth: clamp(lfo_depth, 0.0, 1.0),
            target: voice.lfo_target,
        },
        lfo2,
        mods,
        macros: [0.0; 4],
        tuning: Vec::new(),
    }
}

/// Every seed in the catalogue, in the order the page lists them.
pub fn seeds() -> Vec<&'static Seed> {
    let mut all: Vec<&'static Seed> = Vec::with_capacity(200);
    all.extend(set_a::SEEDS.iter());
    all.extend(set_b::SEEDS.iter());
    all.extend(set_c::SEEDS.iter());
    all.extend(set_d::SEEDS.iter());
    all
}

/// The id a pack's patch is saved and loaded by. It names the pack and which of the
/// four it is, so a loaded patch can still say where it came from.
pub fn patch_id(seed: &Seed, index: usize) -> String {
    format!("pack:{}#{index}", seed.id)
}

/// The patch behind an id, or nothing when the id is not one of the catalogue's: a
/// curated pack (`pack:<pack>#<n>`) or a cell of the coverage grid
/// (`grid:<family>:<wave>:<wave>:<filter>#<n>`).
pub fn find(id: &str) -> Option<SynthConfig> {
    if id.starts_with("grid:") {
        let (cell, variant) = grid::parse(id)?;
        return Some(grid_patch(&cell, variant));
    }
    let rest = id.strip_prefix("pack:")?;
    let (pack, index) = rest.split_once('#')?;
    let index: usize = index.parse().ok()?;
    if index >= 4 {
        return None;
    }
    seeds()
        .into_iter()
        .find(|seed| seed.id == pack)
        .map(|seed| patch(seed, index))
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackPatchSummary {
    pub id: String,
    pub name: String,
}

/// A pack as the page reads it: no configuration, just what a card says.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackSummary {
    pub id: String,
    pub name: String,
    pub family: &'static str,
    /// Owned, because a curated pack's line is written into the binary and a grid
    /// cell's is composed from its coordinates.
    pub blurb: String,
    pub patches: Vec<PackPatchSummary>,
}

/// One line of the coverage report: how much of an axis the catalogue actually holds.
/// The numbers come from the packs, not from a constant, so the page cannot claim a
/// coverage the shelf does not have.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageRow {
    pub label: &'static str,
    pub covered: usize,
    pub total: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    pub rows: Vec<CoverageRow>,
    /// The grid's own totals, so a page can say what the systematic half is.
    pub grid_packs: usize,
    pub grid_patches: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalogue {
    /// The families in the order the filter row draws them, so the page never
    /// invents a family the engine does not have.
    pub families: Vec<&'static str>,
    pub packs: Vec<PackSummary>,
    pub coverage: Coverage,
}

fn seed_summary(seed: &'static Seed) -> PackSummary {
    PackSummary {
        id: seed.id.to_string(),
        name: seed.name.to_string(),
        family: seed.family.label(),
        blurb: seed.blurb.to_string(),
        patches: (0..4)
            .map(|index| PackPatchSummary {
                id: patch_id(seed, index),
                name: seed.patches[index].to_string(),
            })
            .collect(),
    }
}

fn grid_summary(cell: &grid::Cell) -> PackSummary {
    let names = grid::patch_names(cell);
    PackSummary {
        id: grid::id(cell, 0).trim_end_matches("#0").to_string(),
        name: grid::name(cell),
        family: cell.family.label(),
        blurb: grid::blurb(cell),
        patches: names
            .iter()
            .enumerate()
            .map(|(index, name)| PackPatchSummary {
                id: grid::id(cell, index),
                name: name.clone(),
            })
            .collect(),
    }
}

/// The shelf: each family's curated packs, then the coverage grid's cells for that
/// family. Grouping by family means the grid sits beside the taste it completes rather
/// than in a second catalogue nobody scrolls to.
pub fn catalogue() -> Catalogue {
    let seeds = seeds();
    let cells = grid::cells();
    let mut packs = Vec::with_capacity(seeds.len() + cells.len());
    for family in Family::ALL {
        packs.extend(seeds.iter().filter(|seed| seed.family == family).map(|seed| seed_summary(seed)));
        packs.extend(cells.iter().filter(|cell| cell.family == family).map(grid_summary));
    }

    // What the shelf covers, counted from the shelf.
    let families: std::collections::HashSet<&str> = packs.iter().map(|pack| pack.family).collect();
    let waves: std::collections::HashSet<&str> = cells.iter().map(|cell| grid::wave_label(cell.wave)).collect();
    let filters: std::collections::HashSet<&str> = cells.iter().map(|cell| grid::filter_label(cell.filter)).collect();
    let coverage = Coverage {
        rows: vec![
            CoverageRow { label: "Families", covered: families.len(), total: Family::ALL.len() },
            CoverageRow { label: "Oscillator waves", covered: waves.len(), total: grid::WAVES.len() },
            CoverageRow { label: "Filter models", covered: filters.len(), total: grid::FILTERS.len() },
            CoverageRow { label: "Grid cells", covered: cells.len(), total: cells.len() },
        ],
        grid_packs: cells.len(),
        grid_patches: cells.len() * 4,
    };
    Catalogue {
        families: Family::ALL.iter().map(|family| family.label()).collect(),
        packs,
        coverage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_is_every_curated_pack_and_every_grid_cell() {
        let all = seeds();
        assert!(
            (195..=205).contains(&all.len()),
            "the catalogue holds {} curated packs, not about two hundred",
            all.len()
        );
        for seed in &all {
            assert_eq!(seed.patches.len(), 4, "{} does not have four patches", seed.id);
        }
        let catalogue = catalogue();
        assert_eq!(catalogue.packs.len(), all.len() + grid::cells().len());
        assert_eq!(catalogue.coverage.grid_packs, grid::cells().len());
        assert_eq!(catalogue.coverage.grid_patches, grid::cells().len() * 4);
    }

    #[test]
    fn the_shelf_is_five_hundred_and_thirty_six_packs_of_two_thousand_patches() {
        let catalogue = catalogue();
        assert_eq!(catalogue.packs.len(), 536, "the shelf changed size");
        assert_eq!(grid::cells().len(), 336, "the grid changed size");
        let patches: usize = catalogue.packs.iter().map(|pack| pack.patches.len()).sum();
        assert_eq!(patches, 2_144, "the shelf no longer holds 2,144 patches");
        assert!(catalogue
            .packs
            .iter()
            .all(|pack| !pack.patches.is_empty() && pack.patches.len() == 4));
    }

    #[test]
    fn the_shelf_covers_every_axis_it_claims() {
        let catalogue = catalogue();
        // Nothing is a gap: each row is full, and the grid row is its own total so a
        // cell that fell off the shelf would show as 335 of 336 rather than hide.
        for row in &catalogue.coverage.rows {
            assert_eq!(row.covered, row.total, "{} is only {} of {}", row.label, row.covered, row.total);
        }
        // Every grid cell has a card, and every card's patches load by their id.
        let ids: std::collections::HashSet<&str> = catalogue.packs.iter().map(|pack| pack.id.as_str()).collect();
        for cell in grid::cells() {
            let pack = grid::id(&cell, 0);
            assert!(ids.contains(pack.trim_end_matches("#0")), "{} is not on the shelf", grid::name(&cell));
            for variant in 0..4 {
                assert!(find(&grid::id(&cell, variant)).is_some(), "{} does not load", grid::id(&cell, variant));
            }
        }
    }

    #[test]
    fn every_curated_pack_has_its_own_id_and_name() {
        let all = seeds();
        let mut ids = std::collections::HashSet::new();
        let mut names = std::collections::HashSet::new();
        for seed in &all {
            assert!(ids.insert(seed.id), "the id {} is used twice", seed.id);
            assert!(names.insert(seed.name), "the name {} is used twice", seed.name);
            assert!(
                seed.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "the id {} is not kebab-case",
                seed.id
            );
            assert_eq!(
                seed.id,
                seed.name.to_lowercase().replace(' ', "-").replace(|c: char| !c.is_ascii_alphanumeric() && c != '-', ""),
                "{} is not the kebab-case of {}",
                seed.id,
                seed.name
            );
            assert!(!seed.blurb.trim().is_empty(), "{} has no blurb", seed.id);
            assert!(seed.patches.iter().all(|name| !name.trim().is_empty()));
        }
    }

    #[test]
    fn every_family_has_packs_to_show() {
        for family in Family::ALL {
            let count = seeds().into_iter().filter(|seed| seed.family == family).count();
            assert!(count >= 4, "{} holds only {count} packs", family.label());
        }
    }

    #[test]
    fn every_patch_the_catalogue_names_passes_the_engine() {
        for seed in seeds() {
            for index in 0..4 {
                let patch = patch(seed, index);
                patch
                    .validate()
                    .unwrap_or_else(|reason| panic!("{} #{} is not a patch: {reason}", seed.id, index));
            }
        }
        // The grid is the half that claims completeness, so it is the half that must
        // be playable in every cell and every take.
        for cell in grid::cells() {
            for variant in 0..4 {
                grid_patch(&cell, variant).validate().unwrap_or_else(|reason| panic!(
                    "{} #{} is not a patch: {reason}",
                    grid::name(&cell),
                    variant
                ));
            }
        }
    }

    #[test]
    fn a_grid_cell_plays_the_wave_and_filter_it_is_named_for() {
        for cell in grid::cells() {
            let patch = grid_patch(&cell, 0);
            assert_eq!(patch.osc1.wave, cell.wave, "{}", grid::name(&cell));
            assert_eq!(patch.filter.kind, cell.filter, "{}", grid::name(&cell));
            // A cell that lands on the family's own second wave is two oscillators only
            // because the second one moved an octave, so it is not one in disguise.
            if patch.osc1.wave == patch.osc2.wave {
                assert_ne!(patch.osc1.octave, patch.osc2.octave, "{}", grid::name(&cell));
            }
        }
    }

    #[test]
    fn the_four_patches_of_a_pack_are_four_different_patches() {
        let differ = |owner: &str, patches: Vec<SynthConfig>| {
            for a in 0..4 {
                for b in (a + 1)..4 {
                    assert_ne!(patches[a], patches[b], "{owner} #{} and #{} are the same", a, b);
                }
            }
        };
        for seed in seeds() {
            differ(seed.id, (0..4).map(|index| patch(seed, index)).collect());
        }
        for cell in grid::cells() {
            differ(&grid::name(&cell), (0..4).map(|variant| grid_patch(&cell, variant)).collect());
        }
    }

    #[test]
    fn an_id_finds_the_patch_it_names() {
        let seed = seeds()[0];
        for index in 0..4 {
            let found = find(&patch_id(seed, index)).expect("the id the catalogue hands out does not load");
            assert_eq!(found, patch(seed, index));
        }
        let cell = grid::cells()[grid::cells().len() / 2];
        for variant in 0..4 {
            let found = find(&grid::id(&cell, variant)).expect("a grid id does not load");
            assert_eq!(found, grid_patch(&cell, variant));
        }
    }

    #[test]
    fn an_id_that_names_nothing_is_not_a_patch() {
        assert!(find("pack:no-such-pack#0").is_none());
        assert!(find("pack:#0").is_none());
        assert!(find("pack:any-pack#4").is_none());
        assert!(find("pack:any-pack").is_none());
        assert!(find("builtin:init").is_none());
        assert!(find("").is_none());
    }

    #[test]
    fn the_page_gets_the_families_in_one_order() {
        let catalogue = catalogue();
        assert_eq!(catalogue.families.len(), Family::ALL.len());
        assert_eq!(catalogue.families[0], "Bass");
        assert_eq!(catalogue.packs[0].patches.len(), 4);
        assert!(catalogue.packs.iter().all(|pack| catalogue.families.contains(&pack.family)));
    }
}
