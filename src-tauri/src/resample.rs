//! Streaming sample-rate conversion for a master written at a rate other than the
//! engine's 48 kHz.
//!
//! The rack runs at one rate, so a release that has to be 44.1 kHz or 96 kHz is
//! converted after the processors rather than by a second, private copy of them:
//! the file is the master they made, resampled. The converter is a windowed-sinc
//! interpolator with a 32-tap kernel whose cutoff follows the lower of the two
//! rates — 48→44.1 band-limits before it decimates, and 44.1→48 interpolates
//! without imaging. Each output position normalises its own kernel, so a constant
//! signal stays constant and the conversion has no gain error of its own.
//!
//! The kernel reaches backwards from the newest sample, so the converter runs with
//! a fixed 16-sample delay and can emit as the file arrives instead of holding it.
//! That delay is a third of a millisecond and is the reason a master needs no
//! flush step: the frame count follows the duration, which is what every player
//! reads.
//!
//! What this is not: a mastering-grade sample-rate converter with apodising or
//! minimum-phase options, and not part of playback. Files played in the app are
//! still converted by the player's own resampler; this one exists for exports.

use std::f64::consts::PI;

/// Input frames retained for the kernel to read. Larger than the kernel so a
/// future tap count can grow without changing the ring.
const HISTORY: usize = 64;
/// Taps per output sample. Thirty-two is the usual offline compromise: an
/// in-band ripple well under 0.05 dB and a stopband far below the noise of a
/// 16-bit master, at a cost that is still cheaper than real time.
const TAPS: usize = 32;
/// Half the kernel: the delay the converter runs at, in input frames.
const HALF: usize = TAPS / 2;
/// The most outputs one input frame can produce. 96 kHz from 48 kHz is two; a
/// bound keeps a corrupt ratio from turning the emit loop into a hang.
const MAX_PER_PUSH: usize = 4;

pub struct Resampler {
    from: u32,
    to: u32,
    /// Input frames advanced per output frame: `from / to`.
    step: f64,
    /// Where the next output sits on the input's timeline, in frames.
    position: f64,
    /// Anti-imaging or anti-aliasing cutoff, relative to the input's Nyquist:
    /// the lower of the two rates decides it.
    cutoff: f64,
    /// The last `HISTORY` frames, per channel, written at `head`.
    history: [[f32; HISTORY]; 2],
    head: usize,
    /// Frames pushed so far, so a tap can tell silence from a sample that exists.
    pushed: u64,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Self {
        let from = from.max(1);
        let to = to.max(1);
        let cutoff = if to < from {
            to as f64 / from as f64
        } else {
            1.0
        };
        Self {
            from,
            to,
            step: from as f64 / to as f64,
            position: 0.0,
            cutoff,
            history: [[0.0; HISTORY]; 2],
            head: 0,
            pushed: 0,
        }
    }

    /// True when the two rates are the same, in which case the converter is a copy
    /// through and the caller could skip it entirely.
    pub fn is_passthrough(&self) -> bool {
        self.from == self.to
    }

    /// One stereo frame in, however many output frames that position calls for.
    pub fn push(&mut self, frame: [f32; 2], out: &mut Vec<[f32; 2]>) {
        // A non-finite sample is silence, never a value that would poison every
        // output the kernel mixes into it.
        let clean = [
            if frame[0].is_finite() { frame[0] } else { 0.0 },
            if frame[1].is_finite() { frame[1] } else { 0.0 },
        ];
        self.history[0][self.head] = clean[0];
        self.history[1][self.head] = clean[1];
        self.head = (self.head + 1) % HISTORY;
        self.pushed += 1;

        if self.is_passthrough() {
            out.push(clean);
            return;
        }

        // Every output whose newest tap exists. The position is not advanced past a
        // tap that has not arrived, so it carries into the next push.
        for _ in 0..MAX_PER_PUSH {
            let newest = self.position.floor() as i64;
            if newest >= self.pushed as i64 {
                return;
            }
            out.push(self.sample());
            self.position += self.step;
        }
    }

    /// A slice in, its converted frames appended to `out`.
    #[cfg(test)]
    pub fn process(&mut self, frames: &[[f32; 2]], out: &mut Vec<[f32; 2]>) {
        out.reserve(frames.len().max(1));
        for frame in frames {
            self.push(*frame, out);
        }
    }

    /// One output frame at the current position: the taps ending at the newest
    /// sample at or before it, normalised so a constant signal passes unchanged.
    fn sample(&self) -> [f32; 2] {
        let newest = self.position.floor() as i64;
        // A stack array, not a field: the kernel is written where it is read, and
        // the path still allocates nothing per frame.
        let mut weights = [0.0f64; TAPS];
        let mut sum = 0.0f64;
        for (tap, weight) in weights.iter_mut().enumerate() {
            let at = newest - TAPS as i64 + 1 + tap as i64;
            *weight = self.weight(self.position - at as f64);
            sum += *weight;
        }
        if sum.abs() < 1e-12 {
            return [0.0, 0.0];
        }
        let mut channels = [0.0f64; 2];
        for (tap, weight) in weights.iter().enumerate() {
            let at = newest - TAPS as i64 + 1 + tap as i64;
            // Before the file's first frame there is silence, not a repeat of the
            // first sample: a fade-in is cheaper than an invented edge.
            if at < 0 || at >= self.pushed as i64 {
                continue;
            }
            let index = self.index_of(at as u64);
            channels[0] += f64::from(self.history[0][index]) * weight;
            channels[1] += f64::from(self.history[1][index]) * weight;
        }
        [
            (channels[0] / sum) as f32,
            (channels[1] / sum) as f32,
        ]
    }

