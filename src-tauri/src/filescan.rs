//! A whole file looked at once: its peak envelope, its loudness over time and a
//! coarse spectrogram, for the Master page to draw.
//!
//! This is a display, not a calibrated analyser. The envelope is the largest
//! sample in each bucket, the loudness curve is the meter's own 3 s window as it
//! advances, and the spectrogram is 480 columns of 40 log bands taken from a 2048
//! sample Hann window — enough to see where a master's energy sits, not enough to
//! read a level off. The page draws the columns at a readable width and lets the
//! picture be moved sideways, so a four-minute file is inspected rather than
//! squashed into one card width. Nothing here is averaged into a claim: the figures the master
//! is judged by come from `loudness.rs`, in the same pass that writes the file.
//!
//! The scan stops as soon as every column's window has been read and every
//! envelope bucket is full, so looking at a long file costs what the picture
//! costs rather than what the file costs.

use std::path::Path;

use serde::Serialize;

use crate::analysis::fft_in_place;
use crate::loudness::Loudness;
use crate::player;

/// The one rate the engine decodes at, so a bucket is a known length of time.
const RATE: f64 = 48_000.0;
/// Points across the file for the waveform: one per ~0.1 % of its length.
const ENVELOPE_POINTS: usize = 900;
/// Points for the loudness curve, which advances every 100 ms in the meter.
const LOUDNESS_POINTS: usize = 1_200;
/// Spectrogram columns and bands. Forty log bands is a third of an octave each;
/// 480 columns put a four-minute file at about half a second per column, which is
/// detail worth scrolling to rather than a squash to one card width.
const COLUMNS: usize = 480;
const BANDS: usize = 40;
/// The window every column is measured over: 43 ms at 48 kHz.
const FFT: usize = 2_048;
/// The band edges, from a rumble to the top of what a master carries.
const MIN_HZ: f32 = 40.0;
const MAX_HZ: f32 = 16_000.0;
/// Below this a bin is the floor of the picture, not a reading.
const FLOOR_DB: f32 = -90.0;
/// A loudness curve never reads quieter than this, so the plot has a floor.
const LOUDNESS_FLOOR: f32 = -70.0;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileScan {
    pub frames: u64,
    pub seconds: f64,
    /// Peak magnitude per bucket, in time order, 0…1.
    pub envelope: Vec<f32>,
    /// LUFS over time, in time order, floored at −70.
    pub loudness: Vec<f32>,
    /// Columns the spectrogram actually produced, which a short file makes fewer.
    pub columns: usize,
    pub bands: usize,
    /// `columns × bands` values, column by column, each 0…1.
    pub spectrum: Vec<f32>,
}

/// The edge frequencies of the log bands, low to high. One list for the scan and
/// for anything that names a band, so a label cannot disagree with a picture.
fn band_edges() -> Vec<(f32, f32)> {
    let ratio = MAX_HZ / MIN_HZ;
    (0..BANDS)
        .map(|index| {
            let low = MIN_HZ * ratio.powf(index as f32 / BANDS as f32);
            let high = MIN_HZ * ratio.powf((index + 1) as f32 / BANDS as f32);
            (low, high)
        })
        .collect()
}

/// Peak envelope: one bucket at a time, and a merge if the length was a guess.
struct Envelope {
    peaks: Vec<f32>,
    bucket: u64,
    within: u64,
    current: f32,
}

impl Envelope {
    fn new(total: u64) -> Self {
        // A known length gets exactly the points the picture takes; an unknown one
        // starts at 10 ms buckets and is merged down at the end.
        let bucket = if total >= ENVELOPE_POINTS as u64 {
            (total / ENVELOPE_POINTS as u64).max(1)
        } else {
            480
        };
        Self {
            peaks: Vec::with_capacity(ENVELOPE_POINTS),
            bucket,
            within: 0,
            current: 0.0,
        }
    }

    fn push(&mut self, magnitude: f32) {
        self.current = self.current.max(magnitude.min(1.0));
        self.within += 1;
        if self.within == self.bucket {
            self.peaks.push(self.current);
            self.current = 0.0;
            self.within = 0;
        }
    }

    fn full(&self) -> bool {
        self.peaks.len() >= ENVELOPE_POINTS
    }

    fn finish(mut self) -> Vec<f32> {
        if self.within > 0 {
            self.peaks.push(self.current);
        }
        // Half the points at a time, keeping the loudest of each pair, until the
        // vector is the size the picture asks for.
        while self.peaks.len() > ENVELOPE_POINTS {
            let mut merged = Vec::with_capacity(self.peaks.len().div_ceil(2));
            for pair in self.peaks.chunks(2) {
                merged.push(pair.iter().copied().fold(0.0f32, f32::max));
            }
            self.peaks = merged;
        }
        self.peaks
    }
}

