//! Room simulation for the monitored mix.
//!
//! This is a network reverb in the spirit of u-he's Protoverb: eight delay lines in
//! one feedback loop, an all-to-all (Householder) matrix that hands every line's
//! output to every other, a seeded pseudo-random choice of prime-length delays, and a
//! chain of diffusing allpasses on the way in. The matrix is what makes the tail dense
//! instead of a row of separate echoes — the thing a bank of parallel combs never does.
//! The network string is part of the sound, so the same string always rebuilds the
//! same room.
//!
//! It is honest about what it is not: there is no convolution, no sampled or licensed
//! impulse response, and no physical room model. `decay` is a reverb time in the
//! literal sense: every line's feedback is solved from its own length and the room
//! lasts as long as the control says, from a fifth of a second to sixteen. Each loop
//! also carries air absorption and a DC blocker, so the top dies before the bottom and
//! no standing level can outlive the room.
//!
//! Dry and wet are two gains, not a crossfade: raising the room never takes the dry
//! signal away, which is the arrangement a send would give it.

use serde::{Deserialize, Serialize};

/// The three routings the network can take. The first token of a network string.
pub const STRUCTURES: [&str; 3] = ["parallel", "series", "woven"];

const LINES: usize = 8;
const RATE: f32 = 48_000.0;
/// 12 ms fade when the network is rebuilt, so changing rooms never clicks.
const FADE_STEP: f32 = 1.0 / (0.012 * RATE);
const PRIME_LOW: usize = 181;
/// The longest line is 85 ms at 48 kHz: long enough that the first reflections read as
/// a room rather than as a short slap, which is the cheapest thing that makes a delay
/// network sound small.
const PRIME_HIGH: usize = 4099;
/// Above this a loop would ring indefinitely instead of decaying.
const MAX_FEEDBACK: f32 = 0.9998;
/// The reverb time the control asks for at 0 and at 1, in seconds. It is a geometric
/// sweep, so the middle of the control is a usable hall rather than half of a cathedral.
const RT60_SHORT: f32 = 0.2;
const RT60_LONG: f32 = 16.0;
/// Air absorption in every loop: a one-pole low pass near 8 kHz at 48 kHz. A room does
/// not sustain its top as long as its bottom, and without this the tail rings at one
/// pitch instead of darkening.
const DAMPING: f32 = 0.35;
/// The loop's DC blocker, a first-order high pass near 5 Hz: a standing offset must not
/// survive in the feedback network after the signal that made it has gone.
const DC_POLE: f32 = 0.9993;
/// The wet return's own trim, so a room at 100 % sits at about the level of the signal
/// it is added to instead of whispering under it.
const WET_TRIM: f32 = 4.6;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ReverbConfig {
    pub enabled: bool,
    /// Reverb time, 0 = a fifth of a second, 1 = sixteen seconds, swept geometrically.
    pub decay: f32,
    /// The unprocessed signal's own level, so raising the room never takes it away.
    pub dry: f32,
    /// The room's level.
    pub wet: f32,
    /// "<structure>:<seed>", for example "woven:4211".
    pub network: String,
    /// How a room was saved before dry and wet were separate: one crossfade amount.
    /// Read on load so an existing session keeps its balance, and never written back.
    #[serde(rename = "mix", default, skip_serializing_if = "Option::is_none")]
    pub legacy_mix: Option<f32>,
}

impl Default for ReverbConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            decay: 0.55,
            dry: 1.0,
            wet: 0.3,
            network: "woven:4211".to_string(),
            legacy_mix: None,
        }
    }
}

impl ReverbConfig {
    /// Takes an old crossfade amount into the pair that replaced it, once.
    pub fn settle(&mut self) {
        if let Some(mix) = self.legacy_mix.take() {
            if mix.is_finite() {
                let crossfade = mix.clamp(0.0, 1.0);
                self.dry = 1.0 - crossfade;
                self.wet = crossfade;
            }
        }
    }
}

fn finite_in(value: f32, low: f32, high: f32) -> bool {
    value.is_finite() && (low..=high).contains(&value)
}

pub fn validate(config: &ReverbConfig) -> Result<(), String> {
    if !finite_in(config.decay, 0.0, 1.0) {
        return Err("Reverb decay must be between 0 and 1".into());
    }
    if !finite_in(config.dry, 0.0, 1.0) {
        return Err("Reverb dry must be between 0 and 1".into());
    }
    if !finite_in(config.wet, 0.0, 1.0) {
        return Err("Reverb wet must be between 0 and 1".into());
    }
    parse_network(&config.network)?;
    Ok(())
}

/// Splits "<structure>:<seed>" into a known routing and its seed.
pub fn parse_network(network: &str) -> Result<(&'static str, u32), String> {
    let trimmed = network.trim();
    let (structure, seed) = trimmed
        .split_once(':')
        .ok_or("Reverb network must look like \"woven:4211\" (structure:seed)")?;
    let structure = structure.trim().to_ascii_lowercase();
    let known = STRUCTURES
        .iter()
        .copied()
        .find(|name| *name == structure)
        .ok_or("Unknown reverb structure; use parallel, series or woven")?;
    let seed: u32 = seed
        .trim()
        .parse()
        .map_err(|_| "Reverb network seed must be a whole number")?;
    Ok((known, seed))
}