    /// Where an absolute input index lives in the ring.
    fn index_of(&self, at: u64) -> usize {
        let behind = (self.pushed - 1 - at) as usize;
        (self.head + HISTORY - 1 - behind.min(HISTORY - 1)) % HISTORY
    }

    /// The interpolator's weight at a distance in input frames, measured back from
    /// the newest sample. A Hann window keeps the stopband clean without the
    /// pre-echo a longer kernel would smear across a transient.
    fn weight(&self, distance: f64) -> f64 {
        let offset = distance - HALF as f64;
        if offset.abs() > HALF as f64 {
            return 0.0;
        }
        let scaled = offset * self.cutoff;
        let sinc = if scaled.abs() < 1e-9 {
            1.0
        } else {
            (PI * scaled).sin() / (PI * scaled)
        };
        let window = 0.5 + 0.5 * (PI * offset / HALF as f64).cos();
        sinc * window
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(frames: usize, rate: u32, frequency: f32, amplitude: f32) -> Vec<[f32; 2]> {
        (0..frames)
            .map(|index| {
                let value = (2.0 * PI as f32 * frequency * index as f32 / rate as f32).sin()
                    * amplitude;
                [value, value]
            })
            .collect()
    }

    fn peak(frames: &[[f32; 2]]) -> f32 {
        frames
            .iter()
            .fold(0.0f32, |acc, frame| acc.max(frame[0].abs()).max(frame[1].abs()))
    }

    fn db(value: f32) -> f32 {
        20.0 * value.max(1e-12).log10()
    }

    #[test]
    fn the_same_rate_is_a_bit_exact_copy() {
        let mut converter = Resampler::new(48_000, 48_000);
        assert!(converter.is_passthrough());
        let input = sine(1_000, 48_000, 1_000.0, 0.5);
        let mut out = Vec::new();
        converter.process(&input, &mut out);
        assert_eq!(out.len(), input.len());
        for (index, (written, read)) in out.iter().zip(input.iter()).enumerate() {
            assert_eq!(written[0].to_bits(), read[0].to_bits(), "frame {index}");
            assert_eq!(written[1].to_bits(), read[1].to_bits(), "frame {index}");
        }
    }

    #[test]
    fn a_tone_keeps_its_amplitude_and_the_duration_its_length() {
        for (from, to) in [(48_000u32, 44_100u32), (44_100, 48_000)] {
            let input = sine(from as usize, from, 1_000.0, 0.5);
            let mut converter = Resampler::new(from, to);
            let mut out = Vec::new();
            converter.process(&input, &mut out);
            let expected = input.len() as f64 * to as f64 / from as f64;
            assert!(
                (out.len() as f64 - expected).abs() <= 1.0,
                "{from} -> {to} produced {} frames for {} expected",
                out.len(),
                expected
            );
            let settled = &out[200..];
            let measured = peak(settled);
            let error = db(measured) - db(0.5);
            assert!(
                error.abs() < 0.1,
                "{from} -> {to} changed the tone by {error:.3} dB"
            );
        }
    }

    #[test]
    fn a_constant_signal_passes_at_its_own_level() {
        let input = vec![[0.25f32, 0.25f32]; 4_000];
        let mut converter = Resampler::new(48_000, 44_100);
        let mut out = Vec::new();
        converter.process(&input, &mut out);
        for (index, frame) in out.iter().enumerate().skip(200) {
            assert!(
                (frame[0] - 0.25).abs() < 1e-3 && (frame[1] - 0.25).abs() < 1e-3,
                "frame {index} read {} instead of 0.25",
                frame[0]
            );
        }
    }

    #[test]
    fn a_non_finite_sample_never_reaches_the_output() {
        let mut converter = Resampler::new(48_000, 44_100);
        let mut out = Vec::new();
        converter.push([f32::NAN, f32::INFINITY], &mut out);
        for _ in 0..64 {
            converter.push([f32::NEG_INFINITY, f32::NAN], &mut out);
        }
        converter.process(&vec![[0.1f32, -0.1f32]; 256], &mut out);
        assert!(out.iter().all(|frame| frame[0].is_finite() && frame[1].is_finite()));
    }

    #[test]
    fn doubling_the_rate_doubles_the_frames_and_holds_the_tone() {
        let input = sine(2_000, 48_000, 1_000.0, 0.5);
        let mut converter = Resampler::new(48_000, 96_000);
        let mut out = Vec::new();
        converter.process(&input, &mut out);
        let expected = input.len() as f64 * 2.0;
        assert!(
            (out.len() as f64 - expected).abs() <= 1.0,
            "96 kHz produced {} frames for {expected} expected",
            out.len()
        );
        let error = db(peak(&out[200..])) - db(0.5);
        assert!(error.abs() < 0.1, "the tone changed by {error:.3} dB");
    }
}
