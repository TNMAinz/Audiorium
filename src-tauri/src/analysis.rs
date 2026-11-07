//! What the app is measuring, measured.
//!
//! Two engines publish here and only one does at a time: library playback, whose
//! frames pass through the chain, and the input capture, which reads the chosen
//! endpoint. This module keeps the figures the interface shows — a real FFT folded
//! into 56 logarithmic bands, RMS, sample peak and three energy bands — and names
//! the source they came from. There is no second tap and no synthesised waveform:
//! an idle engine reports idle, and silence reads as the floor.
//!
//! For playback the measurement sits after the panner, room, equaliser, limiter and
//! volume, so it describes what you hear, including the work the processors did. For
//! capture it describes the endpoint's own signal, untouched.

use serde::Serialize;
use std::sync::Mutex;

const RATE: f32 = 48_000.0;
const FFT_SIZE: usize = 2048;
const SPECTRUM_BANDS: usize = 56;
const SPECTRUM_MIN_HZ: f32 = 30.0;
const SPECTRUM_MAX_HZ: f32 = 16_000.0;
const SPECTRUM_FLOOR_DB: f32 = -84.0;
/// The one stream every stage in the app runs at.
pub const STREAM_FORMAT: &str = "48 kHz \u{00b7} 32-bit float \u{00b7} stereo";
/// Every fourth frame is kept for the goniometer, and one window publishes at most
/// 256 points: a 16 ms window is 192 points, and a stalled publish cannot grow it.
const VECTOR_STEP: u32 = 4;
const VECTOR_POINTS: usize = 256;

/// Who is measuring. The player publishes what it renders; the input capture
/// publishes what the chosen endpoint carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Playback,
    Input,
}
impl Source {
    /// The name the interface reads.
    pub fn label(self) -> &'static str {
        match self {
            Source::Playback => "playback",
            Source::Input => "input",
        }
    }
}

/// One published set of readings. `playing` is the engine's own answer, not a
/// guess from a nonzero level: it is true only while frames are being measured.
/// `paused` says the player is loaded and waiting, so the last window stands and
/// the panels freeze it; a stop is neither, and lets the panels fade out.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Readings {
    pub playing: bool,
    /// `playback` or `input`: what these numbers are measuring.
    pub source: String,
    /// The player is loaded but paused: these are the last measured values, held.
    pub paused: bool,
    pub format: String,
    pub rms: f32,
    pub peak: f32,
    pub low: f32,
    pub mid: f32,
    pub high: f32,
    /// Correlation between the two output channels over the window: `+1` the same
    /// signal in both ears, `0` unrelated, `-1` one channel inverted.
    pub correlation: f32,
    /// Right minus left of the channel RMS, normalised: `0` centred, `±1` one side only.
    pub balance: f32,
    /// The window's own samples as `mid, side` pairs, for a goniometer. At most
    /// `VECTOR_POINTS` points, so the display keeps the shape of the window without
    /// moving a stream of audio through the interface.
    pub vectorscope: Vec<f32>,
    pub spectrum: Vec<f32>,
}
impl Default for Readings {
    fn default() -> Self {
        Self {
            playing: false,
            source: Source::Playback.label().to_string(),
            paused: false,
            format: STREAM_FORMAT.to_string(),
            rms: 0.0,
            peak: 0.0,
            low: 0.0,
            mid: 0.0,
            high: 0.0,
            correlation: 0.0,
            balance: 0.0,
            vectorscope: Vec::new(),
            spectrum: Vec::new(),
        }
    }
}