/// A fresh network string for a randomize control; pass a changing seed.
pub fn random_network(seed: u32) -> String {
    let mut state = seed | 1;
    let first = next(&mut state);
    let second = next(&mut state);
    let structure = STRUCTURES[(first % STRUCTURES.len() as u32) as usize];
    format!("{structure}:{}", 100 + second % 9_900)
}

/// The reverb time the control asks for, in seconds: 0.2 s at 0, 16 s at 1, swept
/// geometrically so the middle of the control is a hall and not half a cathedral.
pub fn reverb_time(decay: f32) -> f32 {
    let swept = if decay.is_finite() { decay.clamp(0.0, 1.0) } else { 0.0 };
    RT60_SHORT * (RT60_LONG / RT60_SHORT).powf(swept)
}

/// The feedback that makes a loop of `length` samples lose 60 dB in `rt60` seconds.
fn feedback(length: usize, rt60: f32) -> f32 {
    let seconds = length.max(1) as f32 / RATE;
    (10f32.powf(-3.0 * seconds / rt60.max(1e-3))).clamp(0.0, MAX_FEEDBACK)
}

fn next(state: &mut u32) -> u32 {
    if *state == 0 {
        *state = 0x9e37_79b9;
    }
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

fn prime_pool() -> Vec<usize> {
    let mut pool = Vec::new();
    for candidate in PRIME_LOW..PRIME_HIGH {
        if candidate % 2 == 0 {
            continue;
        }
        let mut prime = true;
        let mut divisor = 3;
        while divisor * divisor <= candidate {
            if candidate % divisor == 0 {
                prime = false;
                break;
            }
            divisor += 2;
        }
        if prime {
            pool.push(candidate);
        }
    }
    pool
}

struct Network {
    structure: usize,
    /// Each line's own reverb time as a multiple of the room's: the spread of a real
    /// room, and the reason two lines never decay in lockstep.
    spread: [f32; LINES],
    /// Where each line sits in the stereo field, -1 left to +1 right.
    pan: [f32; LINES],
    /// Line lengths chosen from the prime pool by the seed.
    lengths: [usize; LINES],
}

impl Network {
    fn build(structure: usize, seed: u32) -> Self {
        let pool = prime_pool();
        let mut state = seed | 1;
        let mut lengths = [0usize; LINES];
        let mut spread = [1.0f32; LINES];
        let mut pan = [0.0f32; LINES];
        let mut used = [usize::MAX; LINES];
        for index in 0..LINES {
            let mut attempt = 0;
            loop {
                let pick = pool[(next(&mut state) as usize) % pool.len()];
                if !used[..index].contains(&pick) || attempt > pool.len() {
                    lengths[index] = pick;
                    used[index] = pick;
                    break;
                }
                attempt += 1;
            }
            let jitter = (next(&mut state) % 1001) as f32 / 1000.0;
            spread[index] = 0.82 + 0.36 * jitter;
            let spread = (next(&mut state) % 1001) as f32 / 1000.0;
            pan[index] = match structure {
                0 => spread * 2.0 - 1.0,
                // Series and woven alternate ears line by line.
                _ => {
                    if index % 2 == 0 {
                        -spread
                    } else {
                        spread
                    }
                }
            };
        }
        Self {
            structure,
            spread,
            pan,
            lengths,
        }
    }

    /// Hands each line what its own output becomes on the next pass. Every routing is
    /// unitary: the matrix moves energy between lines but never adds any, so the room
    /// still lasts exactly as long as the feedback gains ask it to.
    fn mix(&self, delayed: &[f32; LINES]) -> [f32; LINES] {
        let mut mixed = [0.0f32; LINES];
        match self.structure {
            // Series: the room is one loop, each line feeding the next.
            1 => {
                for index in 0..LINES {
                    mixed[index] = delayed[(index + LINES - 1) % LINES];
                }
                return mixed;
            }
            // Woven: the same all-to-all mixing, reached through a perfect shuffle, so a
            // line's energy lands two places away instead of on its neighbour.
            2 => {
                for index in 0..LINES {
                    let shuffled = (index * 2) % LINES + (index * 2) / LINES;
                    mixed[index] = delayed[shuffled];
                }
            }
            // Parallel: every line hands its output to every other, evenly.
            _ => mixed = *delayed,
        }
        // Householder: x - 2/N * sum(x). Orthogonal, so it decorrelates all eight lines
        // without colouring any frequency — the density of a Protoverb-class room.
        let bias = 2.0 * mixed.iter().sum::<f32>() / LINES as f32;
        for value in &mut mixed {
            *value -= bias;
        }
        mixed
    }
}

/// One loop of the network: its delay, the air absorption every pass loses, and a DC
/// blocker so a standing offset cannot outlive the room.
struct Line {
    buffer: Vec<f32>,
    index: usize,
    damped: f32,
    dc_in: f32,
    dc_out: f32,
}

impl Line {
    fn new(length: usize) -> Self {
        Self {
            buffer: vec![0.0; length.max(1)],
            index: 0,
            damped: 0.0,
            dc_in: 0.0,
            dc_out: 0.0,
        }
    }

    /// Takes a new length for a rebuilt room and empties what the old one held: the
    /// same string rebuilds the same silence it started from.
    fn resize(&mut self, length: usize) {
        self.buffer.clear();
        self.buffer.resize(length.max(1), 0.0);
        self.index = 0;
        self.damped = 0.0;
        self.dc_in = 0.0;
        self.dc_out = 0.0;
    }

    /// The sample that left the loop one lap ago, filtered the way a room filters it.
    /// Only non-finite values are caught here: clamping inside a feedback loop would be
    /// a distortion, and the output is clamped once, after the room.
    fn read(&mut self) -> f32 {
        let raw = self.buffer[self.index];
        let damped = raw * (1.0 - DAMPING) + self.damped * DAMPING;
        self.damped = finite_or_zero(damped);
        let out = damped - self.dc_in + self.dc_out * DC_POLE;
        self.dc_in = damped;
        self.dc_out = finite_or_zero(out);
        self.dc_out
    }

    fn write(&mut self, value: f32) {
        self.buffer[self.index] = finite_or_zero(value);
        self.index = (self.index + 1) % self.buffer.len();
    }
}

/// Schroeder allpass: smears the input so the network is not fed bare transients.
struct Allpass {
    buffer: Vec<f32>,
    index: usize,
    gain: f32,
}

impl Allpass {
    fn new(length: usize, gain: f32) -> Self {
        Self {
            buffer: vec![0.0; length.max(1)],
            index: 0,
            gain,
        }
    }
    fn process(&mut self, input: f32) -> f32 {
        let delayed = self.buffer[self.index];
        let stored = input + self.gain * delayed;
        self.buffer[self.index] = if stored.is_finite() { stored } else { 0.0 };
        self.index = (self.index + 1) % self.buffer.len();
        let out = delayed - self.gain * stored;
        if out.is_finite() {
            out
        } else {
            0.0
        }
    }
    fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.index = 0;
    }
}

