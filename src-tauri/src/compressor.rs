//! Feed-forward compression for the processing chain.
//!
//! The detector is a peak follower on the louder channel, so one loud channel
//! ducks the whole image and a panned signal is heard as one source. The static
//! curve is the usual soft-knee ratio; the gain that follows it is smoothed with
//! the attack and release, and its own slope is bounded so a changed setting can
//! never put a step in the waveform.
//!
//! It is not a limiter: nothing is delayed, so a fast attack shaves the first
//! sample of a transient instead of catching it in advance. That is why the
//! look-ahead limiter comes last in the chain.

use serde::{Deserialize, Serialize};

const RATE: f32 = 48_000.0;
const FLOOR_DB: f32 = -120.0;
/// Time to fall to 1/e of a held peak, in seconds. Long enough that a bass note
/// reads its amplitude instead of dipping at every zero crossing, short enough
/// that the level still follows a phrase.
const DETECT_RELEASE: f32 = 0.05;
/// The gain is a one-pole, and it is never allowed to move faster than this per
/// sample (12 000 dB/s). A normal attack is slower than this bound; it exists so
/// a setting changed under a loud signal cannot jump the waveform.
const MAX_STEP_DB: f32 = 0.25;
/// A one-pole reaches 90 % of a step after 2.303 time constants, so the controls
/// are divided by this to mean "time to 90 %".
const NINETY_PERCENT: f32 = 2.302_585;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CompressorConfig {
    pub enabled: bool,
    /// Level where the ratio starts to apply, in dBFS.
    pub threshold_db: f32,
    /// How far above the threshold the output falls, as a ratio to the input.
    pub ratio: f32,
    /// Time to reach 90 % of a new reduction, in milliseconds.
    pub attack_ms: f32,
    /// Time to give 90 % of it back, in milliseconds.
    pub release_ms: f32,
    /// Width of the over-easy knee around the threshold, in dB.
    pub knee_db: f32,
    /// Constant gain after the reduction, in dB.
    pub makeup_db: f32,
}

impl Default for CompressorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold_db: -20.0,
            ratio: 3.0,
            attack_ms: 10.0,
            release_ms: 200.0,
            knee_db: 6.0,
            makeup_db: 0.0,
        }
    }
}

pub fn validate(config: &CompressorConfig) -> Result<(), String> {
    if !config.threshold_db.is_finite() || !(-60.0..=0.0).contains(&config.threshold_db) {
        return Err("Compressor threshold must be between -60 and 0 dBFS".into());
    }
    if !config.ratio.is_finite() || !(1.0..=20.0).contains(&config.ratio) {
        return Err("Compressor ratio must be between 1:1 and 20:1".into());
    }
    if !config.attack_ms.is_finite() || !(0.5..=200.0).contains(&config.attack_ms) {
        return Err("Compressor attack must be between 0.5 and 200 ms".into());
    }
    if !config.release_ms.is_finite() || !(10.0..=2000.0).contains(&config.release_ms) {
        return Err("Compressor release must be between 10 and 2000 ms".into());
    }
    if !config.knee_db.is_finite() || !(0.0..=24.0).contains(&config.knee_db) {
        return Err("Compressor knee must be between 0 and 24 dB".into());
    }
    if !config.makeup_db.is_finite() || !(0.0..=24.0).contains(&config.makeup_db) {
        return Err("Compressor make-up must be between 0 and 24 dB".into());
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
        20.0 * value.log10()
    }
}

/// A value the stage can run even if the interface sent something impossible.
fn sanitized(config: &CompressorConfig) -> CompressorConfig {
    let defaults = CompressorConfig::default();
    let pick = |value: f32, fallback: f32| if value.is_finite() { value } else { fallback };
    CompressorConfig {
        enabled: config.enabled,
        threshold_db: pick(config.threshold_db, defaults.threshold_db).clamp(-60.0, 0.0),
        ratio: pick(config.ratio, defaults.ratio).clamp(1.0, 20.0),
        attack_ms: pick(config.attack_ms, defaults.attack_ms).clamp(0.5, 200.0),
        release_ms: pick(config.release_ms, defaults.release_ms).clamp(10.0, 2000.0),
        knee_db: pick(config.knee_db, defaults.knee_db).clamp(0.0, 24.0),
        makeup_db: pick(config.makeup_db, defaults.makeup_db).clamp(0.0, 24.0),
    }
}