/// Shared with the engines, which write, and the interface, which polls.
///
/// The owner decides which engine's numbers the panels show. Both can run at
/// once — a track playing while the input is captured — and one set of readings
/// cannot hold both without alternating, so the source that is publishing is
/// named here and the other one's publishes are ignored.
pub struct AnalysisState {
    readings: Mutex<Readings>,
    owner: Mutex<Option<Source>>,
}
impl Default for AnalysisState {
    fn default() -> Self {
        Self {
            readings: Mutex::new(Readings::default()),
            owner: Mutex::new(None),
        }
    }
}
impl AnalysisState {
    pub fn read(&self) -> Readings {
        match self.readings.lock() {
            Ok(readings) => readings.clone(),
            Err(_) => Readings::default(),
        }
    }
    /// The engine that is starting asks for the panels. Capture wins a contest:
    /// starting it is a deliberate request to see that device, and it keeps the
    /// panels until it is stopped. Playback starting under a running capture is
    /// heard but not measured there.
    pub fn claim(&self, source: Source) -> bool {
        let Ok(mut owner) = self.owner.lock() else {
            return false;
        };
        if *owner == Some(source) {
            return true;
        }
        if source == Source::Playback && *owner == Some(Source::Input) {
            return false;
        }
        *owner = Some(source);
        true
    }
    pub fn release(&self, source: Source) {
        if let Ok(mut owner) = self.owner.lock() {
            if *owner == Some(source) {
                *owner = None;
            }
        }
    }
    pub fn owns(&self, source: Source) -> bool {
        self.owner.lock().map(|owner| *owner == Some(source)).unwrap_or(false)
    }
    pub fn publish(&self, readings: Readings) {
        if let Ok(mut current) = self.readings.lock() {
            *current = readings;
        }
    }
    /// A pause is not a stop: nothing new was measured, so the last window is
    /// kept and flagged paused. The panels freeze it, and resuming carries on
    /// from the same reading instead of from silence.
    pub fn hold(&self, source: Source) {
        if !self.owns(source) {
            return;
        }
        if let Ok(mut current) = self.readings.lock() {
            current.playing = false;
            current.paused = true;
        }
    }
    /// Back to idle: called when an engine stops so the display cannot keep a
    /// level that is no longer being measured.
    pub fn idle(&self, source: Source) {
        if !self.owns(source) {
            return;
        }
        self.release(source);
        self.publish(Readings::default());
    }
}

pub(crate) fn fft_in_place(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    debug_assert!(n.is_power_of_two());
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -2.0 * std::f32::consts::PI / len as f32;
        let (tw_re, tw_im) = (angle.cos(), angle.sin());
        let half = len / 2;
        for start in (0..n).step_by(len) {
            let (mut cur_re, mut cur_im) = (1.0f32, 0.0f32);
            for k in 0..half {
                let a = start + k;
                let b = a + half;
                let vr = re[b] * cur_re - im[b] * cur_im;
                let vi = re[b] * cur_im + im[b] * cur_re;
                re[b] = re[a] - vr;
                im[b] = im[a] - vi;
                re[a] += vr;
                im[a] += vi;
                let next_re = cur_re * tw_re - cur_im * tw_im;
                cur_im = cur_re * tw_im + cur_im * tw_re;
                cur_re = next_re;
            }
        }
        len <<= 1;
    }
}

fn hann(index: usize) -> f32 {
    0.5 - 0.5 * (2.0 * std::f32::consts::PI * index as f32 / (FFT_SIZE - 1) as f32).cos()
}

/// Real magnitude spectrum: a Hann-windowed FFT folded into logarithmic bands.
/// Values are 0..1 on a fixed -84 dBFS floor, so they are measurements, not decoration.
fn spectrum_bands(samples: &[f32]) -> Vec<f32> {
    let mut re = [0.0f32; FFT_SIZE];
    let mut im = [0.0f32; FFT_SIZE];
    for (i, slot) in re.iter_mut().enumerate() {
        *slot = samples.get(i).copied().unwrap_or(0.0) * hann(i);
    }
    fft_in_place(&mut re, &mut im);
    let amplitude_scale = 4.0 / FFT_SIZE as f32;
    let bin_hz = RATE / FFT_SIZE as f32;
    let ratio = SPECTRUM_MAX_HZ / SPECTRUM_MIN_HZ;
    let mut bands = Vec::with_capacity(SPECTRUM_BANDS);
    for band in 0..SPECTRUM_BANDS {
        let low_hz = SPECTRUM_MIN_HZ * ratio.powf(band as f32 / SPECTRUM_BANDS as f32);
        let high_hz = SPECTRUM_MIN_HZ * ratio.powf((band + 1) as f32 / SPECTRUM_BANDS as f32);
        let low = ((low_hz / bin_hz).floor() as usize).clamp(1, FFT_SIZE / 2 - 1);
        let high = ((high_hz / bin_hz).ceil() as usize).clamp(low + 1, FFT_SIZE / 2);
        let mut strongest = 0.0f32;
        for bin in low..high {
            let magnitude = (re[bin] * re[bin] + im[bin] * im[bin]).sqrt() * amplitude_scale;
            strongest = strongest.max(magnitude);
        }
        let decibels = 20.0 * strongest.max(1e-7).log10();
        bands.push(((decibels - SPECTRUM_FLOOR_DB) / -SPECTRUM_FLOOR_DB).clamp(0.0, 1.0));
    }
    bands
}