/// The spectrogram: a ring of the last window's samples, a column captured each
/// time the file reaches that column's window.
struct Spectrum {
    total: u64,
    ring: Vec<f32>,
    head: usize,
    bins: Vec<(usize, usize)>,
    window: Vec<f32>,
    values: Vec<f32>,
    produced: usize,
}

impl Spectrum {
    fn new(total: u64) -> Self {
        let mut window = Vec::with_capacity(FFT);
        for index in 0..FFT {
            window.push(0.5 - 0.5 * (2.0 * std::f32::consts::PI * index as f32 / (FFT - 1) as f32).cos());
        }
        let bins = band_edges()
            .into_iter()
            .map(|(low, high)| {
                let first = ((low as f64 * FFT as f64 / RATE).floor() as usize).max(1);
                let last = ((high as f64 * FFT as f64 / RATE).ceil() as usize)
                    .max(first + 1)
                    .min(FFT / 2);
                (first.min(FFT / 2 - 1), last)
            })
            .collect();
        Self {
            total,
            ring: vec![0.0; FFT],
            head: 0,
            bins,
            window,
            values: Vec::with_capacity(COLUMNS * BANDS),
            produced: 0,
        }
    }

    fn full(&self) -> bool {
        self.produced >= COLUMNS
    }

    /// One mono sample. A column is read when the file has reached the end of its
    /// window; a file too short to fill one produces none rather than a fiction.
    fn push(&mut self, mono: f32, frames: u64) {
        self.ring[self.head] = if mono.is_finite() { mono } else { 0.0 };
        self.head = (self.head + 1) % FFT;
        if self.total == 0 {
            return;
        }
        while self.produced < COLUMNS {
            let start = self.produced as u64 * self.total / COLUMNS as u64;
            if frames < start + FFT as u64 {
                break;
            }
            self.capture();
        }
    }

    fn capture(&mut self) {
        let mut re = vec![0.0f32; FFT];
        let mut im = vec![0.0f32; FFT];
        for (index, slot) in re.iter_mut().enumerate() {
            *slot = self.ring[(self.head + index) % FFT] * self.window[index];
        }
        fft_in_place(&mut re, &mut im);
        for (first, last) in &self.bins {
            let mut loudest = 0.0f32;
            for bin in *first..*last {
                let magnitude = (re[bin] * re[bin] + im[bin] * im[bin]).sqrt() * 2.0 / FFT as f32;
                loudest = loudest.max(magnitude);
            }
            let db = if loudest > 1e-9 {
                20.0 * loudest.log10()
            } else {
                FLOOR_DB
            };
            self.values
                .push(((db.clamp(FLOOR_DB, 0.0) - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0));
        }
        self.produced += 1;
    }
}

/// The loudness curve, decimated to the number of points the picture has room for.
fn curve(powers: &[f64]) -> Vec<f32> {
    if powers.is_empty() {
        return Vec::new();
    }
    let step = (powers.len() as f64 / LOUDNESS_POINTS as f64).max(1.0);
    let mut values = Vec::with_capacity(LOUDNESS_POINTS.min(powers.len()));
    let mut at = 0.0f64;
    while values.len() < LOUDNESS_POINTS && (at as usize) < powers.len() {
        values.push(Loudness::lufs(powers[at as usize]).max(LOUDNESS_FLOOR));
        at += step;
    }
    values
}

/// Scans one file. A file that cannot be decoded is an error; a file that is short
/// or silent is not, it simply draws less.
pub fn scan(path: &Path) -> Result<FileScan, String> {
    let total = player::describe(path)
        .map(|(_, seconds)| (seconds * RATE) as u64)
        .unwrap_or(0);
    let mut meter = Loudness::new();
    let mut envelope = Envelope::new(total);
    let mut spectrum = Spectrum::new(total);
    let mut frames: u64 = 0;
    player::decode(path, &mut |block| {
        for frame in block {
            let left = if frame[0].is_finite() { frame[0] } else { 0.0 };
            let right = if frame[1].is_finite() { frame[1] } else { 0.0 };
            frames += 1;
            envelope.push(left.abs().max(right.abs()));
            meter.push([left, right]);
            spectrum.push((left + right) * 0.5, frames);
            // Every picture this scan promises is complete: the rest of the file
            // would be decoded for nothing.
            if envelope.full() && spectrum.full() {
                return false;
            }
        }
        true
    })?;
    let columns = spectrum.produced;
    Ok(FileScan {
        frames,
        seconds: frames as f64 / RATE,
        envelope: envelope.finish(),
        // The 3 s window is the standard's, but a file shorter than one window still
        // has a shape: its 400 ms blocks are the finest curve the meter has.
        loudness: curve(if meter.short_powers().is_empty() {
            meter.block_powers()
        } else {
            meter.short_powers()
        }),
        columns,
        bands: BANDS,
        spectrum: spectrum.values,
    })
}

/// The whole-file view's one command: the envelope, the loudness curve and the
/// spectrogram of a file the page has chosen, read on the engine's own decoder so a
/// file the app cannot play cannot be drawn either.
#[tauri::command]
pub fn scan_file(path: String) -> Result<FileScan, String> {
    let file = std::path::Path::new(&path);
    if !file.is_file() {
        return Err("That file is not there any more".into());
    }
    scan(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{BitDepth, Tags, WavWriter};
    use std::f32::consts::PI;

    fn scratch(name: &str) -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("audiorium-filescan-{name}-{stamp}.wav"))
    }