/// How much the stage attenuates a steady level, in dB, as a positive number.
fn curve_db(level_db: f32, config: &CompressorConfig) -> f32 {
    if !level_db.is_finite() {
        return 0.0;
    }
    let slope = 1.0 - 1.0 / config.ratio;
    let over = level_db - config.threshold_db;
    let half = config.knee_db * 0.5;
    if over <= -half {
        0.0
    } else if over < half {
        // Over-easy knee: no reduction at the bottom of the knee, the full slope
        // at the top, and a curve with no corner in between.
        slope * (over + half).powi(2) / (2.0 * config.knee_db)
    } else {
        over * slope
    }
}

/// One-pole coefficient for a control expressed as the time to reach 90 %.
fn coefficient(time_ms: f32) -> f32 {
    let seconds = (time_ms / 1000.0).clamp(0.000_5, 2.0);
    (1.0 - (-NINETY_PERCENT / (RATE * seconds)).exp()).clamp(1e-6, 1.0)
}

pub struct Compressor {
    /// Peak follower on the louder channel, linear.
    level: f32,
    /// The attenuation being applied, in dB, zero or more.
    reduction: f32,
}

impl Compressor {
    pub fn new(_config: CompressorConfig) -> Self {
        Self {
            level: 0.0,
            reduction: 0.0,
        }
    }

    /// One stereo frame in, one compressed stereo frame out.
    pub fn process(&mut self, frame: [f32; 2], config: CompressorConfig) -> [f32; 2] {
        let mut frame = frame;
        for channel in frame.iter_mut() {
            if !channel.is_finite() {
                *channel = 0.0;
            }
        }
        if !config.enabled {
            // Bypass is exact, and the detector is cleared so switching the stage
            // back in starts from the signal rather than from a stale level.
            self.level = 0.0;
            self.reduction = 0.0;
            return frame;
        }
        let config = sanitized(&config);

        let loudest = frame[0].abs().max(frame[1].abs());
        let decay = (-1.0 / (RATE * DETECT_RELEASE)).exp();
        self.level = if loudest > self.level {
            loudest
        } else {
            self.level * decay
        };

        let target = curve_db(linear_to_db(self.level), &config);
        let time = if target > self.reduction {
            config.attack_ms
        } else {
            config.release_ms
        };
        let move_db = (target - self.reduction) * coefficient(time);
        self.reduction = (self.reduction + move_db.clamp(-MAX_STEP_DB, MAX_STEP_DB)).max(0.0);

        let gain = db_to_linear(-self.reduction) * db_to_linear(config.makeup_db);
        [
            (frame[0] * gain).clamp(-1.0, 1.0),
            (frame[1] * gain).clamp(-1.0, 1.0),
        ]
    }

