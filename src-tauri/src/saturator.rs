//! Saturation for the processor rack.
//!
//! A tanh curve run at four times the stream rate, so the harmonics it makes land
//! where they belong instead of folding back as aliasing hiss. The interpolator and
//! the decimator are one windowed-sinc low-pass split into four phases: the cut is
//! 0.11 cycles per sample at the 4x rate (0.44 of the stream rate, 0.88 of its
//! Nyquist) and a Blackman window keeps the stop band quiet. Drive is input gain
//! into the curve, normalised so a full-scale sample leaves at full scale: the knob
//! adds harmonics rather than level.
//!
//! Tone is a tilt on the saturated signal, a one-pole blend toward a 2.5 kHz copy
//! below the middle of its travel and a scaled first difference above it. A 10 Hz
//! DC blocker follows, because the even part of the curve lifts the mean, and Mix
//! blends the result against the dry signal delayed by the oversampler's own
//! latency, so half mix does not comb.
//!
//! It is not an amp sim and not tape: no wow, no head bump, no bias. Bypass and
//! `mix == 0.0` are bit-exact pass-through.

use serde::{Deserialize, Serialize};

const RATE: f32 = 48_000.0;
/// Four times up, so a 10 kHz partial still has room for its harmonics.
const OVERSAMPLE: usize = 4;
/// Coefficients per phase, 192 in all: a 96-tap prototype per phase is far more
/// than the cut needs, and the CPU cost is a few dozen multiplies per sample.
const PHASE_TAPS: usize = 48;
const TAPS: usize = OVERSAMPLE * PHASE_TAPS;
/// Cut in cycles per sample at the 4x rate: 0.88 of the stream's Nyquist.
const CUTOFF: f32 = 0.11;
/// The oversampler's latency, in stream samples, so Mix stays phase-aligned.
const DRY_DELAY: usize = 48;
/// One-pole corner the dark half of Tone blends toward.
const DARK_HZ: f32 = 2_500.0;
/// One-pole corner of the DC blocker.
const DC_HZ: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SaturatorConfig {
    pub enabled: bool,
    /// Input gain into the curve, in dB.
    pub drive_db: f32,
    /// 0.5 is neutral, below it darkens, above it brightens.
    pub tone: f32,
    /// Dry/wet, one is fully saturated.
    pub mix: f32,
}

impl Default for SaturatorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            drive_db: 6.0,
            tone: 0.5,
            mix: 1.0,
        }
    }
}

pub fn validate(config: &SaturatorConfig) -> Result<(), String> {
    if !config.drive_db.is_finite() || !(0.0..=24.0).contains(&config.drive_db) {
        return Err("Saturation drive must be between 0 and 24 dB".into());
    }
    if !config.tone.is_finite() || !(0.0..=1.0).contains(&config.tone) {
        return Err("Saturation tone must be between 0 and 1".into());
    }
    if !config.mix.is_finite() || !(0.0..=1.0).contains(&config.mix) {
        return Err("Saturation mix must be between 0 and 1".into());
    }
    Ok(())
}

/// One low-pass, split into the phases the interpolator reads and the single kernel
/// the decimator convolves with. The prototype sums to one so decimation keeps the
/// level; each phase of the interpolator sums to one so a constant goes through
/// without a ripple at the 4x rate.
fn design() -> ([[f32; PHASE_TAPS]; OVERSAMPLE], [f32; TAPS]) {
    let mut kernel = [0.0f32; TAPS];
    let centre = (TAPS as f32 - 1.0) / 2.0;
    let mut total = 0.0f32;
    for tap in 0..TAPS {
        let t = tap as f32 - centre;
        let angle = 2.0 * std::f32::consts::PI * CUTOFF * t;
        let sinc = if t.abs() < 1e-6 {
            1.0
        } else {
            angle.sin() / angle
        };
        let ratio = tap as f32 / (TAPS as f32 - 1.0);
        let window = 0.42 - 0.5 * (2.0 * std::f32::consts::PI * ratio).cos()
            + 0.08 * (4.0 * std::f32::consts::PI * ratio).cos();
        kernel[tap] = sinc * window;
        total += kernel[tap];
    }
    if total.abs() > 1e-9 {
        for value in &mut kernel {
            *value /= total;
        }
    }
    let mut up = [[0.0f32; PHASE_TAPS]; OVERSAMPLE];
    for phase in 0..OVERSAMPLE {
        let mut sum = 0.0f32;
        for k in 0..PHASE_TAPS {
            sum += kernel[k * OVERSAMPLE + phase];
        }
        for k in 0..PHASE_TAPS {
            let value = kernel[k * OVERSAMPLE + phase];
            up[phase][k] = if sum.abs() > 1e-9 { value / sum } else { 0.0 };
        }
    }
    (up, kernel)
}