/// Accumulates one publication window of output frames. The playback thread hands
/// every frame it wrote to the device here; `readings` closes the window.
pub struct Meter {
    low: f32,
    high_cut: f32,
    sums: [f64; 4],
    /// Per-channel energy and their cross term: left², right², left × right.
    phase: [f64; 3],
    /// Mid/side pairs kept for this window's goniometer.
    vectorscope: Vec<f32>,
    vector_step: u32,
    count: usize,
    peak: f32,
    ring: Vec<f32>,
    ring_pos: usize,
}
impl Default for Meter {
    fn default() -> Self {
        Self::new()
    }
}
impl Meter {
    pub fn new() -> Self {
        Self {
            low: 0.0,
            high_cut: 0.0,
            sums: [0.0; 4],
            phase: [0.0; 3],
            vectorscope: Vec::new(),
            vector_step: 0,
            count: 0,
            peak: 0.0,
            ring: vec![0.0; FFT_SIZE],
            ring_pos: 0,
        }
    }

    /// One stereo frame, exactly as it was handed to the output.
    pub fn push(&mut self, frame: [f32; 2]) {
        let mono = (frame[0] + frame[1]) * 0.5;
        self.ring[self.ring_pos] = mono;
        self.ring_pos = (self.ring_pos + 1) & (FFT_SIZE - 1);
        self.peak = self.peak.max(frame[0].abs().max(frame[1].abs()));
        let (left, right) = (frame[0] as f64, frame[1] as f64);
        self.phase[0] += left * left;
        self.phase[1] += right * right;
        self.phase[2] += left * right;
        self.vector_step += 1;
        if self.vector_step >= VECTOR_STEP && self.vectorscope.len() < VECTOR_POINTS * 2 {
            self.vector_step = 0;
            self.vectorscope.push((frame[0] + frame[1]) * 0.5);
            self.vectorscope.push((frame[0] - frame[1]) * 0.5);
        }
        self.low += 0.016 * (mono - self.low);
        self.high_cut += 0.23 * (mono - self.high_cut);
        for (i, value) in [mono, self.low, self.high_cut - self.low, mono - self.high_cut]
            .iter()
            .enumerate()
        {
            self.sums[i] += (*value as f64).powi(2);
        }
        self.count += 1;
    }

    /// Publish the window and start the next one. The ring is read in the order
    /// the samples were played, so the FFT never sees the wrap as a discontinuity.
    pub fn readings(&mut self) -> Readings {
        let level = |i: usize| {
            if self.count == 0 {
                0.0
            } else {
                (self.sums[i] / self.count as f64).sqrt() as f32
            }
        };
        let ordered: Vec<f32> = (0..FFT_SIZE)
            .map(|i| self.ring[(self.ring_pos + i) & (FFT_SIZE - 1)])
            .collect();
        // A window with no frames is silence, not the last sound again: the ring
        // still holds the previous window, and a source that has gone quiet (a
        // loopback with nothing playing) must read as the floor rather than keep
        // drawing a shape it is no longer measuring.
        let spectrum = if self.count == 0 {
            vec![0.0; SPECTRUM_BANDS]
        } else {
            spectrum_bands(&ordered)
        };
        // Phase and balance come from the same window as the level: the correlation is
        // the cross term over the product of the channel energies, which is the cosine
        // of what the two channels have in common. Silence has neither.
        let (left_energy, right_energy, cross) = (self.phase[0], self.phase[1], self.phase[2]);
        let product = (left_energy * right_energy).sqrt();
        let correlation = if product > 1e-12 {
            (cross / product).clamp(-1.0, 1.0) as f32
        } else {
            0.0
        };
        let (left_rms, right_rms) = (left_energy.sqrt(), right_energy.sqrt());
        let balance = if left_rms + right_rms > 1e-9 {
            (((right_rms - left_rms) / (right_rms + left_rms)) as f32).clamp(-1.0, 1.0)
        } else {
            0.0
        };
        let readings = Readings {
            playing: true,
            // The meter is source-agnostic; the engine that runs it names itself.
            source: Source::Playback.label().to_string(),
            paused: false,
            format: STREAM_FORMAT.to_string(),
            rms: level(0),
            peak: self.peak,
            low: level(1),
            mid: level(2),
            high: level(3),
            correlation,
            balance,
            // The cloud belongs to the window it was measured in, so it is handed over
            // rather than shared, and the next window starts empty.
            vectorscope: std::mem::take(&mut self.vectorscope),
            spectrum,
        };
        self.sums = [0.0; 4];
        self.phase = [0.0; 3];
        self.vector_step = 0;
        self.count = 0;
        self.peak = 0.0;
        readings
    }
}

