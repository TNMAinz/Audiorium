//! Additional stereo effects for the shared rack. Parameters are normalised so
//! the same validated configuration is used by live playback and offline renders.
use serde::{Deserialize, Serialize};

pub const COUNT: usize = 12;
const RATE: f32 = 48_000.0;
const DELAY_SIZE: usize = 96_002;
const HILBERT: usize = 127;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EffectConfig {
    pub enabled: bool,
    pub amount: f32,
    pub rate: f32,
    pub feedback: f32,
    pub mix: f32,
}

impl Default for EffectConfig {
    fn default() -> Self {
        Self { enabled: false, amount: 0.5, rate: 0.5, feedback: 0.25, mix: 0.5 }
    }
}

/// Conservative decay time to -60 dB for the longest feedback delay.
pub fn tail_frames(config: &[EffectConfig; COUNT]) -> usize {
    let delay = config[0];
    if !delay.enabled || delay.mix == 0.0 { return 0; }
    let feedback = delay.feedback * 0.9;
    let repeats = if feedback > 0.0 { (0.001_f32.ln() / feedback.ln()).ceil().max(1.0) } else { 1.0 };
    ((0.01 + 1.99 * delay.rate) * RATE * repeats) as usize
}

pub fn validate(config: &[EffectConfig; COUNT]) -> Result<(), String> {
    for (index, effect) in config.iter().enumerate() {
        for value in [effect.amount, effect.rate, effect.feedback, effect.mix] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(format!("Effect {} parameters must be between 0 and 1", index + 1));
            }
        }
    }
    Ok(())
}

struct Delay {
    samples: Vec<[f32; 2]>,
    cursor: usize,
    written: usize,
}

impl Delay {
    fn new(size: usize) -> Self { Self { samples: vec![[0.0; 2]; size], cursor: 0, written: 0 } }
    fn clear(&mut self) { self.written = 0; }
    fn read(&self, delay: f32) -> [f32; 2] {
        let delay = delay.clamp(1.0, (self.samples.len() - 2) as f32);
        let whole = delay.floor() as usize;
        let frac = delay - whole as f32;
        let a = if whole <= self.written { self.samples[(self.cursor + self.samples.len() - whole) % self.samples.len()] } else { [0.0; 2] };
        let b = if whole + 1 <= self.written { self.samples[(self.cursor + self.samples.len() - whole - 1) % self.samples.len()] } else { [0.0; 2] };
        [a[0] + (b[0] - a[0]) * frac, a[1] + (b[1] - a[1]) * frac]
    }
    fn push(&mut self, value: [f32; 2]) {
        self.samples[self.cursor] = value;
        self.cursor = (self.cursor + 1) % self.samples.len();
        self.written = (self.written + 1).min(self.samples.len());
    }
}

/// Each instance owns its delay and filter history; nothing is allocated per frame.
pub struct Effects {
    delay: Delay,
    chorus: Delay,
    flanger: Delay,
    phase: [f32; COUNT],
    phaser_x: [[f32; 4]; 2],
    phaser_y: [[f32; 4]; 2],
    phaser_feedback: [f32; 2],
    filter: [f32; 2],
    crush_hold: [f32; 2],
    crush_counter: usize,
    hilbert: Delay,
    hilbert_coefficients: [f32; HILBERT],
    gate_envelope: f32,
    gate_gain: f32,
    active: [bool; COUNT],
}

impl Effects {
    pub fn new() -> Self {
        let mut coefficients = [0.0; HILBERT];
        for (tap, coefficient) in coefficients.iter_mut().enumerate() {
            let offset = tap as isize - 63;
            if offset != 0 && offset % 2 != 0 {
                let window = 0.54 - 0.46 * (std::f32::consts::TAU * tap as f32 / (HILBERT - 1) as f32).cos();
                *coefficient = 2.0 * window / (std::f32::consts::PI * offset as f32);
            }
        }
        Self {
            delay: Delay::new(DELAY_SIZE), chorus: Delay::new(4096), flanger: Delay::new(2048),
            phase: [0.0; COUNT], phaser_x: [[0.0; 4]; 2], phaser_y: [[0.0; 4]; 2],
            phaser_feedback: [0.0; 2], filter: [0.0; 2], crush_hold: [0.0; 2],
            crush_counter: 0, hilbert: Delay::new(HILBERT + 2), hilbert_coefficients: coefficients,
            gate_envelope: 0.0, gate_gain: 0.0, active: [false; COUNT],
        }
    }

