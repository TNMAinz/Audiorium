//! Programme loudness, measured the way a master has to be measured.
//!
//! The analysis engine answers "how loud is it right now" for the scene. A file
//! on its way out needs the other question: what will the whole programme read
//! when a player or a loudness-normalising service measures it? That is ITU-R
//! BS.1770-4: K-weighted mean square over 400 ms blocks, gated absolute and
//! relative, with a true peak from an oversampled interpolation. Nothing here
//! is approximate on purpose — the numbers are the standard's, from the
//! standard's own coefficients, so an exported file can carry them.
//!
//! The meter never returns a NaN. Silence reads as the floor, not as minus
//! infinity, because every panel that shows this also has to lay it out.

use std::collections::VecDeque;

use serde::Serialize;

/// The loudness floor the panels print. BS.1770's absolute gate is the same
/// number: a block quieter than -70 LUFS does not take part in the integrated
/// figure, and silence reads as the floor rather than as a number no one can use.
pub const FLOOR_LUFS: f32 = -70.0;
/// Peak, correlation and DC floors, in the units each one is read in.
const FLOOR_DB: f32 = -120.0;

/// The one rate the app runs at, and the only rate this meter is built for.
const RATE: usize = 48_000;
/// 100 ms: a 400 ms block advances by this much, so blocks overlap by 75 %.
const HOP: usize = RATE / 10;
/// Four hops make the momentary block the standard asks for.
const BLOCK_HOPS: usize = 4;
/// Thirty hops are the 3 s short-term window LRA is built from.
const SHORT_HOPS: usize = 30;
/// Ten hops are one second: what correlation and DC are measured over.
const LEVEL_HOPS: usize = 10;
/// The oversampling factor for true peak, and the taps each phase carries. Shared
/// with the limiter, so the meter and the limiter mean the same thing by a peak.
pub(crate) const PHASES: usize = 4;
pub(crate) const TAPS: usize = 12;
/// Two hours of blocks. A longer programme keeps the newest ones, which is the
/// only choice that does not make a render of a whole day unbounded in memory.
const BLOCK_CAP: usize = 72_000;

/// The standard's two K-weighting stages at 48 kHz.
///
/// The pre-filter is the head's own shelf: +4 dB above 1.5 kHz, flat below,
/// built from the published analogue prototype and bilinear-transformed at this
/// rate. The second stage is the RLB high-pass that takes the very bottom out.
/// Together they read a 1 kHz sine 0.69 dB above its RMS, which is exactly what
/// the -0.691 dB offset in `loudness` below cancels: a stereo pair of
/// full-scale 1 kHz sines reads 0.0 LUFS.
const SHELF: ([f32; 3], [f32; 3]) = (
    [1.535_124_8, -2.691_696_2, 1.198_392_8],
    [1.0, -1.690_659_3, 0.732_480_77],
);
const HIGH_PASS: ([f32; 3], [f32; 3]) = (
    [1.0, -2.0, 1.0],
    [1.0, -1.990_047_4, 0.990_072_25],
);

#[cfg(test)]
fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

fn linear_to_db(value: f32) -> f32 {
    if !value.is_finite() || value <= 1e-6 {
        FLOOR_DB
    } else {
        (20.0 * value.log10()).max(FLOOR_DB)
    }
}