pub struct Saturator {
    /// The interpolator's phases, per 4x sub-sample.
    up: [[f32; PHASE_TAPS]; OVERSAMPLE],
    /// The decimator's low-pass, unity gain at DC.
    kernel: [f32; TAPS],
    /// Stream-rate history, newest last, that the interpolator reads.
    history: [[f32; PHASE_TAPS]; 2],
    /// Oversampled history, newest last, that the decimator convolves.
    oversampled: [[f32; TAPS]; 2],
    /// The dry signal held back by the oversampler's latency for Mix.
    dry: [[f32; DRY_DELAY]; 2],
    /// Tone's low-pass state, below the middle of its travel.
    dark: [f32; 2],
    /// Tone's previous wet sample, above the middle of its travel.
    previous: [f32; 2],
    dc_in: [f32; 2],
    dc_out: [f32; 2],
    active: bool,
}

impl Saturator {
    pub fn new(_config: SaturatorConfig) -> Self {
        let (up, kernel) = design();
        Self {
            up,
            kernel,
            history: [[0.0; PHASE_TAPS]; 2],
            oversampled: [[0.0; TAPS]; 2],
            dry: [[0.0; DRY_DELAY]; 2],
            dark: [0.0; 2],
            previous: [0.0; 2],
            dc_in: [0.0; 2],
            dc_out: [0.0; 2],
            active: false,
        }
    }

    fn clear(&mut self) {
        self.history = [[0.0; PHASE_TAPS]; 2];
        self.oversampled = [[0.0; TAPS]; 2];
        self.dry = [[0.0; DRY_DELAY]; 2];
        self.dark = [0.0; 2];
        self.previous = [0.0; 2];
        self.dc_in = [0.0; 2];
        self.dc_out = [0.0; 2];
    }