/// Meter ballistics: how the readings are allowed to move between publishes.
///
/// A 25 ms window is an honest measurement and a jittery display: two windows of
/// the same track differ by a decibel or two, so at 60 publishes a second the
/// curve and the level readout twitch. A real meter solved this with a movement
/// that rises quickly and falls slowly, and that is all this is: fast attack so a
/// transient still shows at once, slow release so the eye can follow. Nothing is
/// invented — the value settled on is the value the meter measured.
pub struct Ballistics {
    spectrum: Vec<f32>,
    rms: f32,
    peak: f32,
    low: f32,
    mid: f32,
    high: f32,
    correlation: f32,
    balance: f32,
}

/// Seconds to cover most of a change: the spectrum snaps, the level leans, the
/// peak holds and then falls away.
const SPECTRUM_ATTACK: f32 = 0.015;
const SPECTRUM_RELEASE: f32 = 0.18;
const LEVEL_ATTACK: f32 = 0.04;
const LEVEL_RELEASE: f32 = 0.25;
const PEAK_RELEASE: f32 = 1.2;
/// Phase and balance move with the material, not with one window: they lean rather
/// than snap, so a cymbal cannot swing the needle for a frame.
const PHASE_ATTACK: f32 = 0.12;
const PHASE_RELEASE: f32 = 0.32;

/// One step of a meter movement: exponential, with a shorter time constant rising
/// than falling. Clamped times keep a stalled thread from jumping the display.
fn follow(current: f32, target: f32, dt: f32, attack: f32, release: f32) -> f32 {
    let tau = if target >= current { attack } else { release };
    current + (target - current) * (1.0 - (-dt / tau).exp())
}

impl Default for Ballistics {
    fn default() -> Self {
        Self::new()
    }
}
impl Ballistics {
    pub fn new() -> Self {
        Self {
            spectrum: Vec::new(),
            rms: 0.0,
            peak: 0.0,
            low: 0.0,
            mid: 0.0,
            high: 0.0,
            correlation: 0.0,
            balance: 0.0,
        }
    }

    /// Move the last display towards this measurement and return what to show.
    /// `dt` is the time since the previous publish. An idle engine is not smoothed:
    /// it is idle, and the residual has to go with the sound that made it.
    pub fn apply(&mut self, readings: Readings, dt: f32) -> Readings {
        if !readings.playing {
            *self = Self::new();
            return readings;
        }
        let dt = dt.clamp(0.001, 0.25);
        let spectrum = readings
            .spectrum
            .iter()
            .enumerate()
            .map(|(index, target)| {
                // A band with no history starts at nothing, so the curve fades in
                // over the first few frames instead of appearing whole.
                let current = self.spectrum.get(index).copied().unwrap_or(0.0);
                follow(current, *target, dt, SPECTRUM_ATTACK, SPECTRUM_RELEASE)
            })
            .collect::<Vec<f32>>();
        self.spectrum = spectrum.clone();
        self.rms = follow(self.rms, readings.rms, dt, LEVEL_ATTACK, LEVEL_RELEASE);
        self.low = follow(self.low, readings.low, dt, LEVEL_ATTACK, LEVEL_RELEASE);
        self.mid = follow(self.mid, readings.mid, dt, LEVEL_ATTACK, LEVEL_RELEASE);
        self.high = follow(self.high, readings.high, dt, LEVEL_ATTACK, LEVEL_RELEASE);
        // A peak has no attack at all: the transient is the point. It is the fall
        // that is slowed, so one sample over the line stays readable.
        self.peak = follow(self.peak, readings.peak, dt, 0.001, PEAK_RELEASE);
        self.correlation = follow(
            self.correlation,
            readings.correlation,
            dt,
            PHASE_ATTACK,
            PHASE_RELEASE,
        );
        self.balance = follow(self.balance, readings.balance, dt, PHASE_ATTACK, PHASE_RELEASE);
        Readings {
            rms: self.rms,
            peak: self.peak,
            low: self.low,
            mid: self.mid,
            high: self.high,
            correlation: self.correlation,
            balance: self.balance,
            spectrum,
            ..readings
        }
    }
}