/// One biquad, direct form I. The state is per channel, so two meters never share it.
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    fn new((b, a): ([f32; 3], [f32; 3])) -> Self {
        Self {
            b0: b[0],
            b1: b[1],
            b2: b[2],
            a1: a[1],
            a2: a[2],
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// The two stages in series, one channel.
struct KWeighted {
    shelf: Biquad,
    high_pass: Biquad,
}

impl KWeighted {
    fn new() -> Self {
        Self {
            shelf: Biquad::new(SHELF),
            high_pass: Biquad::new(HIGH_PASS),
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        self.high_pass.process(self.shelf.process(x))
    }
}

/// One published set of measurements. Every field is finite; a value below the
/// meter's floor is the floor, so the interface never has to print minus infinity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoudnessReadings {
    pub integrated_lufs: f32,
    pub short_term_lufs: f32,
    pub momentary_lufs: f32,
    pub loudness_range: f32,
    pub true_peak_db: f32,
    pub sample_peak_db: f32,
    pub dc_offset_db: f32,
    pub correlation: f32,
    /// Completed 400 ms blocks, so a panel can say it is still measuring rather
    /// than showing a floor that looks like silence.
    pub blocks: usize,
}

impl Default for Loudness {
    fn default() -> Self {
        Self::new()
    }
}

/// The meter itself. Push frames; read whenever a panel needs an answer.
pub struct Loudness {
    left: KWeighted,
    right: KWeighted,
    /// Sums over the hop in flight, in f64: gating turns these into means, and a
    /// f32 accumulator drifts over a long programme.
    hop_low: f64,
    hop_wide: f64,
    hop_lr: f64,
    /// The signed sum of the mid signal: a constant offset has to survive the
    /// averaging, so this one is not a sum of squares.
    hop_mid: f64,
    hop_power: f64,
    hop_frames: usize,
    /// The last ten seconds of hops: energy for the short-term window, and the
    /// three sums correlation and DC are read from.
    hops: VecDeque<Hop>,
    blocks: Vec<f64>,
    short: Vec<f64>,
    sample_peak: f32,
    true_peak: f32,
    /// An oversampled peak needs the samples around the one being judged. Twelve
    /// taps per phase at four phases is a 48-tap interpolation: inaudible on a
    /// 1 kHz tone and exact on the inter-sample crest a limiter can miss.
    phases: [[f32; TAPS]; PHASES],
    history: [[f32; TAPS]; 2],
    head: usize,
}

#[derive(Clone, Copy, Default)]
struct Hop {
    /// Sum of squares of the K-weighted left and right channels: what loudness is.
    power: f64,
    low: f64,
    wide: f64,
    lr: f64,
    mid: f64,
    frames: usize,
}

impl Loudness {
    pub fn new() -> Self {
        Self {
            left: KWeighted::new(),
            right: KWeighted::new(),
            hop_low: 0.0,
            hop_wide: 0.0,
            hop_lr: 0.0,
            hop_mid: 0.0,
            hop_power: 0.0,
            hop_frames: 0,
            hops: VecDeque::with_capacity(SHORT_HOPS),
            blocks: Vec::new(),
            short: Vec::new(),
            sample_peak: 0.0,
            true_peak: 0.0,
            phases: interpolation(),
            history: [[0.0; TAPS]; 2],
            head: 0,
        }
    }


    /// One stereo frame. Non-finite samples read as silence rather than poisoning
    /// every reading that follows them.
    pub fn push(&mut self, frame: [f32; 2]) {
        let left = if frame[0].is_finite() { frame[0] } else { 0.0 };
        let right = if frame[1].is_finite() { frame[1] } else { 0.0 };

        self.sample_peak = self.sample_peak.max(left.abs()).max(right.abs());
        let peak = self.overlap(0, left).max(self.overlap(1, right));
        if peak > self.true_peak {
            self.true_peak = peak;
        }
        self.history[0][self.head] = left;
        self.history[1][self.head] = right;
        self.head = (self.head + 1) % TAPS;

        let keyed_left = self.left.process(left);
        let keyed_right = self.right.process(right);
        self.hop_power += (keyed_left as f64).powi(2) + (keyed_right as f64).powi(2);
        self.hop_low += (left as f64).powi(2);
        self.hop_wide += (right as f64).powi(2);
        self.hop_lr += left as f64 * right as f64;
        self.hop_mid += (left as f64 + right as f64) * 0.5;
        self.hop_frames += 1;
        if self.hop_frames == HOP {
            self.close_hop();
        }
    }

    fn overlap(&self, channel: usize, sample: f32) -> f32 {
        // The history is read as if this sample had already been written, so the
        // filter sees the twelve samples ending at the new one.
        let mut best = sample.abs();
        for phase in &self.phases {
            let mut sum = 0.0f32;
            for (tap, coefficient) in phase.iter().enumerate() {
                let age = (self.head + TAPS - tap) % TAPS;
                let value = if tap == 0 { sample } else { self.history[channel][age] };
                sum += coefficient * value;
            }
            best = best.max(sum.abs());
        }
        best
    }

    fn close_hop(&mut self) {
        let hop = Hop {
            power: self.hop_power,
            low: self.hop_low,
            wide: self.hop_wide,
            lr: self.hop_lr,
            mid: self.hop_mid,
            frames: self.hop_frames,
        };
        self.hops.push_back(hop);
        while self.hops.len() > SHORT_HOPS {
            self.hops.pop_front();
        }
        if self.hops.len() >= BLOCK_HOPS {
            let energy: f64 = self
                .hops
                .iter()
                .rev()
                .take(BLOCK_HOPS)
                .map(|hop| hop.power)
                .sum();
            push_capped(&mut self.blocks, energy / (BLOCK_HOPS * HOP) as f64, BLOCK_CAP);
        }
        if self.hops.len() == SHORT_HOPS {
            let energy: f64 = self.hops.iter().map(|hop| hop.power).sum();
            push_capped(&mut self.short, energy / (SHORT_HOPS * HOP) as f64, BLOCK_CAP);
        }
        self.hop_power = 0.0;
        self.hop_low = 0.0;
        self.hop_wide = 0.0;
        self.hop_lr = 0.0;
        self.hop_mid = 0.0;
        self.hop_frames = 0;
    }

    pub fn readings(&self) -> LoudnessReadings {
        let block_energy = |hops: usize| -> f32 {
            if self.hops.len() < hops {
                return FLOOR_LUFS;
            }
            let energy: f64 = self.hops.iter().rev().take(hops).map(|hop| hop.power).sum();
            loudness(energy / (hops * HOP) as f64)
        };
        let levels = self.hops.len().min(LEVEL_HOPS);
        let (mut low, mut wide, mut lr, mut mid, mut frames) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0usize);
        for hop in self.hops.iter().rev().take(levels) {
            low += hop.low;
            wide += hop.wide;
            lr += hop.lr;
            mid += hop.mid;
            frames += hop.frames;
        }
        let correlation = if low > 1e-12 && wide > 1e-12 {
            (lr / (low * wide).sqrt()).clamp(-1.0, 1.0) as f32
        } else {
            0.0
        };
        // A standing offset is the mean of the mid signal over the window, which
        // is the one figure a symmetric signal can hide and a master has to expose.
        let dc = if frames > 0 {
            linear_to_db((mid / frames as f64).abs() as f32)
        } else {
            FLOOR_DB
        };

        LoudnessReadings {
            integrated_lufs: self.integrated(),
            short_term_lufs: self.short_term(),
            momentary_lufs: block_energy(BLOCK_HOPS),
            loudness_range: self.loudness_range(),
            true_peak_db: linear_to_db(self.true_peak),
            sample_peak_db: linear_to_db(self.sample_peak),
            dc_offset_db: dc,
            correlation,
            blocks: self.blocks.len(),
        }
    }

    /// The short-term window's loudness, from the same hops LRA is built from.
    fn short_term(&self) -> f32 {
        if self.hops.len() < SHORT_HOPS {
            return FLOOR_LUFS;
        }
        let energy: f64 = self.hops.iter().map(|hop| hop.power).sum();
        loudness(energy / (SHORT_HOPS * HOP) as f64)
    }

    /// The 400 ms block energies, in order, so a caller can combine several files
    /// into one programme loudness — an album — without re-reading them.
    pub fn block_powers(&self) -> &[f64] {
        &self.blocks
    }

    /// The 3 s window's power as it advances every 100 ms: the shape of the
    /// programme, which is what a whole-file view draws.
    pub fn short_powers(&self) -> &[f64] {
        &self.short
    }

    /// One block's loudness in LUFS, the same conversion the meter reads.
    pub fn lufs(power: f64) -> f32 {
        loudness(power)
    }

    /// 400 ms blocks, absolute gate at -70 LUFS, relative gate 10 LU under the
    /// mean of what passed the absolute one.
    fn integrated(&self) -> f32 {
        integrated_of(&self.blocks)
    }

    /// The range between the 10th and 95th percentile of the 3 s blocks that pass
    /// the LRA gates. Two blocks are the least that can describe a range.
    fn loudness_range(&self) -> f32 {
        let audible: Vec<f64> = self
            .short
            .iter()
            .copied()
            .filter(|energy| loudness(*energy) > FLOOR_LUFS)
            .collect();
        if audible.len() < 2 {
            return 0.0;
        }
        let mean = audible.iter().sum::<f64>() / audible.len() as f64;
        let gate = loudness(mean) - 20.0;
        let mut kept: Vec<f32> = audible
            .into_iter()
            .filter(|energy| loudness(*energy) > gate)
            .map(loudness)
            .collect();
        if kept.len() < 2 {
            return 0.0;
        }
        kept.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let low = percentile(&kept, 10.0);
        let high = percentile(&kept, 95.0);
        (high - low).max(0.0)
    }
}

/// Programme loudness from block energies alone: the absolute gate at -70 LUFS and
/// the relative gate 10 LU under the mean of what passed it. Several files' blocks
/// concatenated are one programme, which is how an album is judged.
pub fn integrated_of(blocks: &[f64]) -> f32 {
    let audible: Vec<f64> = blocks
        .iter()
        .copied()
        .filter(|energy| loudness(*energy) > FLOOR_LUFS)
        .collect();
    if audible.is_empty() {
        return FLOOR_LUFS;
    }
    let mean = audible.iter().sum::<f64>() / audible.len() as f64;
    let gate = loudness(mean) - 10.0;
    let kept: Vec<f64> = audible
        .into_iter()
        .filter(|energy| loudness(*energy) > gate)
        .collect();
    if kept.is_empty() {
        return loudness(mean);
    }
    loudness(kept.iter().sum::<f64>() / kept.len() as f64)
}

fn percentile(sorted: &[f32], percent: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let at = (percent / 100.0).clamp(0.0, 1.0) * (sorted.len() - 1) as f32;
    let low = at.floor() as usize;
    let high = (low + 1).min(sorted.len() - 1);
    let fraction = at - low as f32;
    sorted[low] * (1.0 - fraction) + sorted[high] * fraction
}

/// The standard's loudness of a mean square summed over the channels it was given.
/// -0.691 dB is the offset that makes a stereo full-scale 1 kHz sine read 0 LUFS.
fn loudness(energy: f64) -> f32 {
    if !energy.is_finite() || energy <= 1e-12 {
        return FLOOR_LUFS;
    }
    let lu = -0.691 + 10.0 * energy.log10();
    (lu as f32).max(FLOOR_LUFS)
}

fn push_capped(values: &mut Vec<f64>, value: f64, cap: usize) {
    if values.len() == cap {
        values.remove(0);
    }
    values.push(value);
}

/// Four polyphase branches of one windowed-sinc lowpass, built once. Each branch
/// is normalised to unity sum, so a constant signal passes unchanged and the peak
/// search measures the signal rather than the filter.
pub(crate) fn interpolation() -> [[f32; TAPS]; PHASES] {
    let length = PHASES * TAPS;
    let centre = (length as f32 - 1.0) * 0.5;
    let mut prototype = [0.0f32; PHASES * TAPS];
    for (index, tap) in prototype.iter_mut().enumerate() {
        let x = (index as f32 - centre) / PHASES as f32;
        let sinc = if x.abs() < 1e-9 {
            1.0
        } else {
            (std::f32::consts::PI * x).sin() / (std::f32::consts::PI * x)
        };
        // A Hann window keeps the branch short enough to cost little and long
        // enough that a 1 kHz tone is exact to the last decimal.
        let window = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * index as f32 / (length as f32 - 1.0)).cos();
        *tap = sinc * window;
    }
    let mut branches = [[0.0f32; TAPS]; PHASES];
    for phase in 0..PHASES {
        let mut sum = 0.0;
        for tap in 0..TAPS {
            branches[phase][tap] = prototype[tap * PHASES + phase];
            sum += branches[phase][tap];
        }
        if sum.abs() > 1e-9 {
            for tap in 0..TAPS {
                branches[phase][tap] /= sum;
            }
        }
    }
    branches
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(amplitude: f32, hz: f32, index: usize) -> [f32; 2] {
        let value = (2.0 * std::f32::consts::PI * hz * index as f32 / RATE as f32).sin() * amplitude;
        [value, value]
    }

    fn run(seconds: f32, frame: impl Fn(usize) -> [f32; 2]) -> LoudnessReadings {
        let mut meter = Loudness::new();
        let frames = (seconds * RATE as f32) as usize;
        for index in 0..frames {
            meter.push(frame(index));
        }
        meter.readings()
    }

    #[test]
    fn a_1_khz_sine_reads_its_own_level() {
        let quiet = run(5.0, |index| tone(db_to_linear(-23.0), 1_000.0, index));
        // The standard is calibrated so this is exactly the tone's level: the
        // K-weighting's +0.69 dB and the stereo sum's +3 dB are what -0.691 cancels.
        assert!(
            (quiet.integrated_lufs + 23.0).abs() < 0.3,
            "read {:.2} LUFS at -23 dBFS",
            quiet.integrated_lufs
        );
        let loud = run(5.0, |index| tone(db_to_linear(-20.0), 1_000.0, index));
        assert!(
            (loud.integrated_lufs - quiet.integrated_lufs - 3.0).abs() < 0.2,
            "3 dB more input read {:.2} LU more",
            loud.integrated_lufs - quiet.integrated_lufs
        );
        assert!(quiet.blocks >= 40, "only {} blocks in 5 s", quiet.blocks);
    }

    #[test]
    fn silence_reads_the_floor_and_never_a_nan() {
        let readings = run(2.0, |_| [0.0, 0.0]);
        assert_eq!(readings.integrated_lufs, FLOOR_LUFS);
        assert_eq!(readings.short_term_lufs, FLOOR_LUFS);
        assert_eq!(readings.momentary_lufs, FLOOR_LUFS);
        assert_eq!(readings.loudness_range, 0.0);
        assert_eq!(readings.true_peak_db, FLOOR_DB);
        assert_eq!(readings.sample_peak_db, FLOOR_DB);
        assert_eq!(readings.dc_offset_db, FLOOR_DB);
        assert_eq!(readings.correlation, 0.0);
        assert!(readings.blocks > 0, "silence was not measured at all");
        for value in [
            readings.integrated_lufs,
            readings.short_term_lufs,
            readings.momentary_lufs,
            readings.loudness_range,
            readings.true_peak_db,
            readings.sample_peak_db,
            readings.dc_offset_db,
            readings.correlation,
        ] {
            assert!(value.is_finite(), "{value} is not a number");
        }
    }

    #[test]
    fn a_full_scale_sine_is_found_at_the_ceiling() {
        let readings = run(1.0, |index| tone(1.0, 1_000.0, index));
        assert!(
            readings.true_peak_db >= -0.1,
            "true peak {:.3} dBTP",
            readings.true_peak_db
        );
        assert!(
            (readings.sample_peak_db - 0.0).abs() < 0.1,
            "sample peak {:.3} dBFS",
            readings.sample_peak_db
        );
        assert!(readings.true_peak_db >= readings.sample_peak_db - 0.01);
    }

    #[test]
    fn a_steady_tone_has_no_range_and_a_swinging_one_does() {
        let steady = run(6.0, |index| tone(db_to_linear(-20.0), 1_000.0, index));
        assert!(steady.loudness_range < 0.5, "steady tone read {:.2} LU", steady.loudness_range);

        // Twelve seconds at each level: enough 3 s blocks on both sides for the
        // percentiles to describe the step rather than the edges.
        let swinging = run(24.0, |index| {
            let amplitude = if index < 12 * RATE {
                db_to_linear(-20.0)
            } else {
                db_to_linear(-40.0)
            };
            tone(amplitude, 1_000.0, index)
        });
        assert!(
            swinging.loudness_range > 10.0,
            "a 20 dB step read {:.2} LU",
            swinging.loudness_range
        );
        // The relative gate drops the quiet half, which is the standard's own
        // behaviour: the integrated figure describes the loud programme.
        assert!(
            (swinging.integrated_lufs + 20.0).abs() < 1.0,
            "integrated {:.2} LUFS over the step",
            swinging.integrated_lufs
        );
    }

    #[test]
    fn correlation_reads_the_image_not_its_level() {
        let phase = |index: usize| tone(0.5, 1_000.0, index)[0];
        let identical = run(2.0, |index| [phase(index), phase(index)]);
        assert!((identical.correlation - 1.0).abs() < 0.01, "{:.3}", identical.correlation);
        let anti = run(2.0, |index| [phase(index), -phase(index)]);
        assert!((anti.correlation + 1.0).abs() < 0.01, "{:.3}", anti.correlation);
        let left_only = run(2.0, |index| [phase(index), 0.0]);
        assert!(left_only.correlation.abs() < 0.01, "{:.3}", left_only.correlation);
    }

    #[test]
    fn garbage_in_never_becomes_garbage_out() {
        let mut meter = Loudness::new();
        for index in 0..12_000 {
            let frame = match index % 4 {
                0 => [f32::NAN, 0.2],
                1 => [f32::INFINITY, f32::NEG_INFINITY],
                2 => [0.3, f32::NAN],
                _ => tone(0.4, 1_000.0, index),
            };
            meter.push(frame);
        }
        let readings = meter.readings();
        for value in [
            readings.integrated_lufs,
            readings.short_term_lufs,
            readings.momentary_lufs,
            readings.loudness_range,
            readings.true_peak_db,
            readings.sample_peak_db,
            readings.dc_offset_db,
            readings.correlation,
        ] {
            assert!(value.is_finite(), "{value} is not a number");
        }
        assert!(readings.true_peak_db <= 0.1, "a non-finite frame inflated the peak");
    }

    #[test]
    fn every_render_measures_from_an_empty_meter() {
        // The master page builds its meters per render, so a second render can never
        // inherit the first one's programme.
        let mut first = Loudness::new();
        for index in 0..24_000 {
            first.push(tone(0.5, 1_000.0, index));
        }
        assert!(first.readings().integrated_lufs > FLOOR_LUFS);
        let readings = Loudness::new().readings();
        assert_eq!(readings.integrated_lufs, FLOOR_LUFS);
        assert_eq!(readings.blocks, 0);
        assert_eq!(readings.sample_peak_db, FLOOR_DB);
    }
}
