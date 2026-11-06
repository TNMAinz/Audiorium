//! Parametric equalisation for the monitored mix and library playback.
//!
//! Six RBJ cookbook biquads - peaking, low shelf, high shelf, low pass, high
//! pass and notch - run as a cascade on each channel at the stream's 48 kHz.
//! The coefficients that filter the audio are the same ones the response curve
//! is drawn from, so the graph cannot disagree with the sound.
//!
//! The stage sits after the spatial renderer and the room and before the
//! limiter: a headphone correction belongs next to the transducer, and the
//! limiter then guards the headroom a boost takes away. Switching the stage off
//! is bit-exact pass-through. Changing a band while audio runs crossfades the
//! old cascade into the new one over 10 ms, so turning a knob does not click.
//!
//! What this is not: there is no automatic correction, no measured headphone
//! data and no loudness normalisation. A band does exactly what its parameters
//! say, and nothing here knows which headphone is on your head.

use serde::{Deserialize, Serialize};
use std::f32::consts::PI;

/// The one rate both the monitor stream and library playback render at.
pub const RATE: f32 = 48_000.0;
/// Enough bands for a full AutoEq-style correction with room to edit by hand.
pub const MAX_FILTERS: usize = 24;
pub const MIN_HZ: f32 = 20.0;
pub const MAX_HZ: f32 = 20_000.0;
pub const GAIN_LIMIT_DB: f32 = 24.0;
pub const Q_MIN: f32 = 0.1;
pub const Q_MAX: f32 = 18.0;
pub const PREAMP_MIN_DB: f32 = -36.0;
pub const PREAMP_MAX_DB: f32 = 0.0;
/// Samples in the crossfade between two band sets: 10 ms at 48 kHz.
const FADE: u32 = 480;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EqKind {
    Peaking,
    Lowshelf,
    Highshelf,
    Lowpass,
    Highpass,
    Notch,
}