    fn write_tone(path: &std::path::Path, frames: usize, amplitude: f32, hertz: f32) {
        let samples: Vec<[f32; 2]> = (0..frames)
            .map(|index| {
                let value = (2.0 * PI * hertz * index as f32 / RATE as f32).sin() * amplitude;
                [value, value]
            })
            .collect();
        let mut writer = WavWriter::create(path, BitDepth::TwentyFour, &Tags::default())
            .expect("a wave writer");
        writer.write(&samples).expect("the samples");
        writer.finish().expect("the header");
    }

    #[test]
    fn a_tone_is_drawn_where_it_is() {
        let path = scratch("tone");
        write_tone(&path, 144_000, 0.5, 1_000.0);
        let scan = scan(&path).expect("a scanned tone");
        let _ = std::fs::remove_file(&path);

        assert!((scan.frames as f64 - 144_000.0).abs() < 14_400.0, "frames {}", scan.frames);
        assert!((scan.seconds - 3.0).abs() < 0.3, "seconds {}", scan.seconds);
        let peak = scan.envelope.iter().copied().fold(0.0f32, f32::max);
        assert!((peak - 0.5).abs() < 0.05, "envelope peak {peak}");
        assert!(!scan.loudness.is_empty(), "no loudness curve");
        let loudest = scan.loudness.iter().copied().fold(f32::MIN, f32::max);
        assert!(loudest > -20.0, "loudness peak {loudest} LUFS");
        assert_eq!(scan.bands, BANDS);
        assert!(scan.columns > 0, "no spectrogram columns");
        assert_eq!(scan.spectrum.len(), scan.columns * scan.bands);
        assert!(scan.spectrum.iter().all(|value| value.is_finite() && (0.0..=1.0).contains(value)));

        // The strongest band of the middle column is the tone, not a harmonic or
        // the noise floor: a 1 kHz sine has to land between 500 Hz and 2 kHz.
        let middle = scan.columns / 2;
        let base = middle * scan.bands;
        let strongest = (0..BANDS)
            .max_by(|a, b| {
                scan.spectrum[base + a]
                    .partial_cmp(&scan.spectrum[base + b])
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap();
        let (low, high) = band_edges()[strongest];
        assert!(high >= 500.0 && low <= 2_000.0, "strongest band {low}–{high} Hz");
    }

    /// A float file, so the dither a 24-bit master carries cannot be mistaken for
    /// signal: this test is about what the scan does with real silence.
    fn write_silence(path: &std::path::Path, frames: usize) {
        let samples: Vec<[f32; 2]> = vec![[0.0, 0.0]; frames];
        let mut writer = WavWriter::create(path, BitDepth::Float32, &Tags::default())
            .expect("a wave writer");
        writer.write(&samples).expect("the samples");
        writer.finish().expect("the header");
    }

    #[test]
    fn silence_is_drawn_without_inventing_a_reading() {
        let path = scratch("silence");
        write_silence(&path, 96_000);
        let scan = scan(&path).expect("a silent file scans");
        let _ = std::fs::remove_file(&path);

        assert!(scan.frames > 0);
        assert!(scan.envelope.iter().all(|value| value.is_finite() && *value == 0.0));
        assert!(scan.loudness.iter().all(|value| value.is_finite() && *value <= -69.0));
        assert!(scan.spectrum.iter().all(|value| value.is_finite() && *value == 0.0));
    }

    #[test]
    fn a_missing_file_is_an_error_rather_than_an_empty_picture() {
        assert!(scan(std::path::Path::new("Z:/definitely/not/here.wav")).is_err());
    }
}
