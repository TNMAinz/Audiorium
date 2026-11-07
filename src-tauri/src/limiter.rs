//! Peak limiting for the monitored and playback signal path.
//!
//! This is a look-ahead peak limiter in the spirit of D16 Frontier: the peak of
//! the detector signal over a 1.5 ms window decides the gain that is applied to
//! the same audio after it has travelled through the matching delay, so a
//! transient is already reduced when it reaches the output. Release has three
//! speeds, and the detector can be the loudest channel, which keeps both channels
//! under the ceiling as one image, or either channel alone.
//!
//! The gain works from the threshold; Output is a constant trim after it, so a
//! lower output level is quieter, not more limited.
//!
//! It is a true-peak limiter: the detector judges a frame by the peak of a four
//! times oversampled interpolation of the recent samples, using the same
//! windowed-sinc branches the loudness meter measures true peak with, so the
//! ceiling the stage holds is a dBTP figure and not just a sample peak. That
//! matters because a sample peak limiter can leave an inter-sample crest above the
//! ceiling, which the encoder or the listener's D/A then clips flat.
//!
//! It is not a loudness normaliser: nothing here measures LUFS, and there is no
//! programme-dependent auto-gain. It does not model analogue clipping.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

use crate::loudness::{interpolation, PHASES, TAPS};

/// 1.5 ms at the 48 kHz rate the whole application runs at.
pub const LOOKAHEAD: usize = 72;
/// The ceiling a new limiter starts on: -0.3 dBFS, the usual safety margin below
/// full scale. The control can move it, and the stage holds whatever it is set to.
pub const DEFAULT_CEILING_DB: f32 = -0.3;
/// The most the applied linear gain may move in one sample, so a change never clicks.
const GAIN_STEP: f32 = 0.02;
const METER_DECAY: f32 = 0.999;
const RATE: f32 = 48_000.0;
const FLOOR_DB: f32 = -120.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseMode {
    Fast,
    Medium,
    Slow,
}

impl ReleaseMode {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "fast" => Self::Fast,
            "medium" => Self::Medium,
            "slow" => Self::Slow,
            _ => return None,
        })
    }


    fn tau(self) -> f32 {
        match self {
            Self::Fast => 0.08,
            Self::Medium => 0.25,
            Self::Slow => 0.8,
        }
    }

    fn step(self) -> f32 {
        1.0 - (-1.0 / (RATE * self.tau())).exp()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Detector {
    /// The loudest channel. A detector on the mid sum reads a panned signal several
    /// dB low, so the gain it asks for lets the louder channel overshoot the ceiling
    /// and be clipped flat: that is the grit heard on a stereo mix.
    #[serde(alias = "mid")]
    Both,
    Left,
    Right,
}

impl Detector {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "both" | "mid" => Self::Both,
            "left" => Self::Left,
            "right" => Self::Right,
            _ => return None,
        })
    }

}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LimiterConfig {
    pub enabled: bool,
    /// Where limiting begins, in dBFS.
    pub threshold_db: f32,
    /// The level the stage will not let anything leave above, in dBFS. With make-up
    /// on, this is also where the limited signal leaves: the Output control is a trim
    /// after the gain, never a second, lower threshold for the gain to chase.
    pub ceiling_db: f32,
    pub release: ReleaseMode,
    /// Which signal decides how far to reduce.
    pub detector: Detector,
    /// Smooth clipping above the ceiling for overs that slip past the look-ahead.
    pub soft_clip: bool,
    /// Bring the limited peak back up to the ceiling instead of leaving it at the threshold.
    pub auto_makeup: bool,
}

impl Default for LimiterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold_db: -3.0,
            ceiling_db: DEFAULT_CEILING_DB,
            release: ReleaseMode::Medium,
            detector: Detector::Both,
            soft_clip: false,
            auto_makeup: true,
        }
    }
}

pub fn validate(config: &LimiterConfig) -> Result<(), String> {
    if !config.threshold_db.is_finite() || !(-24.0..=0.0).contains(&config.threshold_db) {
        return Err("Limiter threshold must be between -24 and 0 dBFS".into());
    }
    if !config.ceiling_db.is_finite() || !(-24.0..=0.0).contains(&config.ceiling_db) {
        return Err("Limiter ceiling must be between -24 and 0 dBFS".into());
    }
    Ok(())
}

fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

fn linear_to_db(value: f32) -> f32 {
    if value <= 1e-9 {
        FLOOR_DB
    } else {
        (20.0 * value.log10()).max(FLOOR_DB)
    }
}

pub struct Limiter {
    config: LimiterConfig,
    /// The audio that will meet its own detector result, delayed by the look-ahead.
    delay: VecDeque<[f32; 2]>,
    /// Detector magnitudes of the last look-ahead window.
    window: VecDeque<f32>,
    /// Applied linear gain before make-up, always at most 1.
    gain: f32,
    reduction: f32,
    peak: f32,
    /// The four-times oversampling the detector judges a frame with, and the
    /// samples it needs around that frame. Same branches as `loudness.rs`, so the
    /// limiter and the meter agree about what a true peak is.
    phases: [[f32; TAPS]; PHASES],
    history: [[f32; TAPS]; 2],
    head: usize,
}

impl Limiter {
    pub fn new(config: LimiterConfig) -> Self {
        Self {
            config,
            delay: VecDeque::with_capacity(LOOKAHEAD + 1),
            window: VecDeque::with_capacity(LOOKAHEAD + 1),
            gain: 1.0,
            reduction: 0.0,
            peak: 0.0,
            phases: interpolation(),
            history: [[0.0; TAPS]; 2],
            head: 0,
        }
    }

    /// One stereo frame in, one limited stereo frame out.
    pub fn process(&mut self, frame: [f32; 2], config: LimiterConfig) -> [f32; 2] {
        if !config.enabled {
            self.config = config;
            self.gain = 1.0;
            self.reduction = 0.0;
            self.delay.clear();
            self.window.clear();
            self.history = [[0.0; TAPS]; 2];
            self.head = 0;
            let level = frame[0].abs().max(frame[1].abs());
            self.peak = if level > self.peak { level } else { self.peak * METER_DECAY };
            // Bypass is exact: the same samples come back out.
            return frame;
        }
        if config.detector != self.config.detector {
            // Stale detector values would duck the wrong signal for one window.
            self.window.clear();
        }
        self.config = config;

        let threshold = if config.threshold_db.is_finite() {
            config.threshold_db.clamp(-24.0, 0.0)
        } else {
            0.0
        };
        let threshold_linear = db_to_linear(threshold);
        // The detector reads the oversampled peak, not the sample peak: an
        // inter-sample crest between two quiet samples is exactly what a sample
        // peak limiter lets through and the encoder then clips.
        let left = self.oversampled(0, frame[0]);
        let right = self.oversampled(1, frame[1]);
        self.history[0][self.head] = frame[0];
        self.history[1][self.head] = frame[1];
        self.head = (self.head + 1) % TAPS;
        let detector = match config.detector {
            Detector::Both => left.max(right),
            Detector::Left => left,
            Detector::Right => right,
        };
        self.window.push_back(detector);
        // One sample more than the delay, so the sample this head is about to emit is
        // in the window that decides its gain. Without it the loudest sample of a
        // transient is judged by its neighbours alone and leaves above the ceiling.
        while self.window.len() > LOOKAHEAD + 1 {
            self.window.pop_front();
        }
        let peak = self
            .window
            .iter()
            .copied()
            .fold(0.0_f32, f32::max)
            .max(0.0);
        let ceiling = db_to_linear(if config.ceiling_db.is_finite() {
            config.ceiling_db.clamp(-24.0, 0.0)
        } else {
            DEFAULT_CEILING_DB
        });
        // The gain is decided by the threshold alone. Output is a trim applied after
        // it, so pulling the output down under the programme level is one constant
        // multiply: the waveform keeps its shape instead of being squashed peak by
        // peak, which is what made a quiet output sound grainy.
        let limit_to = threshold_linear;
        let target = if peak > limit_to {
            (limit_to / peak).clamp(0.0, 1.0)
        } else {
            1.0
        };
        if target < self.gain {
            // Spread the attack over the look-ahead window: the transient that set
            // `target` is still in the delay line, so the gain has landed by the
            // time that sample is emitted, and no output sample takes a jump.
            let step = (((self.gain - target) / LOOKAHEAD as f32) * 1.6).max(GAIN_STEP);
            self.gain = (self.gain - step).max(target);
        } else {
            let step = config.release.step();
            self.gain += step * (target - self.gain);
            if self.gain > 1.0 {
                self.gain = 1.0;
            }
        }
        // The release is a one-pole, so it approaches unity without reaching it.
        // Within 0.1 dB of unity is "not limiting" for a meter, not a number.
        self.reduction = if self.gain >= 0.99 {
            0.0
        } else {
            linear_to_db(self.gain)
        };

        self.delay.push_back(frame);
        let delayed = if self.delay.len() > LOOKAHEAD {
            self.delay.pop_front().unwrap_or([0.0; 2])
        } else {
            [0.0; 2]
        };

        // Make-up brings the limited peak to the Output level. Without it the stage
        // never adds gain: the knob can only pull the level down, and the ceiling is
        // then a safety line above the signal.
        let requested = ceiling / limit_to;
        let trim = if config.auto_makeup {
            requested
        } else {
            requested.min(1.0)
        };
        let applied = self.gain * trim;
        let mut out = [0.0f32; 2];
        for channel in 0..2 {
            let value = delayed[channel] * applied;
            let value = if !value.is_finite() {
                0.0
            } else if config.soft_clip && value.abs() > ceiling {
                let magnitude = ceiling + (1.0 - ceiling) * ((value.abs() - ceiling) / (1.0 - ceiling)).tanh();
                value.signum() * magnitude.min(1.0)
            } else {
                // A hard ceiling is the whole point of a peak limiter; it is the
                // last line of defence when a ramp has not finished. Soft clip
                // deliberately allows the knee above it instead.
                value.clamp(-ceiling, ceiling)
            };
            out[channel] = value.clamp(-1.0, 1.0);
        }
        let level = out[0].abs().max(out[1].abs());
        self.peak = if level > self.peak { level } else { self.peak * METER_DECAY };
        out
    }