impl EqKind {
    /// The two kinds whose gain means nothing: their shape is set by Q alone.
    pub fn uses_gain(self) -> bool {
        !matches!(self, Self::Lowpass | Self::Highpass | Self::Notch)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EqFilter {
    pub kind: EqKind,
    /// Centre (or corner) frequency in Hz.
    pub frequency: f32,
    /// Boost or cut in dB. Ignored by the pass and notch kinds.
    pub gain_db: f32,
    pub q: f32,
    pub enabled: bool,
}

impl EqFilter {
    pub fn scale(self) -> f32 {
        if self.kind.uses_gain() {
            self.gain_db
        } else {
            0.0
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EqConfig {
    pub enabled: bool,
    /// Attenuation ahead of the bands, so a boost does not run out of headroom.
    pub preamp_db: f32,
    pub filters: Vec<EqFilter>,
}

impl Default for EqConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            preamp_db: 0.0,
            filters: Vec::new(),
        }
    }
}

impl EqConfig {
    /// True when the stage would change a sample. A switch that is on with no
    /// bands doing anything is still a bypass, and is reported as one.
    pub fn is_active(&self) -> bool {
        self.enabled
            && (self.preamp_db.abs() > 1e-6
                || self
                    .filters
                    .iter()
                    .any(|filter| filter.enabled && filter.scale().abs() > 1e-6))
    }
}

pub fn validate(config: &EqConfig) -> Result<(), String> {
    if !config.preamp_db.is_finite()
        || !(PREAMP_MIN_DB..=PREAMP_MAX_DB).contains(&config.preamp_db)
    {
        return Err(format!(
            "Preamp must be between {PREAMP_MIN_DB} and {PREAMP_MAX_DB} dB"
        ));
    }
    if config.filters.len() > MAX_FILTERS {
        return Err(format!("The equaliser takes at most {MAX_FILTERS} bands"));
    }
    for (index, filter) in config.filters.iter().enumerate() {
        let band = index + 1;
        if !filter.frequency.is_finite() || !(MIN_HZ..=MAX_HZ).contains(&filter.frequency) {
            return Err(format!("Band {band} frequency must be between 20 Hz and 20 kHz"));
        }
        if !filter.gain_db.is_finite() || filter.gain_db.abs() > GAIN_LIMIT_DB {
            return Err(format!("Band {band} gain must be within +/-{GAIN_LIMIT_DB} dB"));
        }
        if !filter.q.is_finite() || !(Q_MIN..=Q_MAX).contains(&filter.q) {
            return Err(format!("Band {band} Q must be between {Q_MIN} and {Q_MAX}"));
        }
    }
    Ok(())
}

/// One normalised biquad. `a0` is divided out at construction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Biquad {
    pub fn new(kind: EqKind, frequency: f32, gain_db: f32, q: f32) -> Self {
        // Above Nyquist a biquad is meaningless, so the corner is held just under it.
        let f0 = frequency.clamp(MIN_HZ, RATE * 0.49);
        let q = q.clamp(Q_MIN, Q_MAX);
        let gain = gain_db.clamp(-GAIN_LIMIT_DB, GAIN_LIMIT_DB);
        let w0 = 2.0 * PI * f0 / RATE;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let a = 10f32.powf(gain / 40.0);
        // Shelves use the cookbook's middle slope (S = 1), which reduces to sqrt 2.
        let shelf_alpha = sin / 2.0 * 2f32.sqrt();
        let (b0, b1, b2, a0, a1, a2) = match kind {
            EqKind::Peaking => (
                1.0 + alpha * a,
                -2.0 * cos,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos,
                1.0 - alpha / a,
            ),
            EqKind::Lowshelf => (
                a * ((a + 1.0) - (a - 1.0) * cos + 2.0 * a.sqrt() * shelf_alpha),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                a * ((a + 1.0) - (a - 1.0) * cos - 2.0 * a.sqrt() * shelf_alpha),
                (a + 1.0) + (a - 1.0) * cos + 2.0 * a.sqrt() * shelf_alpha,
                -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                (a + 1.0) + (a - 1.0) * cos - 2.0 * a.sqrt() * shelf_alpha,
            ),
            EqKind::Highshelf => (
                a * ((a + 1.0) + (a - 1.0) * cos + 2.0 * a.sqrt() * shelf_alpha),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                a * ((a + 1.0) + (a - 1.0) * cos - 2.0 * a.sqrt() * shelf_alpha),
                (a + 1.0) - (a - 1.0) * cos + 2.0 * a.sqrt() * shelf_alpha,
                2.0 * ((a - 1.0) - (a + 1.0) * cos),
                (a + 1.0) - (a - 1.0) * cos - 2.0 * a.sqrt() * shelf_alpha,
            ),
            EqKind::Lowpass => (
                (1.0 - cos) / 2.0,
                1.0 - cos,
                (1.0 - cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            EqKind::Highpass => (
                (1.0 + cos) / 2.0,
                -(1.0 + cos),
                (1.0 + cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            EqKind::Notch => (
                1.0,
                -2.0 * cos,
                1.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
        };
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// |H| in dB at `frequency`, from the same coefficients that filter audio.
    pub fn magnitude_db(&self, frequency: f32) -> f32 {
        let w = 2.0 * PI * frequency.clamp(1.0, RATE * 0.4999) / RATE;
        let (sin1, cos1) = w.sin_cos();
        let (sin2, cos2) = (2.0 * w).sin_cos();
        let num_re = self.b0 + self.b1 * cos1 + self.b2 * cos2;
        let num_im = -(self.b1 * sin1 + self.b2 * sin2);
        let den_re = 1.0 + self.a1 * cos1 + self.a2 * cos2;
        let den_im = -(self.a1 * sin1 + self.a2 * sin2);
        let num = (num_re * num_re + num_im * num_im).sqrt();
        let den = (den_re * den_re + den_im * den_im).sqrt();
        if den <= 1e-12 || !num.is_finite() {
            return 0.0;
        }
        20.0 * (num / den).max(1e-12).log10()
    }
}

/// Direct form II transposed: two state values, stable under changing gain.
#[derive(Clone, Copy, Debug, Default)]
struct Channel {
    z1: f32,
    z2: f32,
}

impl Channel {
    fn step(&mut self, biquad: &Biquad, input: f32) -> f32 {
        let output = biquad.b0 * input + self.z1;
        self.z1 = biquad.b1 * input - biquad.a1 * output + self.z2;
        self.z2 = biquad.b2 * input - biquad.a2 * output;
        output
    }
}

#[derive(Clone, Debug)]
struct Stage {
    biquad: Biquad,
    left: Channel,
    right: Channel,
}

/// One frozen band set, kept alive for the length of a crossfade.
struct Bank {
    config: EqConfig,
    stages: Vec<Stage>,
    gain: f32,
}

impl Bank {
    fn new(config: &EqConfig) -> Self {
        let stages = config
            .filters
            .iter()
            .filter(|filter| filter.enabled)
            .map(|filter| Stage {
                biquad: Biquad::new(filter.kind, filter.frequency, filter.gain_db, filter.q),
                left: Channel::default(),
                right: Channel::default(),
            })
            .collect();
        Self {
            config: config.clone(),
            stages,
            gain: 10f32.powf(config.preamp_db / 20.0),
        }
    }

    /// A straight wire, so switching the stage on can fade up instead of jump.
    fn pass_through() -> Self {
        Self {
            config: EqConfig::default(),
            stages: Vec::new(),
            gain: 1.0,
        }
    }

    fn process(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let mut left = frame[0] * self.gain;
        let mut right = frame[1] * self.gain;
        for stage in &mut self.stages {
            left = stage.left.step(&stage.biquad, left);
            right = stage.right.step(&stage.biquad, right);
        }
        [left, right]
    }
}

pub struct Eq {
    active: Bank,
    fade: Option<(Bank, u32)>,
    was_active: bool,
}

impl Eq {
    pub fn new(config: &EqConfig) -> Self {
        Self {
            active: Bank::new(config),
            fade: None,
            was_active: config.is_active(),
        }
    }

    pub fn process(&mut self, frame: [f32; 2], config: &EqConfig) -> [f32; 2] {
        if !config.is_active() {
            if self.was_active {
                // Bypassed is bit-exact: drop both cascades and their tails.
                self.active = Bank::new(config);
                self.fade = None;
                self.was_active = false;
            }
            return frame;
        }
        if &self.active.config != config {
            let previous = std::mem::replace(&mut self.active, Bank::new(config));
            self.fade = Some((previous, FADE));
        } else if !self.was_active {
            // Coming back from bypass: crossfade from the dry signal.
            self.fade = Some((Bank::pass_through(), FADE));
        }
        self.was_active = true;
        let next = self.active.process(frame);
        let Some((previous, remaining)) = self.fade.as_mut() else {
            return next;
        };
        let dry = previous.process(frame);
        let share = *remaining as f32 / FADE as f32;
        *remaining -= 1;
        let mixed = [
            dry[0] * share + next[0] * (1.0 - share),
            dry[1] * share + next[1] * (1.0 - share),
        ];
        if *remaining == 0 {
            self.fade = None;
        }
        mixed
    }
}

/// The curve the graph draws: the cascade of the bands that are switched on.
/// The stage's own bypass does not flatten it, so the stored correction stays
/// readable while the stage is out of the path; the interface labels it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EqResponse {
    /// Log-spaced 20 Hz to 20 kHz, the x axis of the plot.
    pub frequencies: Vec<f32>,
    /// The cascade's gain in dB at each frequency.
    pub gains: Vec<f32>,
    /// The largest boost anywhere in the audio band.
    pub max_boost_db: f32,
    /// Preamp plus that boost: above 0 dB the stage can run out of headroom.
    pub headroom_db: f32,
}

pub fn response(config: &EqConfig, points: usize) -> EqResponse {
    let points = points.clamp(8, 512);
    let active: Vec<Biquad> = config
        .filters
        .iter()
        .filter(|filter| filter.enabled)
        .map(|filter| Biquad::new(filter.kind, filter.frequency, filter.gain_db, filter.q))
        .collect();
    let mut frequencies = Vec::with_capacity(points);
    let mut gains = Vec::with_capacity(points);
    let mut max_boost_db: f32 = 0.0;
    for index in 0..points {
        let share = index as f32 / (points - 1) as f32;
        let frequency = MIN_HZ * (MAX_HZ / MIN_HZ).powf(share);
        let gain = if active.is_empty() {
            0.0
        } else {
            active
                .iter()
                .map(|biquad| biquad.magnitude_db(frequency))
                .sum()
        };
        max_boost_db = max_boost_db.max(gain);
        frequencies.push(frequency);
        gains.push(gain);
    }
    EqResponse {
        frequencies,
        gains,
        max_boost_db,
        headroom_db: config.preamp_db + max_boost_db,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak_filter(frequency: f32, gain_db: f32, q: f32) -> EqFilter {
        EqFilter {
            kind: EqKind::Peaking,
            frequency,
            gain_db,
            q,
            enabled: true,
        }
    }

    fn config(filters: Vec<EqFilter>) -> EqConfig {
        EqConfig {
            enabled: true,
            preamp_db: 0.0,
            filters,
        }
    }

    fn tone(index: usize, frequency: f32, amplitude: f32) -> [f32; 2] {
        let value = (2.0 * std::f32::consts::PI * frequency * index as f32 / RATE).sin() * amplitude;
        [value, value]
    }

    fn render(config: &EqConfig, frames: usize, frequency: f32) -> Vec<[f32; 2]> {
        let mut eq = Eq::new(config);
        (0..frames)
            .map(|index| eq.process(tone(index, frequency, 0.5), config))
            .collect()
    }

    fn amplitude(frames: &[[f32; 2]], skip: usize) -> f32 {
        frames[skip..]
            .iter()
            .fold(0.0f32, |acc, frame| acc.max(frame[0].abs()))
    }

    fn db(value: f32) -> f32 {
        20.0 * value.max(1e-12).log10()
    }

    #[test]
    fn a_peaking_band_reports_its_own_gain_at_its_centre() {
        let biquad = Biquad::new(EqKind::Peaking, 1_000.0, 6.0, 1.4);
        assert!((biquad.magnitude_db(1_000.0) - 6.0).abs() < 0.01);
        // Far from the centre the band is transparent.
        assert!(biquad.magnitude_db(20.0).abs() < 0.2);
        assert!(biquad.magnitude_db(19_000.0).abs() < 0.2);
    }

    #[test]
    fn a_high_pass_removes_the_bass_and_keeps_the_treble() {
        let biquad = Biquad::new(EqKind::Highpass, 1_000.0, 0.0, 0.707);
        assert!(biquad.magnitude_db(100.0) < -30.0);
        assert!(biquad.magnitude_db(10_000.0).abs() < 0.2);
    }

    #[test]
    fn the_cascade_curve_is_the_sum_of_its_bands() {
        let filters = vec![
            peak_filter(80.0, 5.0, 0.9),
            peak_filter(3_000.0, -4.0, 1.2),
        ];
        let curve = response(&config(filters.clone()), 64);
        for (index, frequency) in curve.frequencies.iter().enumerate() {
            let expected: f32 = filters
                .iter()
                .map(|f| Biquad::new(f.kind, f.frequency, f.gain_db, f.q).magnitude_db(*frequency))
                .sum();
            assert!(
                (curve.gains[index] - expected).abs() < 1e-3,
                "{} Hz: {} vs {}",
                frequency,
                curve.gains[index],
                expected
            );
        }
    }

    #[test]
    fn a_disabled_band_changes_neither_the_sound_nor_the_curve() {
        let mut filter = peak_filter(1_000.0, 12.0, 1.0);
        filter.enabled = false;
        let config = config(vec![filter]);
        let curve = response(&config, 32);
        assert!(curve.gains.iter().all(|gain| gain.abs() < 1e-4));
        let out = render(&config, 2_400, 1_000.0);
        // Nothing to apply: a band that is off leaves the stage idle.
        assert!(!config.is_active());
        for (index, frame) in out.iter().enumerate() {
            assert_eq!(*frame, tone(index, 1_000.0, 0.5), "frame {index}");
        }
    }

    #[test]
    fn a_bypassed_stage_is_bit_exact() {
        let config = EqConfig {
            enabled: false,
            preamp_db: -6.0,
            filters: vec![peak_filter(1_000.0, 6.0, 1.0)],
        };
        let out = render(&config, 480, 1_000.0);
        for (index, frame) in out.iter().enumerate() {
            assert_eq!(*frame, tone(index, 1_000.0, 0.5), "frame {index}");
        }
    }

    #[test]
    fn the_preamp_scales_the_output_by_exactly_its_decibels() {
        let config = EqConfig {
            enabled: true,
            preamp_db: -6.0,
            filters: Vec::new(),
        };
        let out = render(&config, 2_400, 1_000.0);
        let rendered = amplitude(&out, 240);
        let expected = 0.5 * 10f32.powf(-6.0 / 20.0);
        assert!(
            (db(rendered) - db(expected)).abs() < 0.1,
            "{} dB vs {} dB",
            db(rendered),
            db(expected)
        );
    }

    #[test]
    fn a_boost_actually_raises_the_band_it_names() {
        let config = config(vec![peak_filter(1_000.0, 6.0, 1.4)]);
        let out = render(&config, 9_600, 1_000.0);
        let boosted = amplitude(&out, 4_800);
        let gain = db(boosted) - db(0.5);
        assert!(gain > 5.6, "the band only raised the tone by {gain:.2} dB");
        assert!(gain < 6.4, "the band raised the tone by {gain:.2} dB");
    }

    #[test]
    fn changing_bands_while_audio_runs_does_not_click() {
        let quiet = config(vec![peak_filter(1_000.0, 0.0, 1.0)]);
        let loud = config(vec![peak_filter(1_000.0, 12.0, 1.0)]);
        let mut eq = Eq::new(&quiet);
        let mut previous = 0.0f32;
        let mut during = 0.0f32;
        let mut after = 0.0f32;
        for index in 0..6_000 {
            let settings = if index < 2_400 { &quiet } else { &loud };
            let frame = eq.process(tone(index, 1_000.0, 0.5), settings);
            let step = (frame[0] - previous).abs();
            // The change happens at 2 400 and fades for 480 samples; the steady
            // state after 3 600 shows how much a sample moves on its own.
            if index > 2_400 && index < 3_600 {
                during = during.max(step);
            } else if index > 3_600 {
                after = after.max(step);
            }
            previous = frame[0];
        }
        assert!(
            during < after * 1.25 + 0.005,
            "the fade stepped {during} against {after} when settled"
        );
    }

    #[test]
    fn the_curve_survives_a_bypassed_stage_so_the_bands_stay_readable() {
        let mut config = config(vec![peak_filter(1_000.0, 6.0, 1.4)]);
        let live = response(&config, 64);
        config.enabled = false;
        assert_eq!(response(&config, 64), live);
    }

    #[test]
    fn headroom_reports_the_boost_a_band_asks_for() {
        let curve = response(&config(vec![peak_filter(1_000.0, 6.0, 1.4)]), 128);
        assert!((curve.max_boost_db - 6.0).abs() < 0.1, "{}", curve.max_boost_db);
        assert!((curve.headroom_db - 6.0).abs() < 0.1, "{}", curve.headroom_db);
        let compensated = EqConfig {
            enabled: true,
            preamp_db: -6.0,
            filters: vec![peak_filter(1_000.0, 6.0, 1.4)],
        };
        assert!(response(&compensated, 128).headroom_db.abs() < 0.1);
    }

    #[test]
    fn impossible_settings_are_rejected() {
        let mut broken = config(vec![peak_filter(1_000.0, 6.0, 1.0)]);
        broken.filters[0].frequency = 0.0;
        assert!(validate(&broken).is_err());
        broken.filters[0].frequency = 1_000.0;
        broken.filters[0].q = 100.0;
        assert!(validate(&broken).is_err());
        broken.filters[0].q = 1.0;
        broken.filters[0].gain_db = 40.0;
        assert!(validate(&broken).is_err());
        broken.filters[0].gain_db = 6.0;
        broken.preamp_db = 6.0;
        assert!(validate(&broken).is_err());
        broken.preamp_db = -6.0;
        broken.filters = vec![peak_filter(1_000.0, 6.0, 1.0); MAX_FILTERS + 1];
        assert!(validate(&broken).is_err());
        broken.filters.truncate(1);
        assert!(validate(&broken).is_ok());

        let mut not_a_number = config(vec![peak_filter(f32::NAN, 6.0, 1.0)]);
        assert!(validate(&not_a_number).is_err());
        not_a_number.filters[0].frequency = 1_000.0;
        not_a_number.filters[0].gain_db = f32::INFINITY;
        assert!(validate(&not_a_number).is_err());
    }

    #[test]
    fn every_output_stays_finite_with_extreme_bands() {
        let config = config(vec![
            peak_filter(20.0, 24.0, 18.0),
            peak_filter(19_000.0, 24.0, 0.1),
            EqFilter { kind: EqKind::Highpass, frequency: 20.0, gain_db: 0.0, q: 0.1, enabled: true },
            EqFilter { kind: EqKind::Notch, frequency: 1_000.0, gain_db: 0.0, q: 18.0, enabled: true },
        ]);
        let out = render(&config, 9_600, 440.0);
        assert!(out
            .iter()
            .all(|frame| frame[0].is_finite() && frame[1].is_finite()));
    }
}
