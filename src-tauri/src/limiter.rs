//! Peak limiting for the monitored and playback signal path.
//!
//! This is a look-ahead peak limiter in the spirit of D16 Frontier: the peak of
//! the detector signal over a 1.5 ms window decides the gain that is applied to
//! the same audio after it has travelled through the matching delay, so a
//! transient is already reduced when it reaches the output. Release has three
//! speeds, and the detector can be the mid signal or either channel so that a
//! loud left channel ducks the whole image.
//!
//! It is not a loudness normaliser: nothing here measures LUFS, and there is no
//! programme-dependent auto-gain. It is not a true-peak limiter either, because
//! nothing is oversampled, so inter-sample peaks are estimated by the sample
//! peak. It does not model analogue clipping.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// 1.5 ms at the 48 kHz rate the whole application runs at.
pub const LOOKAHEAD: usize = 72;
/// Output ceiling: -0.3 dBFS, the usual safety margin below full scale.
const CEILING_DB: f32 = -0.3;
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
    Mid,
    Left,
    Right,
}

impl Detector {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "mid" => Self::Mid,
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
            release: ReleaseMode::Medium,
            detector: Detector::Mid,
            soft_clip: false,
            auto_makeup: true,
        }
    }
}

pub fn validate(config: &LimiterConfig) -> Result<(), String> {
    if !config.threshold_db.is_finite() || !(-24.0..=0.0).contains(&config.threshold_db) {
        return Err("Limiter threshold must be between -24 and 0 dBFS".into());
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
        let detector = match config.detector {
            Detector::Mid => (frame[0] + frame[1]) * 0.5,
            Detector::Left => frame[0],
            Detector::Right => frame[1],
        }
        .abs();
        self.window.push_back(detector);
        while self.window.len() > LOOKAHEAD {
            self.window.pop_front();
        }
        let peak = self
            .window
            .iter()
            .copied()
            .fold(0.0_f32, f32::max)
            .max(0.0);
        let target = if peak > threshold_linear {
            (threshold_linear / peak).clamp(0.0, 1.0)
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

        let makeup = if config.auto_makeup {
            (db_to_linear(CEILING_DB) / threshold_linear).max(1.0)
        } else {
            1.0
        };
        let ceiling = db_to_linear(CEILING_DB);
        let applied = self.gain * makeup;
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
            release: ReleaseMode::Medium,
            detector: Detector::Mid,
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
        assert!(held <= CEILING_DB + 0.01, "ceiling passed at {held} dBFS");
        assert!((held - CEILING_DB).abs() < 0.3, "make-up left {held} dBFS");
        assert!(limiter.reduction_db() < -3.0, "{}", limiter.reduction_db());
    }
}