    /// The peak of the four-times oversampled signal ending at this sample.
    fn oversampled(&self, channel: usize, sample: f32) -> f32 {
        let mut best = sample.abs();
        for phase in &self.phases {
            let mut sum = 0.0f32;
            for (tap, coefficient) in phase.iter().enumerate() {
                let age = (self.head + TAPS - tap) % TAPS;
                let value = if tap == 0 { sample } else { self.history[channel][age] };
                sum += coefficient * value;
            }
            if sum.is_finite() {
                best = best.max(sum.abs());
            }
        }
        best
    }

    /// Gain reduction currently applied, in dB, zero or negative.
    pub fn reduction_db(&self) -> f32 {
        self.reduction
    }

    /// Peak of the recent output, in dBFS, floored at -120.
    pub fn peak_db(&self) -> f32 {
        linear_to_db(self.peak)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> LimiterConfig {
        LimiterConfig {
            enabled: true,
            threshold_db: -6.0,
            ceiling_db: DEFAULT_CEILING_DB,
            release: ReleaseMode::Medium,
            detector: Detector::Both,
            soft_clip: false,
            auto_makeup: false,
        }
    }

    fn sine(hz: f32, amplitude: f32, index: usize) -> [f32; 2] {
        let value = (2.0 * std::f32::consts::PI * hz * index as f32 / RATE).sin() * amplitude;
        [value, value]
    }

    fn peak_db(frames: &[[f32; 2]]) -> f32 {
        let peak = frames
            .iter()
            .map(|frame| frame[0].abs().max(frame[1].abs()))
            .fold(0.0_f32, f32::max);
        linear_to_db(peak)
    }

    #[test]
    fn disabled_returns_the_input_untouched() {
        let config = LimiterConfig {
            enabled: false,
            ..base()
        };
        let mut limiter = Limiter::new(config);
        for index in 0..1_000 {
            let input = sine(1_000.0, 1.0, index);
            assert_eq!(limiter.process(input, config), input);
        }
        assert_eq!(limiter.reduction_db(), 0.0);
    }

    #[test]
    fn a_loud_sine_is_held_at_the_threshold() {
        let config = base();
        let mut limiter = Limiter::new(config);
        let out: Vec<[f32; 2]> = (0..20_000)
            .map(|index| limiter.process(sine(1_000.0, 1.0, index), config))
            .collect();
        let held = peak_db(&out[out.len() - 2_000..]);
        assert!(held <= -5.9, "held at {held} dBFS");
        assert!((held + 6.0).abs() < 0.4, "held at {held} dBFS");
    }

    #[test]
    fn reduction_is_reported_for_a_loud_signal_and_none_for_a_quiet_one() {
        let config = base();
        let mut limiter = Limiter::new(config);
        for index in 0..2_000 {
            limiter.process(sine(1_000.0, 1.0, index), config);
        }
        assert!(limiter.reduction_db() < -3.0, "{}", limiter.reduction_db());

        let mut quiet = Limiter::new(config);
        for index in 0..2_000 {
            quiet.process(sine(1_000.0, 0.1, index), config);
        }
        assert_eq!(quiet.reduction_db(), 0.0);
        assert!(quiet.peak_db() < -12.0, "{}", quiet.peak_db());
    }

    #[test]
    fn fast_release_recovers_quickly_and_slow_release_does_not() {
        let mut fast = base();
        fast.release = ReleaseMode::Fast;
        let mut limiter = Limiter::new(fast);
        for index in 0..4_800 {
            limiter.process(sine(1_000.0, 1.0, index), fast);
        }
        assert!(limiter.reduction_db() < -3.0, "{}", limiter.reduction_db());
        for _ in 0..72_000 {
            limiter.process([0.0; 2], fast);
        }
        let recovered = limiter.reduction_db();
        assert!(recovered > -1.0, "fast release left {recovered} dB");

        let mut slow = base();
        slow.release = ReleaseMode::Slow;
        let mut limiter = Limiter::new(slow);
        for index in 0..4_800 {
            limiter.process(sine(1_000.0, 1.0, index), slow);
        }
        for _ in 0..72_000 {
            limiter.process([0.0; 2], slow);
        }
        let remaining = limiter.reduction_db();
        assert!(remaining < -0.2, "slow release only left {remaining} dB");
        assert!(remaining > -3.0, "slow release collapsed: {remaining} dB");
    }

    #[test]
    fn the_detector_can_be_one_channel() {
        let left_only = |index: usize| {
            let value = sine(1_000.0, 0.9, index)[0];
            [value, 0.0]
        };
        let mut left = base();
        left.detector = Detector::Left;
        let mut limiter = Limiter::new(left);
        for index in 0..2_000 {
            limiter.process(left_only(index), left);
        }
        assert!(limiter.reduction_db() < -1.0, "{}", limiter.reduction_db());

        let mut right = base();
        right.detector = Detector::Right;
        let mut limiter = Limiter::new(right);
        for index in 0..2_000 {
            limiter.process(left_only(index), right);
        }
        assert_eq!(limiter.reduction_db(), 0.0);
    }

    #[test]
    fn soft_clip_contains_an_over_hot_signal() {
        let config = LimiterConfig {
            threshold_db: -3.0,
            soft_clip: true,
            auto_makeup: true,
            ..base()
        };
        let mut limiter = Limiter::new(config);
        let mut loudest = 0.0_f32;
        for index in 0..8_000 {
            let out = limiter.process(sine(1_000.0, 1.6, index), config);
            for channel in out {
                assert!(channel.is_finite());
                assert!((-1.0..=1.0).contains(&channel), "{channel}");
                loudest = loudest.max(channel.abs());
            }
        }
        assert!(loudest > 0.9, "the signal was muted instead of clipped: {loudest}");
        assert!(limiter.reduction_db() < -1.0, "{}", limiter.reduction_db());
    }

    #[test]
    fn changing_the_threshold_does_not_click() {
        let config = base();
        let mut limiter = Limiter::new(config);
        let mut previous = 0.0_f32;
        let mut worst = 0.0_f32;
        for index in 0..8_000 {
            let current = if index < 4_000 {
                config
            } else {
                LimiterConfig {
                    threshold_db: -12.0,
                    ..config
                }
            };
            let out = limiter.process(sine(1_000.0, 1.0, index), current);
            // The first frames are the empty look-ahead delay, so skip its onset.
            if index > LOOKAHEAD + 2 {
                worst = worst.max((out[0] - previous).abs());
            }
            previous = out[0];
        }
        assert!(worst < 0.2, "largest step was {worst}");
    }

    #[test]
    fn silence_stays_silent_and_everything_stays_finite() {
        let config = LimiterConfig {
            soft_clip: true,
            ..base()
        };
        let mut limiter = Limiter::new(config);
        for _ in 0..48_000 {
            let out = limiter.process([0.0; 2], config);
            assert_eq!(out, [0.0; 2]);
        }
        assert_eq!(limiter.peak_db(), FLOOR_DB);

        let mut seed = 0x1234_5678u32;
        for _ in 0..48_000 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let value = (seed as f32 / u32::MAX as f32) * 4.0 - 2.0;
            let out = limiter.process([value, -value], config);
            assert!(out.iter().all(|sample| sample.is_finite()));
            assert!(out.iter().all(|sample| (-1.0..=1.0).contains(sample)));
        }
    }