    fn lfo(&mut self, index: usize, hz: f32) -> f32 {
        self.phase[index] = (self.phase[index] + hz / RATE).fract();
        (std::f32::consts::TAU * self.phase[index]).sin()
    }

    fn reset(&mut self, index: usize) {
        self.phase[index] = 0.0;
        match index {
            0 => self.delay.clear(),
            1 => self.chorus.clear(),
            2 => self.flanger.clear(),
            3 => { self.phaser_x = [[0.0; 4]; 2]; self.phaser_y = [[0.0; 4]; 2]; self.phaser_feedback = [0.0; 2]; }
            4 => { self.crush_hold = [0.0; 2]; self.crush_counter = 0; }
            7 => self.filter = [0.0; 2],
            10 => self.hilbert.clear(),
            11 => { self.gate_envelope = 0.0; self.gate_gain = 0.0; }
            _ => {}
        }
    }

    pub fn process(&mut self, mut signal: [f32; 2], config: &[EffectConfig; COUNT]) -> [f32; 2] {
        for (index, effect) in config.iter().enumerate() {
            let active = effect.enabled && effect.mix > 0.0;
            if !active {
                if self.active[index] { self.reset(index); }
                self.active[index] = false;
                continue;
            }
            self.active[index] = true;
            let input = signal;
            let wet = match index {
                // Stereo delay, 10 ms–2 s, with cross-feedback.
                0 => {
                    let time = (0.01 + 1.99 * effect.rate) * RATE;
                    let out = self.delay.read(time);
                    let fb = effect.feedback * 0.9;
                    let cross = effect.amount;
                    self.delay.push([
                        input[0] + fb * (out[0] * (1.0 - cross) + out[1] * cross),
                        input[1] + fb * (out[1] * (1.0 - cross) + out[0] * cross),
                    ]);
                    out
                }
                // Chorus and flanger use interpolated modulated delays.
                1 | 2 => {
                    let lfo = self.lfo(index, 0.05 + effect.rate * 5.0);
                    let (base, depth) = if index == 1 { (900.0, 700.0) } else { (120.0, 100.0) };
                    let time = base + depth * effect.amount * lfo;
                    let line = if index == 1 { &mut self.chorus } else { &mut self.flanger };
                    let left = line.read(time);
                    let right = if index == 1 { line.read(base - depth * effect.amount * lfo) } else { left };
                    let out = [left[0], right[1]];
                    let feedback = if index == 2 { effect.feedback * 0.9 } else { 0.0 };
                    line.push([input[0] + out[0] * feedback, input[1] + out[1] * feedback]);
                    out
                }
                // Four swept all-pass stages per channel.
                3 => {
                    let lfo = self.lfo(index, 0.05 + effect.rate * 5.0);
                    let hz = 200.0 * (2.0_f32).powf(4.0 * (0.5 + 0.5 * lfo * effect.amount));
                    let tangent = (std::f32::consts::PI * hz / RATE).tan();
                    let coefficient = (1.0 - tangent) / (1.0 + tangent);
                    let mut out = [0.0; 2];
                    for channel in 0..2 {
                        let mut value = input[channel] + self.phaser_feedback[channel] * effect.feedback * 0.8;
                        for stage in 0..4 {
                            let next = -coefficient * value + self.phaser_x[channel][stage] + coefficient * self.phaser_y[channel][stage];
                            self.phaser_x[channel][stage] = value;
                            self.phaser_y[channel][stage] = next;
                            value = next;
                        }
                        self.phaser_feedback[channel] = value;
                        out[channel] = value;
                    }
                    out
                }
                // Bit reduction plus sample-and-hold rate reduction.
                4 => {
                    let interval = 1 + (effect.rate * 63.0).round() as usize;
                    if self.crush_counter == 0 {
                        let bits = 2.0 + (1.0 - effect.amount) * 14.0;
                        let levels = 2.0_f32.powf(bits - 1.0);
                        self.crush_hold = input.map(|sample| (sample * levels).round() / levels);
                    }
                    self.crush_counter += 1;
                    if self.crush_counter >= interval { self.crush_counter = 0; }
                    self.crush_hold
                }
                5 => {
                    let drive = 1.0 + effect.amount * 24.0;
                    input.map(|sample| (sample * drive).tanh() / drive.tanh())
                }
                6 => {
                    let lfo = self.lfo(index, 0.1 + effect.rate * 19.9);
                    let gain = 1.0 - effect.amount * (0.5 + 0.5 * lfo);
                    input.map(|sample| sample * gain)
                }
                7 => {
                    let lfo = self.lfo(index, 0.05 + effect.rate * 10.0);
                    let hz = 80.0 * (2.0_f32).powf(7.0 * (0.5 + 0.5 * lfo * effect.amount));
                    let coef = 1.0 - (-std::f32::consts::TAU * hz / RATE).exp();
                    for channel in 0..2 { self.filter[channel] += coef * (input[channel] - self.filter[channel]); }
                    self.filter
                }
                8 => {
                    let mid = (input[0] + input[1]) * 0.5;
                    let side = (input[0] - input[1]) * 0.5 * (effect.amount * 2.0);
                    [mid + side, mid - side]
                }
                9 => {
                    let carrier = self.lfo(index, 1.0 + effect.rate * 1999.0);
                    input.map(|sample| sample * carrier)
                }
                // Windowed FIR Hilbert transform produces an analytic signal; the
                // dry channel is delayed by the same 63 samples as the real part.
                10 => {
                    self.hilbert.push(input);
                    let real = self.hilbert.read(64.0);
                    let mut quadrature = [0.0; 2];
                    for tap in 0..HILBERT {
                        let coef = self.hilbert_coefficients[tap];
                        if coef == 0.0 { continue; }
                        let delayed = self.hilbert.read((tap + 1) as f32);
                        for channel in 0..2 { quadrature[channel] += delayed[channel] * coef; }
                    }
                    let hz = (effect.rate * 2.0 - 1.0) * 1000.0;
                    self.phase[index] = (self.phase[index] + hz / RATE).rem_euclid(1.0);
                    let (sin, cos) = (std::f32::consts::TAU * self.phase[index]).sin_cos();
                    let shifted = [real[0] * cos - quadrature[0] * sin, real[1] * cos - quadrature[1] * sin];
                    signal = real;
                    shifted
                }
                11 => {
                    let level = input[0].abs().max(input[1].abs());
                    let attack = 1.0 - (-1.0 / (0.005 * RATE)).exp();
                    let release = 1.0 - (-1.0 / ((0.02 + effect.rate * 0.98) * RATE)).exp();
                    self.gate_envelope += (level - self.gate_envelope) * if level > self.gate_envelope { attack } else { release };
                    let threshold = 10.0_f32.powf((-80.0 + effect.amount * 80.0) / 20.0);
                    let target = if self.gate_envelope >= threshold { 1.0 } else { 0.0 };
                    self.gate_gain += (target - self.gate_gain) * if target > self.gate_gain { attack } else { release };
                    input.map(|sample| sample * self.gate_gain)
                }
                _ => unreachable!(),
            };
            signal = [signal[0] * (1.0 - effect.mix) + wet[0] * effect.mix, signal[1] * (1.0 - effect.mix) + wet[1] * effect.mix];
        }
        signal
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delay_produces_repeats_and_its_render_tail_covers_them() {
        let mut effects = Effects::new();
        let mut config = [EffectConfig::default(); COUNT];
        config[0] = EffectConfig { enabled: true, amount: 0.0, rate: 0.0, feedback: 0.5, mix: 1.0 };
        let mut first = 0.0;
        let mut second = 0.0;
        for frame in 0..1_000 {
            let out = effects.process(if frame == 0 { [1.0, 0.0] } else { [0.0; 2] }, &config);
            if frame == 480 { first = out[0]; }
            if frame == 960 { second = out[0]; }
        }
        assert!((first - 1.0).abs() < 1e-5, "{first}");
        assert!((second - 0.45).abs() < 1e-5, "{second}");
        assert!(tail_frames(&config) >= 960);
    }

    #[test]
    fn chorus_widens_a_mono_signal() {
        let mut effects = Effects::new();
        let mut config = [EffectConfig::default(); COUNT];
        config[1] = EffectConfig { enabled: true, amount: 1.0, rate: 0.5, mix: 1.0, ..EffectConfig::default() };
        let mut difference = 0.0_f32;
        for frame in 0..12_000 {
            let sample = (frame as f32 * 0.1).sin();
            let out = effects.process([sample; 2], &config);
            difference = difference.max((out[0] - out[1]).abs());
        }
        assert!(difference > 0.1, "chorus stayed mono: {difference}");
    }

    #[test]
    fn bypassing_delay_discards_its_old_echoes() {
        let mut effects = Effects::new();
        let mut config = [EffectConfig::default(); COUNT];
        config[0] = EffectConfig { enabled: true, rate: 0.0, mix: 1.0, ..EffectConfig::default() };
        effects.process([1.0, 1.0], &config);
        config[0].enabled = false;
        effects.process([0.0; 2], &config);
        config[0].enabled = true;
        for _ in 0..500 { assert_eq!(effects.process([0.0; 2], &config), [0.0; 2]); }
    }

    #[test]
    fn frequency_shifter_moves_a_sine_to_one_sideband() {
        let mut effects = Effects::new();
        let mut config = [EffectConfig::default(); COUNT];
        config[10] = EffectConfig { enabled: true, rate: 0.75, mix: 1.0, ..EffectConfig::default() };
        let mut upper = [0.0_f64; 2];
        let mut lower = [0.0_f64; 2];
        for n in 0..48_000 {
            let sample = (std::f32::consts::TAU * 2000.0 * n as f32 / RATE).sin();
            let output = effects.process([sample; 2], &config)[0] as f64;
            if n < 1000 { continue; }
            for (hz, bin) in [(2500.0, &mut upper), (1500.0, &mut lower)] {
                let phase = std::f64::consts::TAU * hz * n as f64 / RATE as f64;
                bin[0] += output * phase.cos();
                bin[1] += output * phase.sin();
            }
        }
        let amplitude = |v: [f64; 2]| v[0].hypot(v[1]);
        assert!(amplitude(upper) > amplitude(lower) * 5.0, "upper {:?}, lower {:?}", upper, lower);
    }

    #[test]
    fn bypass_is_exact_and_each_effect_is_finite_and_changes_a_signal() {
        let mut effects = Effects::new();
        let config = [EffectConfig::default(); COUNT];
        assert_eq!(effects.process([0.2, -0.3], &config), [0.2, -0.3]);
        for index in 0..COUNT {
            let mut effects = Effects::new();
            let mut config = [EffectConfig::default(); COUNT];
            config[index].enabled = true;
            config[index].mix = 1.0;
            config[index].rate = 0.02;
            config[index].amount = 0.8;
            let mut changed = false;
            for frame in 0..12_000 {
                let value = 0.25 * (frame as f32 * 0.07).sin();
                let input = [value, value * 0.5];
                let output = effects.process(input, &config);
                assert!(output.iter().all(|x| x.is_finite() && x.abs() < 10.0), "effect {index}");
                changed |= (output[0] - input[0]).abs() > 0.001;
            }
            assert!(changed, "effect {index} did not process");
        }
    }
}