pub struct Reverb {
    config: ReverbConfig,
    network: Network,
    lines: Vec<Line>,
    /// Two diffusers on each side, before the network: a bare transient into a delay
    /// loop is a click, and diffusion is what turns it into a room.
    pre_left: [Allpass; 2],
    pre_right: [Allpass; 2],
    /// And one more before every single line. Without it all eight lines receive the
    /// same signal at the same instant, so they add coherently and the room colours
    /// whatever frequency they happen to align on. One scattered allpass each keeps
    /// them independent.
    scatter: [Allpass; LINES],
    /// Diffusers inside the loop, one per line. A feedback matrix of bare delays still
    /// has deep notches between its modes — a sustained note can fall into one and
    /// disappear — and raising the decay deepens them. Diffusing the signal on every
    /// pass is what fills the notches in: the dense, smooth tail a Protoverb has,
    /// rather than the metallic ring of a comb with a rotation on it.
    diffuse: [Allpass; LINES],
    post_left: [Allpass; 1],
    post_right: [Allpass; 1],
    pending: Option<ReverbConfig>,
    /// 1 = the current network is fully faded in, 0 = silent rebuild point.
    fade: f32,
    /// The decay the figures below were solved for, so a knob that is still costs
    /// nothing; -1 means they have to be solved again.
    solved: f32,
    /// Per line: the feedback that spends the decay, and the tap weight that makes the
    /// room no quieter for being longer.
    gain: [f32; LINES],
    level: [f32; LINES],
}

impl Reverb {
    pub fn new(config: ReverbConfig) -> Self {
        let (structure, seed) = parse_network(&config.network).unwrap_or(("woven", 4211));
        let network = Network::build(structure_index(structure), seed);
        let lines = network
            .lengths
            .iter()
            .map(|length| Line::new(*length))
            .collect();
        Self {
            config,
            network,
            lines,
            pre_left: [Allpass::new(89, 0.62), Allpass::new(223, 0.62)],
            pre_right: [Allpass::new(97, 0.62), Allpass::new(233, 0.62)],
            scatter: [
                Allpass::new(43, 0.55),
                Allpass::new(59, 0.55),
                Allpass::new(71, 0.55),
                Allpass::new(101, 0.55),
                Allpass::new(113, 0.55),
                Allpass::new(131, 0.55),
                Allpass::new(149, 0.55),
                Allpass::new(173, 0.55),
            ],
            diffuse: [
                Allpass::new(157, 0.5),
                Allpass::new(191, 0.5),
                Allpass::new(229, 0.5),
                Allpass::new(271, 0.5),
                Allpass::new(313, 0.5),
                Allpass::new(359, 0.5),
                Allpass::new(401, 0.5),
                Allpass::new(449, 0.5),
            ],
            post_left: [Allpass::new(211, 0.5)],
            post_right: [Allpass::new(227, 0.5)],
            pending: None,
            fade: 1.0,
            solved: -1.0,
            gain: [0.0; LINES],
            level: [0.0; LINES],
        }
    }

    fn rebuild(&mut self, config: &ReverbConfig) {
        let (structure, seed) = parse_network(&config.network).unwrap_or(("woven", 4211));
        let network = Network::build(structure_index(structure), seed);
        for (line, length) in self.lines.iter_mut().zip(network.lengths.iter()) {
            line.resize(*length);
        }
        for allpass in &mut self.pre_left {
            allpass.clear();
        }
        for allpass in &mut self.pre_right {
            allpass.clear();
        }
        for allpass in &mut self.scatter {
            allpass.clear();
        }
        for allpass in &mut self.diffuse {
            allpass.clear();
        }
        for allpass in &mut self.post_left {
            allpass.clear();
        }
        for allpass in &mut self.post_right {
            allpass.clear();
        }
        self.network = network;
        // New line lengths, new feedback: the figures have to be solved again.
        self.solved = -1.0;
    }