    #[test]
    fn auto_makeup_lifts_the_limited_signal_to_the_ceiling() {
        let config = LimiterConfig {
            auto_makeup: true,
            ..base()
        };
        let mut limiter = Limiter::new(config);
        let out: Vec<[f32; 2]> = (0..20_000)
            .map(|index| limiter.process(sine(1_000.0, 1.0, index), config))
            .collect();
        let held = peak_db(&out[out.len() - 4_000..]);
        assert!(held <= DEFAULT_CEILING_DB + 0.01, "ceiling passed at {held} dBFS");
        assert!((held - DEFAULT_CEILING_DB).abs() < 0.3, "make-up left {held} dBFS");
        assert!(limiter.reduction_db() < -3.0, "{}", limiter.reduction_db());
    }

    #[test]
    fn the_ceiling_is_the_output_level_the_stage_holds() {
        // The same loud sine, held at two different ceilings: the output follows the
        // knob instead of a constant, which is what the panel's Output control is.
        let held_at = |ceiling_db: f32| {
            let config = LimiterConfig {
                auto_makeup: true,
                ceiling_db,
                ..base()
            };
            let mut limiter = Limiter::new(config);
            let out: Vec<[f32; 2]> = (0..20_000)
                .map(|index| limiter.process(sine(1_000.0, 1.0, index), config))
                .collect();
            peak_db(&out[out.len() - 4_000..])
        };
        let quiet = held_at(-12.0);
        let loud = held_at(-1.0);
        assert!((quiet - -12.0).abs() < 0.3, "{quiet} dBFS at a -12 dBFS ceiling");
        assert!((loud - -1.0).abs() < 0.3, "{loud} dBFS at a -1 dBFS ceiling");
        assert!(loud > quiet + 8.0, "the ceiling did not move the output");
    }