#[tauri::command]
pub fn analysis_status(
    state: tauri::State<'_, std::sync::Arc<AnalysisState>>,
) -> Readings {
    state.read()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frequency: f32, amplitude: f32) -> Vec<f32> {
        (0..FFT_SIZE)
            .map(|i| amplitude * (2.0 * std::f32::consts::PI * frequency * i as f32 / RATE).sin())
            .collect()
    }
    fn band_center(band: usize) -> f32 {
        let ratio = SPECTRUM_MAX_HZ / SPECTRUM_MIN_HZ;
        SPECTRUM_MIN_HZ * ratio.powf((band as f32 + 0.5) / SPECTRUM_BANDS as f32)
    }
    /// A measurement as the meter would have closed it. The three energy bands sit
    /// under the same level the way a musical signal's would, at fixed fractions of
    /// it, so a test can tell them apart.
    fn readings_of(rms: f32, peak: f32, spectrum: Vec<f32>) -> Readings {
        Readings {
            playing: true,
            source: Source::Playback.label().to_string(),
            paused: false,
            format: STREAM_FORMAT.to_string(),
            rms,
            peak,
            low: rms * 0.6,
            mid: rms,
            high: rms * 0.3,
            correlation: 1.0,
            balance: 0.0,
            vectorscope: Vec::new(),
            spectrum,
        }
    }
    /// A meter fed a stereo frame list, as the output thread would.
    fn meter_of(samples: &[f32]) -> Meter {
        let mut meter = Meter::new();
        for sample in samples {
            meter.push([*sample, *sample]);
        }
        meter
    }

    #[test]
    fn spectrum_places_a_tone_in_the_matching_band() {
        let bands = spectrum_bands(&tone(1_000.0, 0.5));
        assert_eq!(bands.len(), SPECTRUM_BANDS);
        let peak = bands
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(index, _)| index)
            .unwrap();
        let center = band_center(peak);
        assert!(
            (center - 1_000.0).abs() / 1_000.0 < 0.2,
            "peak band {peak} centres at {center} Hz"
        );
        // Neighbouring bands legitimately hold part of the main lobe; distant bands must stay quiet.
        assert!(bands[0] < 0.5, "unexpected low-frequency energy");
        assert!(bands[SPECTRUM_BANDS - 1] < 0.5, "unexpected high-frequency energy");
        assert!(bands[peak] > 0.85);
    }

    #[test]
    fn silence_stays_at_the_floor_and_full_scale_is_near_the_top() {
        assert!(spectrum_bands(&[0.0; FFT_SIZE])
            .iter()
            .all(|value| *value <= 0.001));
        let loud = spectrum_bands(&tone(440.0, 1.0));
        assert!(loud.iter().cloned().fold(0.0f32, f32::max) > 0.95);
    }

    #[test]
    fn band_centres_increase_monotonically() {
        for band in 1..SPECTRUM_BANDS {
            assert!(band_center(band) > band_center(band - 1));
        }
    }

    #[test]
    fn the_meter_reports_the_level_of_the_frames_it_was_given() {
        // A full-scale 1 kHz sine has an RMS of 1/sqrt(2) and a peak of 1.0.
        let mut meter = meter_of(&tone(1_000.0, 1.0));
        let readings = meter.readings();
        assert!(readings.playing);
        assert!(
            (readings.rms - 0.7071).abs() < 0.02,
            "rms {}",
            readings.rms
        );
        assert!((readings.peak - 1.0).abs() < 1e-6, "peak {}", readings.peak);
        // The tone lands in the band its frequency belongs to.
        let loudest = readings
            .spectrum
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(index, _)| index)
            .unwrap();
        assert!((band_center(loudest) - 1_000.0).abs() / 1_000.0 < 0.2);
    }

    #[test]
    fn the_meter_starts_a_new_window_and_forgets_the_last_one() {
        let mut meter = meter_of(&tone(1_000.0, 1.0));
        let first = meter.readings();
        assert!(first.rms > 0.5);
        // Silence after a loud window is silence, not a held peak.
        for _ in 0..64 {
            meter.push([0.0, 0.0]);
        }
        let second = meter.readings();
        assert_eq!(second.peak, 0.0);
        assert!(second.rms < 0.01, "rms {}", second.rms);
    }

    #[test]
    fn a_pause_holds_the_last_window_and_a_stop_gives_it_up() {
        let state = AnalysisState::default();
        assert!(state.claim(Source::Playback));
        let mut meter = meter_of(&tone(440.0, 0.5));
        state.publish(meter.readings());
        let before = state.read();
        assert!(before.playing && !before.paused);
        state.hold(Source::Playback);
        let held = state.read();
        assert!(!held.playing, "a paused player still reports playing");
        assert!(held.paused, "a pause is not reported as a pause");
        assert_eq!(held.rms, before.rms, "the held level moved");
        assert_eq!(held.peak, before.peak, "the held peak moved");
        assert_eq!(held.spectrum, before.spectrum, "the held curve moved");
        assert_eq!(held.vectorscope, before.vectorscope, "the held trace moved");
        // A stop is not a pause: nothing is held, so the panels can fade it out.
        state.idle(Source::Playback);
        let stopped = state.read();
        assert!(!stopped.playing && !stopped.paused);
        assert_eq!(stopped.rms, 0.0);
        assert!(stopped.spectrum.is_empty());
    }

    #[test]
    fn idle_reads_as_idle_rather_than_a_frozen_level() {
        let state = AnalysisState::default();
        assert!(state.claim(Source::Playback));
        assert!(!state.read().playing);
        assert!(state.read().spectrum.is_empty());
        let mut meter = meter_of(&tone(440.0, 0.5));
        state.publish(meter.readings());
        assert!(state.read().playing);
        state.idle(Source::Playback);
        let idle = state.read();
        assert!(!idle.playing);
        assert_eq!(idle.peak, 0.0);
        assert!(idle.spectrum.is_empty());
    }

    #[test]
    fn a_window_with_no_frames_reads_as_silence_not_as_the_previous_sound() {
        let mut meter = meter_of(&tone(440.0, 0.5));
        let loud = meter.readings();
        assert!(loud.spectrum.iter().cloned().fold(0.0f32, f32::max) > 0.5);
        // Nothing arrived for the next window: a quiet loopback, not a held curve.
        let quiet = meter.readings();
        assert_eq!(quiet.rms, 0.0);
        assert_eq!(quiet.peak, 0.0);
        assert!(quiet.spectrum.iter().all(|band| *band == 0.0), "the old curve was redrawn");
    }

    #[test]
    fn the_input_keeps_the_panels_and_playback_steps_out_of_them() {
        let state = AnalysisState::default();
        assert!(state.claim(Source::Playback));
        // Starting the capture is a request to see that device: it takes the panels.
        assert!(state.claim(Source::Input));
        assert!(!state.claim(Source::Playback), "playback displaced a running capture");
        let mut meter = meter_of(&tone(440.0, 0.5));
        state.publish(Readings {
            source: Source::Input.label().to_string(),
            ..meter.readings()
        });
        assert_eq!(state.read().source, "input");
        // A player pausing or stopping under a running capture is heard, not shown:
        // it must not freeze or clear the input's readings.
        state.hold(Source::Playback);
        assert!(state.read().playing, "a paused player froze the input");
        state.idle(Source::Playback);
        assert!(state.read().playing, "a stopped player cleared the input");
        // Stopping the capture gives the panels up, and playback can have them.
        state.idle(Source::Input);
        assert!(!state.read().playing);
        assert!(state.claim(Source::Playback));
    }

    #[test]
    fn the_display_rises_fast_and_falls_slow() {
        let mut ballistics = Ballistics::new();
        let step = readings_of(0.5, 0.5, vec![0.5; 4]);
        // One 16 ms publish covers about a third of the 40 ms level attack, and a
        // fifth of the 15 ms spectrum attack.
        let rising = ballistics.apply(step, 0.016);
        assert!(
            rising.rms > 0.12 && rising.rms < 0.24,
            "level attack did not move to the measurement: {}",
            rising.rms
        );
        assert!(
            rising.spectrum[0] > 0.25 && rising.spectrum[0] < 0.40,
            "spectrum attack did not move to the measurement: {}",
            rising.spectrum[0]
        );
        // Falling is not the same step: the same interval gives back only a little.
        let falling = ballistics.apply(readings_of(0.0, 0.0, vec![0.0; 4]), 0.016);
        assert!(
            falling.rms > rising.rms * 0.9,
            "the level fell as fast as it rose: {} then {}",
            rising.rms,
            falling.rms
        );
        assert!(
            falling.peak > rising.peak * 0.9,
            "the peak did not hold: {} then {}",
            rising.peak,
            falling.peak
        );
    }

    #[test]
    fn the_display_settles_on_the_value_the_meter_measured() {
        let mut ballistics = Ballistics::new();
        let measured = readings_of(0.31, 0.62, vec![0.4, 0.2, 0.9, 0.1]);
        let mut shown = measured.clone();
        for _ in 0..200 {
            shown = ballistics.apply(measured.clone(), 0.016);
        }
        assert!((shown.rms - 0.31).abs() < 1e-3, "rms {}", shown.rms);
        assert!((shown.peak - 0.62).abs() < 1e-3, "peak {}", shown.peak);
        assert!((shown.low - 0.186).abs() < 1e-3, "low {}", shown.low);
        assert!((shown.mid - 0.31).abs() < 1e-3, "mid {}", shown.mid);
        assert!((shown.high - 0.093).abs() < 1e-3, "high {}", shown.high);
        for (index, target) in [0.4, 0.2, 0.9, 0.1].iter().enumerate() {
            assert!(
                (shown.spectrum[index] - target).abs() < 2e-3,
                "band {index} settled on {}",
                shown.spectrum[index]
            );
        }
        assert_eq!(shown.spectrum.len(), 4, "smoothing changed the band count");
    }

    #[test]
    fn stopping_clears_the_display_without_dragging_the_last_level() {
        let mut ballistics = Ballistics::new();
        for _ in 0..20 {
            ballistics.apply(readings_of(0.8, 0.9, vec![0.7; 8]), 0.016);
        }
        let idle = ballistics.apply(Readings::default(), 0.016);
        assert!(!idle.playing);
        assert_eq!(idle.rms, 0.0, "a stopped engine still shows a level");
        assert!(idle.spectrum.is_empty());
        // Starting again begins from nothing, not from where the old track left off.
        let restarted = ballistics.apply(readings_of(0.5, 0.5, vec![0.5; 4]), 0.016);
        assert!(
            restarted.rms < 0.25,
            "the display carried a level over the silence: {}",
            restarted.rms
        );
    }

    #[test]
    fn the_meter_reads_phase_and_balance_from_the_frames_it_was_given() {
        let tone = tone(440.0, 0.5);
        let meter_of_frames = |frame: &dyn Fn(f32) -> [f32; 2]| {
            let mut meter = Meter::new();
            for sample in &tone {
                meter.push(frame(*sample));
            }
            meter.readings()
        };
        // The same signal in both ears is mono-coherent and centred.
        let mono = meter_of_frames(&|sample| [sample, sample]);
        assert!(
            (mono.correlation - 1.0).abs() < 0.01,
            "correlation {}",
            mono.correlation
        );
        assert!(mono.balance.abs() < 0.01, "balance {}", mono.balance);
        // One channel inverted is out of phase, still centred in level.
        let inverted = meter_of_frames(&|sample| [sample, -sample]);
        assert!(
            (inverted.correlation + 1.0).abs() < 0.01,
            "correlation {}",
            inverted.correlation
        );
        assert!(inverted.balance.abs() < 0.01);
        // One channel only: no correlation to report, and the balance reaches its end.
        let left_only = meter_of_frames(&|sample| [sample, 0.0]);
        assert_eq!(left_only.correlation, 0.0);
        assert!(
            (left_only.balance + 1.0).abs() < 0.01,
            "balance {}",
            left_only.balance
        );
        // Quadrature channels share their level and nothing else.
        let mut meter = Meter::new();
        for index in 0..FFT_SIZE {
            let phase = 2.0 * std::f32::consts::PI * 440.0 * index as f32 / RATE;
            meter.push([0.5 * phase.sin(), 0.5 * phase.cos()]);
        }
        let wide = meter.readings();
        assert!(
            wide.correlation.abs() < 0.05,
            "correlation {}",
            wide.correlation
        );
        // Silence has no phase and no side, and must not print a NaN.
        let quiet = meter_of(&[0.0; 256]).readings();
        assert_eq!(quiet.correlation, 0.0);
        assert_eq!(quiet.balance, 0.0);
    }

    #[test]
    fn the_goniometer_keeps_the_shape_of_the_window_it_measured() {
        let tone = tone(440.0, 0.5);
        let cloud = |frame: &dyn Fn(f32) -> [f32; 2]| {
            let mut meter = Meter::new();
            for sample in &tone {
                meter.push(frame(*sample));
            }
            meter.readings().vectorscope
        };
        // The same signal in both ears is all mid: every point on the vertical axis.
        let mono = cloud(&|sample| [sample, sample]);
        assert!(!mono.is_empty() && mono.len() % 2 == 0, "{mono:?}");
        let side = mono.chunks(2).map(|pair| pair[1].abs()).fold(0.0f32, f32::max);
        let mid = mono.chunks(2).map(|pair| pair[0].abs()).fold(0.0f32, f32::max);
        assert!(side < 1e-6, "a mono file has side in it: {side}");
        assert!(mid > 0.1, "a mono file has no mid in it: {mid}");
        // One channel inverted is all side: every point on the horizontal axis.
        let inverted = cloud(&|sample| [sample, -sample]);
        let side = inverted.chunks(2).map(|pair| pair[1].abs()).fold(0.0f32, f32::max);
        let mid = inverted.chunks(2).map(|pair| pair[0].abs()).fold(0.0f32, f32::max);
        assert!(mid < 1e-6, "an inverted pair has mid in it: {mid}");
        assert!(side > 0.1, "an inverted pair has no side in it: {side}");
        // The cloud is bounded, and it belongs to its own window.
        assert!(mono.len() <= VECTOR_POINTS * 2);
        let mut meter = Meter::new();
        for _ in 0..(VECTOR_POINTS as u32 * VECTOR_STEP * 2) {
            meter.push([0.25, -0.25]);
        }
        assert_eq!(meter.readings().vectorscope.len(), VECTOR_POINTS * 2);
        assert!(meter.readings().vectorscope.is_empty(), "the cloud was replayed");
    }

    #[test]
    fn phase_and_balance_lean_instead_of_snapping() {
        let mut ballistics = Ballistics::new();
        let step = Readings {
            correlation: -1.0,
            balance: 0.5,
            ..readings_of(0.4, 0.5, vec![0.4; 4])
        };
        let first = ballistics.apply(step.clone(), 0.016);
        assert!(
            first.correlation < 0.0 && first.correlation > -0.4,
            "phase jumped to {}",
            first.correlation
        );
        let settled = (0..400).fold(first, |_, _| ballistics.apply(step.clone(), 0.016));
        assert!((settled.correlation + 1.0).abs() < 1e-2);
        assert!((settled.balance - 0.5).abs() < 1e-2);
    }

    #[test]
    fn the_published_format_is_the_one_stream_the_chain_runs_at() {
        assert_eq!(Readings::default().format, STREAM_FORMAT);
        assert!(STREAM_FORMAT.contains("48 kHz"));
    }
}