    /// Solves every line's feedback from its own length and the reverb time the control
    /// asks for, and the tap weight that keeps the room's level out of the decay's hands.
    fn solve(&mut self, decay: f32) {
        let decay = if decay.is_finite() {
            decay.clamp(0.0, 1.0)
        } else {
            0.0
        };
        if (decay - self.solved).abs() < 1e-5 {
            return;
        }
        let rt60 = reverb_time(decay);
        for index in 0..LINES {
            let gain = feedback(self.network.lengths[index], rt60 * self.network.spread[index]);
            self.gain[index] = gain;
            // What the loop will not carry is what the tap gives back: a sixteen-second
            // room is not a quieter one than a one-second room.
            self.level[index] = (1.0f32 - gain * gain).max(0.0).sqrt();
        }
        self.solved = decay;
    }

    /// One stereo frame in, one reverberated stereo frame out.
    ///
    /// Takes the configuration by reference so the render thread never clones the
    /// network string per frame.
    pub fn process(&mut self, frame: [f32; 2], config: &ReverbConfig) -> [f32; 2] {
        if config.network != self.config.network {
            if self.pending.is_none() {
                self.pending = Some(config.clone());
            }
        } else {
            self.config.enabled = config.enabled;
            self.config.decay = config.decay;
            self.config.dry = config.dry;
            self.config.wet = config.wet;
        }
        if let Some(next) = self.pending.clone() {
            self.fade = (self.fade - FADE_STEP).max(0.0);
            if self.fade == 0.0 {
                self.rebuild(&next);
                self.config = next;
                self.pending = None;
            }
        } else if self.fade < 1.0 {
            self.fade = (self.fade + FADE_STEP).min(1.0);
        }

        if !self.config.enabled {
            return frame;
        }
        let dry = self.config.dry.clamp(0.0, 1.0);
        // A rebuilt network fades the room in, not the dry signal out: the two gains are
        // independent, so changing rooms never moves the level of what was playing.
        let wet = (self.config.wet * self.fade).clamp(0.0, 1.0);
        if wet == 0.0 {
            return if dry == 1.0 {
                frame
            } else {
                [clean(frame[0] * dry), clean(frame[1] * dry)]
            };
        }
        let roomed = self.wet(frame, self.config.decay);
        [
            (frame[0] * dry + roomed[0] * wet).clamp(-1.0, 1.0),
            (frame[1] * dry + roomed[1] * wet).clamp(-1.0, 1.0),
        ]
    }

    fn wet(&mut self, frame: [f32; 2], decay: f32) -> [f32; 2] {
        self.solve(decay);
        let gain = self.gain;
        let level = self.level;
        let pan = self.network.pan;

        // True stereo in: each side gets its own diffusion chain, so a side-only signal
        // still enters the room and hard-panned material comes back wide instead of
        // collapsing to the centre.
        let mut left_in = frame[0];
        let mut right_in = frame[1];
        for allpass in &mut self.pre_left {
            left_in = allpass.process(left_in);
        }
        for allpass in &mut self.pre_right {
            right_in = allpass.process(right_in);
        }

        // Every line is read before any is written: the matrix needs the whole field at
        // once, which is the difference between a room and eight separate echoes.
        let mut delayed = [0.0f32; LINES];
        for index in 0..LINES {
            delayed[index] = self.lines[index].read();
        }
        let mixed = self.network.mix(&delayed);

        // Half the lines take the left side and half the right, at the energy one line
        // needs; the matrix spreads each side through the whole room from there.
        let input = (2.0 / LINES as f32).sqrt();
        let mut left = 0.0f32;
        let mut right = 0.0f32;
        for index in 0..LINES {
            let side = if index % 2 == 0 { left_in } else { right_in } * input;
            let fed = self.scatter[index].process(side);
            // The loop's own diffuser sits after the matrix, so every pass through the
            // room is smeared again rather than repeated as a comb tooth.
            let carried = self.diffuse[index].process(mixed[index]);
            self.lines[index].write(fed + gain[index] * carried);
            let tap = delayed[index] * level[index];
            left += tap * (0.5 + 0.5 * pan[index]);
            right += tap * (0.5 - 0.5 * pan[index]);
        }

        for allpass in &mut self.post_left {
            left = allpass.process(left);
        }
        for allpass in &mut self.post_right {
            right = allpass.process(right);
        }
        let trim = WET_TRIM / (LINES as f32).sqrt();
        [clean(left * trim), clean(right * trim)]
    }
}

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

fn clean(value: f32) -> f32 {
    if value.is_finite() {
        if value.abs() < 1e-20 {
            0.0
        } else {
            value.clamp(-1.0, 1.0)
        }
    } else {
        0.0
    }
}