    #[test]
    fn turning_the_output_down_is_a_trim_not_more_limiting() {
        // The complaint this guards against: turning Output down under the programme
        // level used to make the gain chase a lower ceiling, and the squashing it did
        // sounded grainy. Output is one constant multiply after the gain, so the
        // limiter works exactly as hard at -12 dBFS as it does at -3, and only the
        // level changes.
        let held_at = |ceiling_db: f32| {
            let config = LimiterConfig {
                threshold_db: -3.0,
                ceiling_db,
                auto_makeup: true,
                ..base()
            };
            let mut limiter = Limiter::new(config);
            let out: Vec<[f32; 2]> = (0..20_000)
                .map(|index| limiter.process(sine(1_000.0, 1.0, index), config))
                .collect();
            (peak_db(&out[out.len() - 4_000..]), limiter.reduction_db())
        };
        let (loud, loud_reduction) = held_at(-3.0);
        let (quiet, quiet_reduction) = held_at(-12.0);
        assert!((quiet - -12.0).abs() < 0.3, "held at {quiet} dBFS");
        assert!((loud - -3.0).abs() < 0.3, "held at {loud} dBFS");
        assert!(
            (loud_reduction - quiet_reduction).abs() < 0.1,
            "the output changed the gain: {loud_reduction} dB against {quiet_reduction} dB"
        );
        assert!(quiet_reduction < -2.0, "the stage did not limit: {quiet_reduction} dB");
    }