    /// Gain reduction currently applied, in dB, zero or negative.
    pub fn reduction_db(&self) -> f32 {
        -self.reduction
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> CompressorConfig {
        CompressorConfig {
            enabled: true,
            threshold_db: -20.0,
            ratio: 4.0,
            attack_ms: 10.0,
            release_ms: 200.0,
            knee_db: 0.0,
            makeup_db: 0.0,
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
    fn bypass_is_bit_exact_and_reports_no_reduction() {
        let config = CompressorConfig {
            enabled: false,
            ..base()
        };
        let mut compressor = Compressor::new(config);
        for index in 0..2_000 {
            let input = sine(1_000.0, 0.8, index);
            assert_eq!(compressor.process(input, config), input, "frame {index}");
        }
        assert_eq!(compressor.reduction_db(), 0.0);
    }

    #[test]
    fn a_steady_tone_settles_to_the_ratio() {
        let config = base();
        let mut compressor = Compressor::new(config);
        let out: Vec<[f32; 2]> = (0..48_000)
            .map(|index| compressor.process(sine(1_000.0, 0.5, index), config))
            .collect();
        // -6 dBFS in, -20 dBFS threshold, 4:1: 10.5 dB of reduction, -16.5 dBFS out.
        let held = peak_db(&out[out.len() - 4_800..]);
        assert!((held - -16.5).abs() < 0.5, "held at {held} dBFS");
        assert!(
            (compressor.reduction_db() + 10.5).abs() < 0.5,
            "reduction {} dB",
            compressor.reduction_db()
        );
    }

    #[test]
    fn a_ratio_of_one_does_nothing() {
        let config = CompressorConfig {
            ratio: 1.0,
            threshold_db: -40.0,
            ..base()
        };
        let mut compressor = Compressor::new(config);
        for index in 0..8_000 {
            let input = sine(1_000.0, 0.5, index);
            let out = compressor.process(input, config);
            assert!((out[0] - input[0]).abs() < 1e-6, "frame {index}");
            assert!((out[1] - input[1]).abs() < 1e-6, "frame {index}");
        }
        assert_eq!(compressor.reduction_db(), 0.0);
    }

    #[test]
    fn attack_and_release_follow_their_controls() {
        let config = CompressorConfig {
            attack_ms: 10.0,
            release_ms: 500.0,
            ..base()
        };
        let mut compressor = Compressor::new(config);
        let settled = 10.5;
        let mut reached = None;
        let mut index = 0;
        while index < 9_600 {
            compressor.process(sine(1_000.0, 0.5, index), config);
            if reached.is_none() && -compressor.reduction_db() >= settled * 0.9 {
                reached = Some(index);
            }
            index += 1;
        }
        let struck = reached.expect("the attack never arrived") as f32 / RATE * 1000.0;
        assert!(
            (struck / 10.0 - 1.0).abs() < 0.3,
            "attack took {struck} ms"
        );

        // The signal stops: the reduction has to be given back on the release.
        let mut recovered = None;
        for step in 0..48_000 {
            compressor.process([0.0; 2], config);
            if recovered.is_none() && -compressor.reduction_db() <= settled * 0.1 {
                recovered = Some(step);
            }
        }
        let released = recovered.expect("the release never arrived") as f32 / RATE * 1000.0;
        assert!(
            (released / 500.0 - 1.0).abs() < 0.3,
            "release took {released} ms"
        );
    }

    #[test]
    fn the_knee_has_no_corner() {
        let config = CompressorConfig {
            knee_db: 12.0,
            ..base()
        };
        let mut previous: Option<f32> = None;
        let mut level = config.threshold_db - 8.0;
        while level <= config.threshold_db + 8.0 {
            let reduction = curve_db(level, &config);
            if let Some(previous) = previous {
                assert!(
                    (reduction - previous).abs() <= 1.0,
                    "the curve jumped at {level} dBFS"
                );
            }
            previous = Some(reduction);
            level += 0.25;
        }
        // Outside the knee the full ratio applies: 8 dB over, 4:1 → 6 dB.
        assert!((curve_db(config.threshold_db + 8.0, &config) - 6.0).abs() < 0.01);
        assert_eq!(curve_db(config.threshold_db - 8.0, &config), 0.0);
    }

    #[test]
    fn impossible_settings_are_rejected_and_still_run_finite() {
        let bad = CompressorConfig {
            threshold_db: 12.0,
            ratio: 0.5,
            attack_ms: 0.0,
            ..base()
        };
        assert!(validate(&bad).is_err());
        assert!(validate(&base()).is_ok());

        let mut compressor = Compressor::new(bad);
        for index in 0..2_000 {
            let out = compressor.process(
                [f32::NAN, if index % 2 == 0 { f32::INFINITY } else { 0.5 }],
                bad,
            );
            assert!(out.iter().all(|sample| sample.is_finite()));
            assert!(out.iter().all(|sample| (-1.0..=1.0).contains(sample)));
        }
        assert!(compressor.reduction_db() <= 0.0);
    }
}