    /// One stereo frame in, one saturated stereo frame out.
    pub fn process(&mut self, frame: [f32; 2], config: SaturatorConfig) -> [f32; 2] {
        // Bypass and a dry mix are the same branch: nothing runs, nothing is
        // remembered, and the frame comes back untouched.
        if !config.enabled || config.mix <= 0.0 {
            if self.active {
                self.clear();
                self.active = false;
            }
            return frame;
        }
        if !self.active {
            self.clear();
            self.active = true;
        }

        let dry = [
            if frame[0].is_finite() { frame[0] } else { 0.0 },
            if frame[1].is_finite() { frame[1] } else { 0.0 },
        ];
        let drive = 10f32.powf(config.drive_db.clamp(0.0, 24.0) / 20.0);
        // Full scale leaves at full scale: the knob changes the shape, not the level.
        let normaliser = drive.tanh().max(1e-6);
        let tone = config.tone.clamp(0.0, 1.0);
        let tilt = (tone - 0.5) * 2.0;
        let dark_coefficient = 1.0 - (-2.0 * std::f32::consts::PI * DARK_HZ / RATE).exp();
        let dc_coefficient = (-2.0 * std::f32::consts::PI * DC_HZ / RATE).exp();

        for channel in 0..2 {
            let history = &mut self.history[channel];
            history.copy_within(1.., 0);
            history[PHASE_TAPS - 1] = dry[channel];
            let held = &mut self.dry[channel];
            held.copy_within(1.., 0);
            held[DRY_DELAY - 1] = dry[channel];
        }

        // Up, saturate, down: four sub-samples per stream sample.
        for sub in 0..OVERSAMPLE {
            for channel in 0..2 {
                let mut sum = 0.0f32;
                for k in 0..PHASE_TAPS {
                    sum += self.up[sub][k] * self.history[channel][PHASE_TAPS - 1 - k];
                }
                let shaped = (drive * sum).tanh() / normaliser;
                let oversampled = &mut self.oversampled[channel];
                if sub == 0 {
                    oversampled.copy_within(OVERSAMPLE.., 0);
                }
                oversampled[TAPS - OVERSAMPLE + sub] = shaped;
            }
        }

        let mut wet = [0.0f32; 2];
        for channel in 0..2 {
            let mut sum = 0.0f32;
            for tap in 0..TAPS {
                sum += self.kernel[tap] * self.oversampled[channel][tap];
            }
            wet[channel] = sum;
        }

        let mut out = [0.0f32; 2];
        for channel in 0..2 {
            let shaped = wet[channel];
            // Tone: below the middle blends toward a dark copy, above it adds the
            // first difference. Exactly half is untouched, and says so by not running.
            let tilted = if tilt < 0.0 {
                self.dark[channel] += dark_coefficient * (shaped - self.dark[channel]);
                shaped + (-tilt) * (self.dark[channel] - shaped)
            } else if tilt > 0.0 {
                shaped + tilt * 0.5 * (shaped - self.previous[channel])
            } else {
                shaped
            };
            self.previous[channel] = shaped;

            // The curve lifts the mean, so the offset is removed here rather than
            // carried into the next stage.
            let blocked = tilted - self.dc_in[channel] + dc_coefficient * self.dc_out[channel];
            self.dc_in[channel] = tilted;
            self.dc_out[channel] = blocked;

            let mixed = self.dry[channel][0] * (1.0 - config.mix) + blocked * config.mix;
            // The interpolation ringing can reach a percent past full scale at the
            // extremes; the stage promises it never leaves ±1.0.
            out[channel] = if mixed.is_finite() {
                mixed.clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> SaturatorConfig {
        SaturatorConfig {
            enabled: true,
            drive_db: 6.0,
            tone: 0.5,
            mix: 1.0,
        }
    }

    fn sine(hz: f32, amplitude: f32, index: usize) -> f32 {
        (2.0 * std::f32::consts::PI * hz * index as f32 / RATE).sin() * amplitude
    }

    /// Amplitude of one frequency in a settled window, by projection.
    fn amplitude(frames: &[[f32; 2]], start: usize, length: usize, hz: f32) -> f32 {
        let omega = 2.0 * std::f64::consts::PI * hz as f64 / RATE as f64;
        let (mut cosine, mut sinus) = (0.0f64, 0.0f64);
        for index in 0..length {
            let sample = frames[start + index][0] as f64;
            let phase = omega * (start + index) as f64;
            cosine += sample * phase.cos();
            sinus += sample * phase.sin();
        }
        2.0 * (cosine * cosine + sinus * sinus).sqrt() as f32 / length as f32
    }

    fn harmonic_ratio(drive_db: f32) -> f32 {
        let config = SaturatorConfig {
            drive_db,
            ..base()
        };
        let mut saturator = Saturator::new(config);
        let frames: Vec<[f32; 2]> = (0..24_000)
            .map(|index| {
                let value = sine(1_000.0, 0.3, index);
                saturator.process([value, value], config)
            })
            .collect();
        let fundamental = amplitude(&frames, 8_000, 12_000, 1_000.0);
        let third = amplitude(&frames, 8_000, 12_000, 3_000.0);
        20.0 * (third / fundamental.max(1e-9)).log10()
    }

    #[test]
    fn bypass_is_bit_exact() {
        let config = SaturatorConfig {
            enabled: false,
            ..base()
        };
        let mut saturator = Saturator::new(config);
        for index in 0..2_000 {
            let input = [sine(440.0, 1.4, index), sine(660.0, 0.9, index)];
            assert_eq!(saturator.process(input, config), input, "frame {index}");
        }
    }

    #[test]
    fn a_zero_mix_is_bit_exact() {
        let config = SaturatorConfig {
            mix: 0.0,
            ..base()
        };
        let mut saturator = Saturator::new(config);
        for index in 0..2_000 {
            let input = [sine(440.0, 1.2, index), sine(120.0, 0.7, index)];
            assert_eq!(saturator.process(input, config), input, "frame {index}");
        }
    }

    #[test]
    fn a_hot_full_scale_sine_stays_inside_full_scale() {
        let config = SaturatorConfig {
            drive_db: 12.0,
            ..base()
        };
        let mut saturator = Saturator::new(config);
        let mut loudest = 0.0f32;
        for index in 0..20_000 {
            let value = sine(1_000.0, 1.0, index);
            let out = saturator.process([value, value], config);
            for channel in out {
                assert!(channel.is_finite());
                loudest = loudest.max(channel.abs());
            }
        }
        assert!(loudest <= 1.0, "left full scale at {loudest}");
        assert!(loudest > 0.8, "the stage muted the signal: {loudest}");
    }

    #[test]
    fn drive_raises_the_third_harmonic() {
        let gentle = harmonic_ratio(3.0);
        let hot = harmonic_ratio(18.0);
        assert!(
            hot >= gentle + 10.0,
            "third harmonic {hot:.1} dB at 18 dB drive against {gentle:.1} dB at 3 dB"
        );
    }

    #[test]
    fn the_dc_blocker_leaves_no_standing_offset() {
        let config = base();
        let mut saturator = Saturator::new(config);
        for _ in 0..4_800 {
            saturator.process([0.8, 0.8], config);
        }
        let mut tail = Vec::new();
        for _ in 0..24_000 {
            tail.push(saturator.process([0.0, 0.0], config));
        }
        let settled = &tail[tail.len() - 4_800..];
        let mean: f32 = settled.iter().map(|frame| frame[0]).sum::<f32>() / settled.len() as f32;
        assert!(mean.abs() < 1e-3, "the stage kept a {mean} offset");
    }

    #[test]
    fn impossible_settings_are_rejected() {
        assert!(validate(&SaturatorConfig { drive_db: -1.0, ..base() }).is_err());
        assert!(validate(&SaturatorConfig { tone: 1.5, ..base() }).is_err());
        assert!(validate(&SaturatorConfig { mix: 2.0, ..base() }).is_err());
        assert!(validate(&SaturatorConfig { drive_db: f32::NAN, ..base() }).is_err());
        assert!(validate(&base()).is_ok());
        assert!(validate(&SaturatorConfig::default()).is_ok());
    }
}