fn structure_index(name: &str) -> usize {
    STRUCTURES
        .iter()
        .position(|item| *item == name)
        .unwrap_or(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ReverbConfig {
        ReverbConfig {
            enabled: true,
            decay: 0.6,
            dry: 0.5,
            wet: 0.5,
            network: "woven:4211".to_string(),
            legacy_mix: None,
        }
    }

    fn run(settings: &ReverbConfig, mut input: impl FnMut(usize) -> [f32; 2], frames: usize) -> Vec<[f32; 2]> {
        let mut reverb = Reverb::new(settings.clone());
        (0..frames)
            .map(|index| reverb.process(input(index), &settings))
            .collect()
    }

    fn peak(frames: &[[f32; 2]], from: usize, to: usize) -> f32 {
        frames[from..to]
            .iter()
            .fold(0.0f32, |acc, frame| acc.max(frame[0].abs().max(frame[1].abs())))
    }

    fn energy(frames: &[[f32; 2]], from: usize, to: usize) -> f64 {
        frames[from..to]
            .iter()
            .map(|frame| (frame[0] as f64).powi(2) + (frame[1] as f64).powi(2))
            .sum()
    }

    #[test]
    fn disabling_or_turning_the_room_down_is_an_exact_pass_through() {
        let input = [[0.25f32, -0.5f32], [1.0, -1.0], [0.0, 0.125]];
        let mut off = ReverbConfig::default();
        off.enabled = false;
        let mut reverb = Reverb::new(off.clone());
        for frame in input {
            assert_eq!(reverb.process(frame, &off), frame);
        }
        // Wet at zero and dry at full is the untouched signal, bit for bit.
        let quiet = ReverbConfig { wet: 0.0, dry: 1.0, ..config() };
        let mut reverb = Reverb::new(quiet.clone());
        for frame in input {
            assert_eq!(reverb.process(frame, &quiet), frame);
        }
        // Dry alone is the input at its own level, with no room on it.
        let trimmed = ReverbConfig { wet: 0.0, dry: 0.5, ..config() };
        let mut reverb = Reverb::new(trimmed.clone());
        for frame in input {
            assert_eq!(reverb.process(frame, &trimmed), [frame[0] * 0.5, frame[1] * 0.5]);
        }
    }

    #[test]
    fn dry_and_wet_are_two_gains_not_one_crossfade() {
        // Full dry with a room on top is louder than the input, and the input is still
        // there at full level: the whole reason the two are separate controls.
        let together = ReverbConfig { dry: 1.0, wet: 1.0, ..config() };
        let alone = ReverbConfig { dry: 1.0, wet: 0.0, ..config() };
        let impulse = |index: usize| if index == 0 { [0.5, -0.5] } else { [0.0, 0.0] };
        let mixed = run(&together, impulse, 4_800);
        let dry = run(&alone, impulse, 4_800);
        assert!(energy(&mixed, 0, 4_800) > energy(&dry, 0, 4_800), "the room added nothing");
        assert_eq!(dry[0], [0.5, -0.5], "the dry signal was not the input at full level");
    }

    #[test]
    fn a_room_saved_as_one_crossfade_keeps_its_balance() {
        let mut saved = ReverbConfig { legacy_mix: Some(0.4), dry: 1.0, wet: 0.3, ..config() };
        saved.settle();
        assert_eq!(saved.legacy_mix, None);
        assert!((saved.dry - 0.6).abs() < 1e-6);
        assert!((saved.wet - 0.4).abs() < 1e-6);
        // Nothing saved: the pair is left as it is.
        let mut fresh = ReverbConfig::default();
        fresh.settle();
        assert_eq!((fresh.dry, fresh.wet), (1.0, 0.3));
    }

    #[test]
    fn silence_stays_exactly_silent() {
        let settings = config();
        let frames = run(&settings, |_| [0.0, 0.0], 48_000);
        assert!(frames.iter().all(|frame| frame[0] == 0.0 && frame[1] == 0.0));
    }

    #[test]
    fn an_impulse_leaves_a_tail_that_decays_away() {
        let settings = config();
        let frames = run(
            &settings,
            |index| if index == 0 { [1.0, 1.0] } else { [0.0, 0.0] },
            48_000 * 6,
        );
        assert!(
            peak(&frames, 9_600, 9_664) > 1e-4,
            "no tail at 200 ms: {}",
            peak(&frames, 9_600, 9_664)
        );
        assert!(
            peak(&frames, 48_000 * 6 - 4_800, 48_000 * 6) < 1e-3,
            "tail never died: {}",
            peak(&frames, 48_000 * 6 - 4_800, 48_000 * 6)
        );
        assert!(frames
            .iter()
            .all(|frame| frame[0].is_finite() && frame[1].is_finite()));
    }

    #[test]
    fn more_decay_keeps_more_energy_later() {
        let long = ReverbConfig { decay: 0.9, ..config() };
        let short = ReverbConfig { decay: 0.2, ..config() };
        let impulse = |index: usize| if index == 0 { [1.0, -1.0] } else { [0.0, 0.0] };
        let long_frames = run(&long, impulse, 98_000);
        let short_frames = run(&short, impulse, 98_000);
        let window = (96_000, 96_384);
        assert!(
            energy(&long_frames, window.0, window.1) > energy(&short_frames, window.0, window.1),
            "decay did not change the tail: {} vs {}",
            energy(&long_frames, window.0, window.1),
            energy(&short_frames, window.0, window.1)
        );
    }

    #[test]
    fn the_seed_is_the_sound_and_the_same_seed_repeats() {
        let first = ReverbConfig { network: "parallel:7".to_string(), ..config() };
        let same = ReverbConfig { network: "parallel:7".to_string(), ..config() };
        let other = ReverbConfig { network: "parallel:8".to_string(), ..config() };
        let impulse = |index: usize| if index == 0 { [0.8, -0.3] } else { [0.0, 0.0] };
        let a = run(&first, impulse, 24_000);
        let b = run(&same, impulse, 24_000);
        let c = run(&other, impulse, 24_000);
        assert_eq!(a, b, "the same network string must rebuild the same room");
        let difference: f32 = a
            .iter()
            .zip(c.iter())
            .map(|(x, y)| (x[0] - y[0]).abs() + (x[1] - y[1]).abs())
            .sum();
        assert!(difference > 1e-4, "the seed made no difference: {difference}");
    }

    #[test]
    fn every_structure_stays_bounded_and_finite() {
        for network in ["parallel:3", "series:11", "woven:4211"] {
            let settings = ReverbConfig {
                network: network.to_string(),
                decay: 1.0,
                dry: 1.0,
                wet: 1.0,
                ..config()
            };
            let frames = run(
                &settings,
                |index| if index % 2 == 0 { [1.0, -1.0] } else { [-1.0, 1.0] },
                48_000,
            );
            for frame in &frames {
                assert!(frame[0].is_finite() && frame[1].is_finite(), "{network}");
                assert!(
                    frame[0].abs() <= 1.0 && frame[1].abs() <= 1.0,
                    "{network} left full scale: {frame:?}"
                );
            }
        }
    }

    #[test]
    fn the_series_chain_does_not_saturate_into_a_screech() {
        // The series routing used to be eight comb stages in a row, each with its own
        // feedback peak, multiplied without compensation: it pinned the output to full
        // scale and clipped it into a harsh screech at ordinary listening levels. It is a
        // cyclic matrix in a loop now, and this keeps it that way.
        let settings = ReverbConfig {
            network: "series:11".to_string(),
            decay: 0.62,
            dry: 0.0,
            wet: 1.0,
            ..config()
        };
        let frames = run(
            &settings,
            |index| {
                let time = index as f32 / RATE;
                let tone = |frequency: f32| (time * frequency * std::f32::consts::TAU).sin() * 0.25;
                [tone(440.0), tone(330.0)]
            },
            48_000,
        );
        let pinned = frames
            .iter()
            .filter(|frame| frame[0].abs() >= 0.999 || frame[1].abs() >= 0.999)
            .count();
        assert_eq!(pinned, 0, "series pinned {pinned} frames at full scale");
        let late = peak(&frames, 24_000, 48_000);
        assert!(late < 0.9, "series is still shouting: {late}");
        assert!(late > 1e-3, "series went silent: {late}");
    }

    #[test]
    fn the_series_room_is_a_room_not_a_wall() {
        let wet = ReverbConfig {
            network: "series:11".to_string(),
            decay: 0.62,
            dry: 0.0,
            wet: 1.0,
            ..config()
        };
        let dry = ReverbConfig { wet: 0.0, dry: 1.0, ..wet.clone() };
        let impulse = |index: usize| if index == 0 { [1.0, -1.0] } else { [0.0, 0.0] };
        let room = run(&wet, impulse, 48_000);
        let reference = run(&dry, impulse, 48_000);
        let difference: f32 = room
            .iter()
            .zip(reference.iter())
            .map(|(a, b)| (a[0] - b[0]).abs() + (a[1] - b[1]).abs())
            .sum();
        assert!(
            difference > 0.5,
            "series is indistinguishable from dry: {difference}"
        );
        // No zero-delay leak: the room answers from its shortest line. From there the
        // diffusion and the feedback matrix fill every sample, which is the difference
        // between a room and a chain of separate echoes.
        assert!(
            room[0..PRIME_LOW].iter().all(|frame| frame[0] == 0.0 && frame[1] == 0.0),
            "the room leaked before its shortest line"
        );
        let tail = &room[24_000..48_000];
        let sounding = tail
            .iter()
            .filter(|frame| frame[0] != 0.0 || frame[1] != 0.0)
            .count();
        assert!(
            sounding as f32 > tail.len() as f32 * 0.99,
            "only {sounding} of {} tail frames carried the room" , tail.len()
        );
        let short = run(&ReverbConfig { decay: 0.05, ..wet.clone() }, impulse, 48_000);
        let long = run(&ReverbConfig { decay: 0.95, ..wet.clone() }, impulse, 48_000);
        let window = (40_000, 44_000);
        assert!(
            energy(&long, window.0, window.1) > energy(&short, window.0, window.1) * 2.0,
            "decay did not lengthen the series tail: {} vs {}",
            energy(&long, window.0, window.1),
            energy(&short, window.0, window.1)
        );
        assert!(
            energy(&short, window.0, window.1) < energy(&short, 0, 2_000) * 0.5,
            "the short series room never died"
        );
    }

    #[test]
    fn every_structure_keeps_its_gain_in_the_same_bracket() {
        // The three routings should differ in character, not in level by 60 dB.
        let settings = |network: &str| ReverbConfig {
            network: network.to_string(),
            decay: 0.62,
            dry: 0.0,
            wet: 1.0,
            ..config()
        };
        let levels: Vec<f32> = ["parallel:11", "series:11", "woven:11"]
            .iter()
            .map(|network| {
                let frames = run(
                    &settings(network),
                    |index| {
                        let time = index as f32 / RATE;
                        let tone = |frequency: f32| (time * frequency * std::f32::consts::TAU).sin() * 0.25;
                        [tone(440.0), tone(330.0)]
                    },
                    48_000,
                );
                peak(&frames, 24_000, 48_000)
            })
            .collect();
        let loudest = levels.iter().copied().fold(0.0f32, f32::max);
        let quietest = levels.iter().copied().fold(f32::INFINITY, f32::min);
        assert!(
            loudest / quietest < 6.0,
            "routings are far apart in level: {levels:?}"
        );
    }

    #[test]
    fn network_strings_are_parsed_or_rejected() {
        assert_eq!(parse_network("parallel:7").unwrap(), ("parallel", 7));
        assert_eq!(parse_network("SERIES:12").unwrap(), ("series", 12));
        assert_eq!(parse_network(" woven:4211 ").unwrap(), ("woven", 4211));
        assert!(parse_network("hall:3").is_err());
        assert!(parse_network("woven").is_err());
        assert!(parse_network("woven:").is_err());
        assert!(parse_network("woven:abc").is_err());
        assert!(parse_network(":5").is_err());
    }

    #[test]
    fn invalid_parameters_are_rejected() {
        let base = config();
        assert!(validate(&base).is_ok());
        for bad in [
            ReverbConfig { decay: 1.5, ..base.clone() },
            ReverbConfig { decay: f32::NAN, ..base.clone() },
            ReverbConfig { dry: -0.1, ..base.clone() },
            ReverbConfig { wet: f32::INFINITY, ..base.clone() },
            ReverbConfig { network: "hall:3".to_string(), ..base.clone() },
        ] {
            assert!(validate(&bad).is_err(), "accepted {bad:?}");
        }
    }

    /// The time the room takes to fall 60 dB, measured from its own output: the last
    /// 50 ms window above a millionth of the loudest window's energy.
    fn measured_rt60(settings: &ReverbConfig) -> f32 {
        let frames = run(
            settings,
            |index| if index == 0 { [1.0, 1.0] } else { [0.0, 0.0] },
            48_000 * 20,
        );
        let window = 2_400;
        let windows: Vec<f64> = frames
            .chunks(window)
            .map(|chunk| energy(chunk, 0, chunk.len()))
            .collect();
        let peak = windows.iter().copied().fold(0.0f64, f64::max).max(1e-12);
        let last = windows
            .iter()
            .rposition(|value| *value / peak > 1e-6)
            .unwrap_or(0);
        last as f32 * window as f32 / RATE
    }

    #[test]
    fn the_decay_control_is_a_reverb_time() {
        assert!((reverb_time(0.0) - RT60_SHORT).abs() < 1e-3);
        assert!((reverb_time(1.0) - RT60_LONG).abs() < 1e-3);
        // A sweep, not a ramp: the middle of the control is a real hall.
        assert!(reverb_time(0.5) > 1.4 && reverb_time(0.5) < 2.4);
        // The top of the control is a cathedral, not a six-second room.
        assert!(reverb_time(1.0) >= 12.0);
        let mut previous = 0.0;
        for step in 0..=20 {
            let value = reverb_time(step as f32 / 20.0);
            assert!(value > previous, "the control is not monotonic at {step}");
            previous = value;
        }
        assert_eq!(reverb_time(f32::NAN), RT60_SHORT);
        // A short line needs more feedback than a long one for the same reverb time.
        assert!(feedback(200, 2.0) > feedback(1_800, 2.0));
        assert!(feedback(1_800, 2.0) < 1.0);
    }

    #[test]
    fn the_room_lasts_as_long_as_the_control_says() {
        for network in ["woven:4211", "parallel:11", "series:11"] {
            let at = |decay: f32| measured_rt60(&ReverbConfig {
                network: network.to_string(),
                decay,
                dry: 0.0,
                wet: 1.0,
                ..config()
            });
            let short = at(0.1);
            let middle = at(0.5);
            let long = at(0.9);
            let asked = |decay: f32| reverb_time(decay);
            println!("{network}: {short:.2} s · {middle:.2} s · {long:.2} s");
            // The room lasts what the knob says, within the spread the lines carry.
            for (measured, decay) in [(short, 0.1), (middle, 0.5), (long, 0.9)] {
                let wanted = asked(decay);
                assert!(
                    measured > wanted * 0.6 && measured < wanted * 1.6,
                    "{network} asked {wanted:.2} s and measured {measured:.2} s"
                );
            }
            assert!(short < 1.0, "{network} short room lasted {short:.2} s");
            assert!(middle > short, "{network} mid {middle:.2} s vs short {short:.2} s");
            // The point of the change: the top of the control is a cathedral.
            assert!(long > 6.0, "{network} long room only lasted {long:.2} s");
            assert!(long > middle * 1.5, "{network} long {long:.2} s vs mid {middle:.2} s");
        }
    }

    /// RMS of a steady tone over the second half of a run: what the stage holds once
    /// its room has filled, not the transient that filled it.
    fn held_level(settings: &ReverbConfig, frames: usize) -> f32 {
        // Broadband noise, not one tone: a single sine can land in one of the room's
        // notches, and then the reading says more about that frequency than the room.
        let mut state = 20_260_107u32;
        let mut noise = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state as f32 / u32::MAX as f32) * 2.0 - 1.0
        };
        let out = run(
            settings,
            |_| [noise() * 0.2, noise() * 0.2],
            frames,
        );
        let from = frames / 2;
        let energy: f64 = out[from..]
            .iter()
            .map(|frame| (frame[0] as f64).powi(2) + (frame[1] as f64).powi(2))
            .sum();
        (energy / (2.0 * (frames - from) as f64)).sqrt() as f32
    }

    #[test]
    fn the_room_is_heard_at_its_own_level_whatever_its_length() {
        // The complaint this answers: a long decay used to be a quieter room, so turning
        // the knob up made the reverb smaller instead of bigger.
        let dry = held_level(
            &ReverbConfig { dry: 1.0, wet: 0.0, ..config() },
            96_000,
        );
        let mut levels = Vec::new();
        for decay in [0.2f32, 0.5, 0.8] {
            let wet = held_level(
                &ReverbConfig { decay, dry: 0.0, wet: 1.0, ..config() },
                96_000,
            );
            levels.push(wet);
            assert!(
                wet > dry * 0.5 && wet < dry * 1.6,
                "decay {decay}: wet {wet:.3} against dry {dry:.3}"
            );
        }
        println!("dry {dry:.3} · wet at 0.2 {:.3} · 0.5 {:.3} · 0.8 {:.3}", levels[0], levels[1], levels[2]);
        let loudest = levels.iter().copied().fold(0.0f32, f32::max);
        let quietest = levels.iter().copied().fold(f32::INFINITY, f32::min);
        assert!(
            loudest / quietest < 2.5,
            "the room's level moves with its decay: {levels:?}"
        );
    }

    #[test]
    fn a_side_only_signal_still_enters_the_room() {
        // The room is fed true stereo. A source that exists only in the side channel used
        // to vanish from the parallel routing entirely, which put a hole in the image.
        let side = ReverbConfig { dry: 0.0, wet: 1.0, ..config() };
        let out = run(
            &side,
            |index| {
                let time = index as f32 / RATE;
                let tone = (time * 330.0 * std::f32::consts::TAU).sin() * 0.2;
                [tone, -tone]
            },
            48_000,
        );
        let late = energy(&out, 24_000, 48_000);
        assert!(late > 1e-3, "the side channel never reached the room: {late:e}");
        let left: f64 = out[24_000..].iter().map(|f| (f[0] as f64).powi(2)).sum();
        let right: f64 = out[24_000..].iter().map(|f| (f[1] as f64).powi(2)).sum();
        assert!(left > 1e-4 && right > 1e-4, "one ear got nothing: {left:e} {right:e}");
    }

    #[test]
    fn the_tail_darkens_as_it_decays() {
        // Air absorption in every loop: the top of a room dies before its bottom, which
        // is most of what separates a room from a delay line ringing at one pitch.
        let frames = run(
            &ReverbConfig { decay: 0.85, dry: 0.0, wet: 1.0, ..config() },
            |index| if index == 0 { [1.0, 1.0] } else { [0.0, 0.0] },
            96_000,
        );
        let brightness = |from: usize, to: usize| {
            let mut high = 0.0f64;
            let mut all = 0.0f64;
            for pair in frames[from..to].windows(2) {
                let a = pair[0][0] as f64;
                let b = pair[1][0] as f64;
                high += (b - a).powi(2);
                all += a * a;
            }
            high / all.max(1e-30)
        };
        let early = brightness(240, 24_000);
        let late = brightness(66_000, 90_000);
        assert!(
            early > late * 2.0,
            "the tail did not darken: {early:e} early against {late:e} late"
        );
    }

    #[test]
    fn a_standing_level_cannot_outlive_the_room() {
        // A DC blocker in every loop: a constant input, held for a second, must not leave
        // an offset behind once the room around it has died. The mean of a dying tail is
        // zero; the mean of a standing level is the level itself, which is what this reads.
        let frames = run(
            &ReverbConfig { decay: 0.02, dry: 0.0, wet: 1.0, ..config() },
            |index| if index < 48_000 { [0.2, 0.2] } else { [0.0, 0.0] },
            96_000,
        );
        let window = &frames[72_000..];
        let mean = window.iter().map(|frame| frame[0] as f64).sum::<f64>() / window.len() as f64;
        let right = window.iter().map(|frame| frame[1] as f64).sum::<f64>() / window.len() as f64;
        // Below -66 dB of the level that was fed in: a standing offset would sit at the
        // level itself, not a thousandth of it.
        assert!(
            mean.abs() < 1e-4 && right.abs() < 1e-4,
            "a standing level survived the room: {mean:e} / {right:e}"
        );
    }

    #[test]
    fn randomized_networks_are_always_valid_and_change() {
        let first = random_network(1);
        let second = random_network(2);
        assert!(parse_network(&first).is_ok(), "{first}");
        assert!(parse_network(&second).is_ok(), "{second}");
        assert_ne!(first, second);
    }
}