    #[test]
    fn the_louder_channel_decides_so_neither_is_clipped_flat() {
        // A detector on the mid sum reads a panned signal low, so the gain it asks for
        // lets the louder channel leave above the ceiling and be clipped flat. The
        // link detector holds both channels, which is what removes the grit.
        let config = LimiterConfig {
            threshold_db: -3.0,
            ceiling_db: -3.0,
            auto_makeup: true,
            ..base()
        };
        let ceiling = db_to_linear(-3.0);
        let mut limiter = Limiter::new(config);
        let mut loudest = [0.0_f32; 2];
        for index in 0..20_000 {
            let left = sine(440.0, 1.0, index)[0];
            let out = limiter.process([left, left * 0.4], config);
            for channel in 0..2 {
                loudest[channel] = loudest[channel].max(out[channel].abs());
            }
        }
        for (channel, peak) in loudest.iter().enumerate() {
            assert!(*peak <= ceiling + 1e-4, "channel {channel} left {peak} above the ceiling");
        }
        assert!(limiter.reduction_db() < -2.0, "{}", limiter.reduction_db());
    }

    #[test]
    fn an_inter_sample_crest_is_held_under_the_ceiling() {
        // A quarter of the sample rate at 45 degrees of phase: every sample sits at
        // 0.707 of the tone's real peak, so the sample peak reads -3 dBFS while the
        // waveform between the samples reaches full scale. A sample-peak limiter
        // hears nothing to do here and lets the encoder clip the crest; a true-peak
        // detector has to pull about 3 dB out of it.
        let config = LimiterConfig {
            threshold_db: -3.0,
            ceiling_db: -3.0,
            auto_makeup: true,
            ..base()
        };
        let ceiling = db_to_linear(-3.0);
        let mut limiter = Limiter::new(config);
        let mut output: Vec<[f32; 2]> = Vec::with_capacity(20_000);
        for index in 0..20_000 {
            let angle = std::f32::consts::FRAC_PI_4
                + 2.0 * std::f32::consts::PI * 12_000.0 * index as f32 / RATE;
            let input = [angle.sin(), angle.sin()];
            output.push(limiter.process(input, config));
        }

        // The peak of the four-times oversampled tail, computed here rather than by
        // the meter: a meter started mid-signal has a cold filter for its first
        // samples, and that onset would be read as a peak the limiter never made.
        let phases = interpolation();
        let tail = output.len() - 4_800;
        let mut true_peak = 0.0f32;
        let mut sample_peak = 0.0f32;
        for index in tail..output.len() {
            sample_peak = sample_peak.max(output[index][0].abs());
            for phase in &phases {
                let mut sum = 0.0f32;
                for tap in 0..TAPS {
                    sum += phase[tap] * output[index - tap][0];
                }
                true_peak = true_peak.max(sum.abs());
            }
        }
        let true_peak_db = linear_to_db(true_peak);
        assert!(
            true_peak_db <= -3.0 + 0.1,
            "the master left {true_peak_db:.2} dBTP above a -3.0 dBTP ceiling"
        );
        assert!(
            sample_peak < true_peak * 0.8,
            "the output carries no inter-sample crest to judge: sample {sample_peak}, true {true_peak}"
        );
        // Holding the crest costs the samples: they land near -6 dBFS, not at the
        // -3 dBFS ceiling a sample-peak detector would have aimed for.
        assert!(sample_peak <= ceiling, "the output left {sample_peak} above the ceiling");
        assert!(sample_peak > 0.4, "the tone was muted rather than limited: {sample_peak}");
        assert!(limiter.reduction_db() < -2.5, "{}", limiter.reduction_db());
    }

    #[test]
    fn a_ceiling_outside_the_scale_is_rejected_rather_than_silently_moved() {
        let config = LimiterConfig {
            ceiling_db: -30.0,
            ..base()
        };
        assert!(validate(&config).is_err());
    }
}
