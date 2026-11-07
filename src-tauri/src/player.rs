//! Library playback: the app's only sound source.
//!
//! Files are decoded in Rust and rendered through a real WASAPI output stream at
//! 48 kHz stereo float, with the spatial renderer and the processing chain between
//! the decoder and the device. Every frame written is also handed to the analysis
//! meter, so the panels measure what is leaving the app. Nothing is copied,
//! uploaded or written: the file is read where it lives and only decoded samples
//! are kept, in memory.

use serde::Serialize;
use std::collections::VecDeque;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, SeekMode, SeekTo};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;
use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

use crate::analysis::{AnalysisState, Ballistics, Meter, Source};
use crate::chain::{Chain, ChainConfig, ChainState};
use crate::spatial::{Bed, SpatialConfig, SpatialMode, BED_CHANNELS};
#[cfg(windows)]
use crate::spatial_out::SpatialSink;

/// Everything in the app renders at one rate, so playback is resampled to match.
const RATE: u32 = 48_000;
const FRAME_BYTES: usize = 8;
/// One second of decoded audio rides out decoder hiccups without adding latency
/// that a seek cannot cancel.
const RING_FRAMES: usize = RATE as usize;
const PREFILL_FRAMES: usize = RING_FRAMES / 4;

fn fail(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackStatus {
    /// An output stream is open (playing or paused).
    active: bool,
    playing: bool,
    /// The track reached its end on its own.
    finished: bool,
    title: String,
    artist: String,
    album: String,
    path: String,
    format: String,
    device: String,
    /// Seconds actually heard, not seconds decoded.
    position: f64,
    duration: f64,
    volume: f32,
    /// Which renderer is open for this track: `windows` for the spatial sink,
    /// `derived` when the Atmos mode fell back to the app's 7.1.4 renderer, and
    /// `app` for every other spatial mode.
    spatial: String,
    error: Option<String>,
}
impl Default for PlaybackStatus {
    fn default() -> Self {
        Self {
            active: false,
            playing: false,
            finished: false,
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            path: String::new(),
            format: String::new(),
            device: String::new(),
            position: 0.0,
            duration: 0.0,
            volume: 0.7,
            spatial: String::new(),
            error: None,
        }
    }
}

struct Shared {
    ring: Mutex<VecDeque<[f32; 2]>>,
    status: Mutex<PlaybackStatus>,
    stop: AtomicBool,
    paused: AtomicBool,
    seek: Mutex<Option<f64>>,
    /// Frames rendered since playback (re)started.
    played: AtomicU64,
    /// Frames before the current start, so a seek keeps an absolute position.
    base: AtomicU64,
    finished: AtomicBool,
}

pub struct PlayerState {
    inner: Mutex<Engine>,
    spatial: Arc<Mutex<SpatialConfig>>,
    chain: Arc<ChainState>,
    /// Written by the playback thread, polled by the interface.
    analysis: Arc<AnalysisState>,
    volume: Arc<AtomicU32>,
    /// A gain for comparing two files, not the user's level: a level-matched A/B
    /// has to even out two loudnesses without moving the fader under the user, and
    /// has to be able to put the comparison away without leaving the change behind.
    monitor: Arc<AtomicU32>,
    /// Whether the comparison is hearing the files themselves. A master already
    /// contains the rack, so playing it through the rack again would compare a
    /// twice-processed file with a once-processed one.
    direct: Arc<AtomicBool>,
}
struct Engine {
    thread: Option<JoinHandle<()>>,
    shared: Option<Arc<Shared>>,
}

impl PlayerState {
    pub fn new(
        spatial: Arc<Mutex<SpatialConfig>>,
        chain: Arc<ChainState>,
        analysis: Arc<AnalysisState>,
    ) -> Self {
        Self {
            inner: Mutex::new(Engine {
                thread: None,
                shared: None,
            }),
            spatial,
            chain,
            analysis,
            volume: Arc::new(AtomicU32::new(0.7f32.to_bits())),
            monitor: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            direct: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn status(&self) -> PlaybackStatus {
        let mut status = match self.inner.lock() {
            Ok(inner) => inner
                .shared
                .as_ref()
                .and_then(|shared| live_status(shared).ok())
                .unwrap_or_default(),
            Err(_) => PlaybackStatus::default(),
        };
        // The level is a setting, not a reading. It belongs to the app rather than
        // to an open stream, so it has to be reported before anything has played
        // too — otherwise every poll answers with the default and the slider,
        // which follows this value, snaps back the moment it is moved.
        status.volume = f32::from_bits(self.volume.load(Ordering::Relaxed));
        status
    }
    fn shutdown(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(fail)?;
        if let Some(shared) = inner.shared.take() {
            shared.stop.store(true, Ordering::Release);
        }
        if let Some(thread) = inner.thread.take() {
            thread
                .join()
                .map_err(|_| "Playback thread failed".to_string())?;
        }
        Ok(())
    }
}
impl Drop for PlayerState {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(shared) = inner.shared.take() {
                shared.stop.store(true, Ordering::Release);
            }
            if let Some(thread) = inner.thread.take() {
                let _ = thread.join();
            }
        }
    }
}

fn live_status(shared: &Shared) -> Result<PlaybackStatus, String> {
    let mut status = shared.status.lock().map_err(fail)?.clone();
    let played = shared.played.load(Ordering::Relaxed);
    let base = shared.base.load(Ordering::Relaxed);
    let position = (base + played) as f64 / RATE as f64;
    status.position = if status.duration > 0.0 {
        position.min(status.duration)
    } else {
        position
    };
    status.playing = status.active && !shared.paused.load(Ordering::Relaxed);
    status.finished = shared.finished.load(Ordering::Relaxed);
    Ok(status)
}

// ---------- resampling ----------

/// Second-order Butterworth low-pass, applied before reducing the rate so content
/// above the new Nyquist limit cannot fold back as aliasing.
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: [f32; 2],
    z2: [f32; 2],
}

impl Biquad {
    fn low_pass(sample_rate: f32, cutoff: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * cutoff / sample_rate;
        let cos = w0.cos();
        let alpha = w0.sin() / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 - cos) / 2.0 / a0,
            b1: (1.0 - cos) / a0,
            b2: (1.0 - cos) / 2.0 / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha) / a0,
            z1: [0.0; 2],
            z2: [0.0; 2],
        }
    }
    fn process(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let mut out = [0.0f32; 2];
        for channel in 0..2 {
            let x = frame[channel];
            let y = self.b0 * x + self.z1[channel];
            self.z1[channel] = self.b1 * x - self.a1 * y + self.z2[channel];
            self.z2[channel] = self.b2 * x - self.a2 * y;
            out[channel] = if y.is_finite() { y.clamp(-1.0, 1.0) } else { 0.0 };
        }
        out
    }
}

fn hermite(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let c0 = p1;
    let c1 = 0.5 * (p2 - p0);
    let c2 = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
    let c3 = 0.5 * (p3 - p0) + 1.5 * (p1 - p2);
    ((c3 * t + c2) * t + c1) * t + c0
}

/// Streaming rate converter: cubic Hermite interpolation with a pre-filter when
/// the rate drops. Keeps four samples of lookahead, so it delays by one frame and
/// never needs the whole file in memory.
pub struct Resampler {
    step: f64,
    position: f64,
    window: VecDeque<[f32; 2]>,
    lowpass: Option<Biquad>,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Self {
        let lowpass = (from > to).then(|| Biquad::low_pass(from as f32, to as f32 * 0.45));
        Self {
            step: from as f64 / to as f64,
            position: 0.0,
            window: VecDeque::with_capacity(4),
            lowpass,
        }
    }
    pub fn process(&mut self, input: &[[f32; 2]], out: &mut Vec<[f32; 2]>) {
        for frame in input {
            let frame = match &mut self.lowpass {
                Some(filter) => filter.process(*frame),
                None => *frame,
            };
            self.window.push_back(frame);
            loop {
                if self.window.len() < 4 {
                    break;
                }
                let (p0, p1, p2, p3) = (
                    self.window[0],
                    self.window[1],
                    self.window[2],
                    self.window[3],
                );
                let t = self.position as f32;
                out.push([
                    hermite(p0[0], p1[0], p2[0], p3[0], t),
                    hermite(p0[1], p1[1], p2[1], p3[1], t),
                ]);
                self.position += self.step;
                while self.position >= 1.0 {
                    self.position -= 1.0;
                    self.window.pop_front();
                }
            }
        }
    }
}

// ---------- decoding ----------

/// What the sink wants after receiving a chunk of decoded audio.
pub enum Control {
    Continue,
    Stop,
    Seek(f64),
}

fn label_for(path: &Path, sample_rate: u32, channels: usize) -> String {
    let container = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_uppercase();
    let layout = match channels {
        1 => "mono".to_string(),
        2 => "stereo".to_string(),
        other => format!("{other} ch"),
    };
    format!(
        "{container} · {:.1} kHz · {layout}",
        sample_rate as f64 / 1000.0
    )
}

fn open_stream(path: &Path) -> Result<symphonia::core::probe::ProbeResult, String> {
    let file = File::open(path).map_err(|error| format!("Cannot read that file: {error}"))?;
    let stream = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }
    symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )
        .map_err(|error| format!("This file cannot be decoded: {error}"))
}

/// Decodes a whole file at the engine rate, handing each converted block to the
/// sink and stopping when it returns false or the render asks to cancel. The
/// master page is the caller: it wants the same decoder and the same resampler
/// playback uses, without an output device in the way.
pub fn decode(
    path: &Path,
    sink: &mut dyn FnMut(&[[f32; 2]]) -> bool,
) -> Result<(), String> {
    let mut reader = Reader::open(path, 0.0)?;
    reader.run(&mut |block| {
        if sink(block) {
            Control::Continue
        } else {
            Control::Stop
        }
    })?;
    Ok(())
}

/// Probes a file without opening the output, so unsupported files fail at once.
pub fn describe(path: &Path) -> Result<(String, f64), String> {
    let reader = Reader::open(path, 0.0)?;
    Ok((reader.label.clone(), reader.duration))
}

/// Lengths for the shelf's hover labels, read on demand so a scan never pays for them:
/// one header probe per file, and a file that cannot be read is simply unknown.
#[tauri::command]
pub fn track_durations(paths: Vec<String>) -> Vec<Option<f64>> {
    paths
        .iter()
        .map(|path| describe(Path::new(path)).ok().map(|(_, duration)| duration))
        .collect()
}

struct Reader {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    duration: f64,
    label: String,
    resampler: Resampler,
    interleaved: Vec<[f32; 2]>,
}

impl Reader {
    fn open(path: &Path, start: f64) -> Result<Self, String> {
        let mut format = open_stream(path)?.format;
        let (track_id, sample_rate, duration, channels, params) = {
            let track = format
                .tracks()
                .iter()
                .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
                .ok_or("This file has no decodable audio track")?;
            let sample_rate = track.codec_params.sample_rate.unwrap_or(RATE);
            let duration = track
                .codec_params
                .n_frames
                .map(|frames| frames as f64 / sample_rate as f64)
                .unwrap_or(0.0);
            let channels = track
                .codec_params
                .channels
                .map(|channels| channels.count())
                .unwrap_or(2);
            (
                track.id,
                sample_rate,
                duration,
                channels,
                track.codec_params.clone(),
            )
        };
        let decoder = symphonia::default::get_codecs()
            .make(&params, &DecoderOptions::default())
            .map_err(|error| format!("No decoder for this file: {error}"))?;
        if start > 0.0 {
            format
                .seek(
                    SeekMode::Accurate,
                    SeekTo::Time {
                        time: Time::new(start.trunc() as u64, start.fract()),
                        track_id: Some(track_id),
                    },
                )
                .map_err(|error| format!("Cannot seek in this file: {error}"))?;
        }
        Ok(Self {
            format,
            decoder,
            track_id,
            duration,
            label: label_for(path, sample_rate, channels),
            resampler: Resampler::new(sample_rate, RATE),
            interleaved: Vec::new(),
        })
    }

    /// Decodes until the sink says otherwise. `Ok(Some(position))` means a seek
    /// was requested and the caller should reopen at that position.
    fn run(
        &mut self,
        control: &mut dyn FnMut(&[[f32; 2]]) -> Control,
    ) -> Result<Option<f64>, String> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(SymphoniaError::IoError(error))
                    if error.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(None)
                }
                Err(SymphoniaError::ResetRequired) => return Ok(None),
                Err(error) => return Err(format!("Cannot read this file: {error}")),
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            let decoded = match self.decoder.decode(&packet) {
                Ok(decoded) => decoded,
                Err(SymphoniaError::DecodeError(_)) => continue,
                Err(SymphoniaError::IoError(error))
                    if error.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(None)
                }
                Err(error) => return Err(format!("Decode failed: {error}")),
            };
            if decoded.frames() == 0 {
                continue;
            }
            let channels = decoded.spec().channels.count().max(1);
            let mut buffer =
                SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
            buffer.copy_interleaved_ref(decoded);
            let samples = buffer.samples();
            self.interleaved.clear();
            for frame in samples.chunks_exact(channels) {
                let left = frame[0];
                let right = if frame.len() > 1 { frame[1] } else { frame[0] };
                self.interleaved.push([
                    if left.is_finite() {
                        left.clamp(-1.0, 1.0)
                    } else {
                        0.0
                    },
                    if right.is_finite() {
                        right.clamp(-1.0, 1.0)
                    } else {
                        0.0
                    },
                ]);
            }
            let mut converted = Vec::with_capacity(self.interleaved.len() + 2);
            self.resampler.process(&self.interleaved, &mut converted);
            if converted.is_empty() {
                continue;
            }
            match control(&converted) {
                Control::Continue => {}
                Control::Stop => return Ok(None),
                Control::Seek(position) => return Ok(Some(position)),
            }
        }
    }
}

// ---------- output ----------

fn resolve_device(device_id: &str) -> Result<wasapi::Device, String> {
    wasapi::initialize_mta().ok().map_err(fail)?;
    let enumerator = DeviceEnumerator::new().map_err(fail)?;
    if device_id.is_empty() {
        return enumerator
            .get_default_device(&Direction::Render)
            .map_err(fail);
    }
    let devices = enumerator
        .get_device_collection(&Direction::Render)
        .map_err(fail)?;
    for device in &devices {
        let device = device.map_err(fail)?;
        if device.get_id().map_err(fail)? == device_id {
            return Ok(device);
        }
    }
    Err("The selected playback output is unavailable".into())
}

fn take_seek(shared: &Shared) -> Option<f64> {
    shared.seek.lock().ok().and_then(|mut slot| slot.take())
}

/// Fills the ring, waiting while it is full. A stop or a seek request interrupts
/// the wait so neither transport action is blocked by a slow output.
fn push_frames(shared: &Shared, frames: &[[f32; 2]]) -> Control {
    let mut ring = match shared.ring.lock() {
        Ok(ring) => ring,
        Err(_) => return Control::Stop,
    };
    for frame in frames {
        while ring.len() >= RING_FRAMES {
            drop(ring);
            if shared.stop.load(Ordering::Acquire) {
                return Control::Stop;
            }
            if let Some(position) = take_seek(shared) {
                return Control::Seek(position);
            }
            thread::sleep(Duration::from_millis(3));
            ring = match shared.ring.lock() {
                Ok(ring) => ring,
                Err(_) => return Control::Stop,
            };
        }
        ring.push_back(*frame);
    }
    Control::Continue
}

fn run_decoder(shared: Arc<Shared>, path: PathBuf, start_at: f64) {
    let mut start = start_at;
    // Opening elsewhere than the top counts as a start position, so the reported
    // position is absolute from the first frame rather than from the new device.
    shared
        .base
        .store((start * RATE as f64) as u64, Ordering::Relaxed);
    shared.played.store(0, Ordering::Relaxed);
    loop {
        let mut reader = match Reader::open(&path, start) {
            Ok(reader) => reader,
            Err(error) => {
                if let Ok(mut status) = shared.status.lock() {
                    status.error = Some(error);
                }
                shared.finished.store(true, Ordering::Release);
                return;
            }
        };
        let outcome = reader.run(&mut |frames| push_frames(&shared, frames));
        if shared.stop.load(Ordering::Acquire) {
            return;
        }
        match outcome {
            Ok(Some(position)) => {
                start = position;
                if let Ok(mut ring) = shared.ring.lock() {
                    ring.clear();
                }
                shared
                    .base
                    .store((position * RATE as f64) as u64, Ordering::Relaxed);
                shared.played.store(0, Ordering::Relaxed);
            }
            Ok(None) => {
                shared.finished.store(true, Ordering::Release);
                return;
            }
            Err(error) => {
                if let Ok(mut status) = shared.status.lock() {
                    status.error = Some(error);
                }
                shared.finished.store(true, Ordering::Release);
                return;
            }
        }
    }
}

/// The measured frame path both outputs share: level, spatial stage, panner,
/// reverb, equaliser, limiter and the meter. Keeping it in one place is what
/// lets the WASAPI stereo loop and the Windows spatial loop publish the same
/// readings for the same frames.
struct Processor {
    chain: Chain,
    smooth_volume: f32,
    meter: Meter,
    ballistics: Ballistics,
    last_publish: Instant,
    bed: Bed,
}

impl Processor {
    fn new(spatial: SpatialConfig, chain_state: &ChainState) -> Self {
        Self {
            chain: Chain::new(spatial, chain_state.effective()),
            smooth_volume: 0.0,
            meter: Meter::new(),
            ballistics: Ballistics::new(),
            last_publish: Instant::now(),
            bed: Bed::new(spatial),
        }
    }

    fn frame(
        &mut self,
        input: [f32; 2],
        target: f32,
        direct: bool,
        spatial: SpatialConfig,
        config: &ChainConfig,
    ) -> [f32; 2] {
        self.smooth_volume += 0.002 * (target - self.smooth_volume);
        let placed = [
            input[0] * self.smooth_volume,
            input[1] * self.smooth_volume,
        ];
        // A comparison hears the two files, not the rack twice: the master already
        // contains it, so the source is what the rack would make of it and the
        // master is what that produced.
        if direct {
            return placed;
        }
        let processed = self.chain.process(placed, spatial, config);
        [
            processed[0].clamp(-1.0, 1.0),
            processed[1].clamp(-1.0, 1.0),
        ]
    }

    fn feeds(&mut self, spatial: SpatialConfig, frame: [f32; 2]) -> [f32; BED_CHANNELS] {
        self.bed.set_config(spatial);
        self.bed.feeds(frame)
    }

    fn publish(
        &mut self,
        paused: bool,
        analysis: &AnalysisState,
        chain_state: &ChainState,
        config: &ChainConfig,
    ) {
        chain_state.publish(self.chain.meters(config));
        if paused {
            analysis.hold(Source::Playback);
        } else if analysis.claim(Source::Playback) {
            // The panels belong to one source: while the input is being captured,
            // playback is heard but its readings are not the ones on show. Claiming
            // here (rather than only at stream start) is what lets a track that was
            // already playing take the panels back the moment the capture stops.
            let readings = self.meter.readings();
            let since = self.last_publish.elapsed().as_secs_f32();
            self.last_publish = Instant::now();
            analysis.publish(self.ballistics.apply(readings, since));
        }
    }
}

/// On a stereo sink there is no Windows stream to feed, so the Atmos position
/// renders through the app's own derived heights instead of passing through.
fn stereo_mode(config: SpatialConfig) -> SpatialConfig {
    if config.mode == SpatialMode::Atmos {
        SpatialConfig {
            mode: SpatialMode::DerivedHeights,
            ..config
        }
    } else {
        config
    }
}

fn run_engine(
    device_id: String,
    path: PathBuf,
    shared: Arc<Shared>,
    spatial: Arc<Mutex<SpatialConfig>>,
    chain_state: Arc<ChainState>,
    analysis: Arc<AnalysisState>,
    volume: Arc<AtomicU32>,
    monitor: Arc<AtomicU32>,
    direct: Arc<AtomicBool>,
    start_at: f64,
    ready: mpsc::SyncSender<Result<(), String>>,
) {
    let device = match resolve_device(&device_id) {
        Ok(device) => device,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let device_name = device
        .get_friendlyname()
        .unwrap_or_else(|_| "Output".into());
    let starting = spatial.lock().map(|config| *config).unwrap_or_default();
    // The sink is chosen once per track: the spatial interfaces are !Send and
    // Windows cannot switch an open stream's format, so a later mode change
    // applies to the next track. A missing spatial endpoint falls through to
    // the app's own derived renderer instead of failing playback.
    if starting.mode == SpatialMode::Atmos {
        if let Ok(sink) = SpatialSink::open(&device_id) {
            run_spatial_engine(
                sink,
                path,
                shared,
                spatial,
                chain_state,
                analysis,
                volume,
                monitor,
                direct,
                start_at,
                ready,
                device_name,
            );
            return;
        }
    }
    let mut client = match device.get_iaudioclient() {
        Ok(client) => client,
        Err(error) => {
            let _ = ready.send(Err(format!("Cannot open the playback output: {error}")));
            return;
        }
    };
    let format = WaveFormat::new(32, 32, &SampleType::Float, RATE as usize, 2, None);
    let mode = StreamMode::PollingShared {
        autoconvert: true,
        buffer_duration_hns: 400_000,
    };
    if let Err(error) = client.initialize_client(&format, &Direction::Render, &mode) {
        let _ = ready.send(Err(format!(
            "Cannot open the playback output (it may be in exclusive use): {error}"
        )));
        return;
    }
    let render = match client.get_audiorenderclient() {
        Ok(render) => render,
        Err(error) => {
            let _ = ready.send(Err(fail(error)));
            return;
        }
    };
    let max_frames = match client.get_buffer_size() {
        Ok(size) => size as usize,
        Err(error) => {
            let _ = ready.send(Err(fail(error)));
            return;
        }
    };
    let mut bytes = vec![0u8; max_frames * FRAME_BYTES];
    if render.write_to_device(max_frames, &bytes, None).is_err() {
        let _ = ready.send(Err("Cannot prime the playback output".into()));
        return;
    }
    if let Err(error) = client.start_stream() {
        let _ = ready.send(Err(format!("Cannot start the playback output: {error}")));
        return;
    }
    if let Ok(mut status) = shared.status.lock() {
        status.active = true;
        status.device = device_name;
        status.volume = f32::from_bits(volume.load(Ordering::Relaxed));
        status.spatial = if starting.mode == SpatialMode::Atmos {
            "derived".into()
        } else {
            "app".into()
        };
        status.error = None;
    }
    let _ = ready.send(Ok(()));
    analysis.claim(Source::Playback);
    let decoder_shared = shared.clone();
    let decoder = thread::spawn(move || run_decoder(decoder_shared, path, start_at));
    // Fill a little before the clock starts, otherwise the first frames are silence.
    let prefill = Instant::now();
    while shared.ring.lock().map(|ring| ring.len()).unwrap_or(0) < PREFILL_FRAMES
        && prefill.elapsed() < Duration::from_secs(2)
        && !shared.finished.load(Ordering::Relaxed)
    {
        thread::sleep(Duration::from_millis(4));
    }
    let mut processor = Processor::new(stereo_mode(starting), &chain_state);
    let mut local: Vec<[f32; 2]> = Vec::with_capacity(max_frames);
    let mut last_status = Instant::now();
    while !shared.stop.load(Ordering::Acquire) {
        let paused = shared.paused.load(Ordering::Relaxed);
        let available = match client.get_available_space_in_frames() {
            Ok(frames) => (frames as usize).min(max_frames),
            Err(error) => {
                if let Ok(mut status) = shared.status.lock() {
                    status.error = Some(fail(error));
                }
                break;
            }
        };
        let requested = spatial.lock().map(|config| *config).unwrap_or_default();
        // The spatial stage is a pass-through in Atmos mode because the bed is
        // handed to Windows after the chain. With no spatial sink, the same
        // mode renders through the app's own 7.1.4 height layer instead.
        let current = stereo_mode(requested);
        let chain_config = chain_state.effective();
        local.clear();
        let mut heard = 0usize;
        if !paused {
            if let Ok(mut ring) = shared.ring.lock() {
                for _ in 0..available {
                    match ring.pop_front() {
                        Some(frame) => {
                            local.push(frame);
                            heard += 1;
                        }
                        None => break,
                    }
                }
            }
        }
        for (index, slot) in bytes[..available * FRAME_BYTES]
            .chunks_exact_mut(FRAME_BYTES)
            .enumerate()
        {
            let frame = local.get(index).copied().unwrap_or([0.0; 2]);
            let target = f32::from_bits(volume.load(Ordering::Relaxed))
                * f32::from_bits(monitor.load(Ordering::Relaxed));
            let output =
                processor.frame(frame, target, direct.load(Ordering::Relaxed), current, &chain_config);
            // A paused stream is still flushing silence to the device. That is not a
            // measurement of anything, and feeding it to the meter would drag the last
            // window down to the floor before the pause could be held.
            if !paused {
                processor.meter.push(output);
            }
            for (channel, chunk) in slot.chunks_exact_mut(4).enumerate() {
                chunk.copy_from_slice(&output[channel].to_le_bytes());
            }
        }
        if available > 0 {
            if let Err(error) = render.write_to_device(available, &bytes[..available * FRAME_BYTES], None) {
                if let Ok(mut status) = shared.status.lock() {
                    status.error = Some(fail(error));
                }
                break;
            }
        }
        shared.played.fetch_add(heard as u64, Ordering::Relaxed);
        // The panels poll every 20 ms, so the engine publishes faster than they
        // read: every poll then carries a measurement taken since the last one
        // instead of the same frame three times, which is what made the meters
        // step at the old 80 ms cadence.
        if last_status.elapsed() >= Duration::from_millis(16) {
            last_status = Instant::now();
            if let Ok(mut status) = shared.status.lock() {
                status.volume = f32::from_bits(volume.load(Ordering::Relaxed));
                status.spatial = if requested.mode == SpatialMode::Atmos {
                    "derived".into()
                } else {
                    "app".into()
                };
            }
            // A pause is not a stop: no window was measured, so the last one is left
            // standing and flagged paused. The meter keeps the window it had half
            // filled, so resuming continues from the pause instead of from silence.
            processor.publish(paused, &analysis, &chain_state, &chain_config);
        }
        let drained = shared
            .ring
            .lock()
            .map(|ring| ring.is_empty())
            .unwrap_or(true);
        if shared.finished.load(Ordering::Relaxed) && drained {
            break;
        }
        thread::sleep(Duration::from_millis(3));
    }
    shared.stop.store(true, Ordering::Release);
    let _ = client.stop_stream();
    let _ = decoder.join();
    // Nothing is being measured any more, so no level is reported.
    analysis.idle(Source::Playback);
    if let Ok(mut status) = shared.status.lock() {
        status.active = false;
        status.playing = false;
        let finished = shared.finished.load(Ordering::Relaxed);
        status.finished = finished;
        if finished && status.duration > 0.0 {
            status.position = status.duration;
        }
    }
}

/// The Windows Spatial Sound path: the same measured stereo chain, then a
/// 7.1.4 bed handed to Windows as twelve static objects. The meter reads the
/// stereo bed source, not the platform's own render, which happens downstream
/// of this app.
fn run_spatial_engine(
    mut sink: SpatialSink,
    path: PathBuf,
    shared: Arc<Shared>,
    spatial: Arc<Mutex<SpatialConfig>>,
    chain_state: Arc<ChainState>,
    analysis: Arc<AnalysisState>,
    volume: Arc<AtomicU32>,
    monitor: Arc<AtomicU32>,
    direct: Arc<AtomicBool>,
    start_at: f64,
    ready: mpsc::SyncSender<Result<(), String>>,
    device_name: String,
) {
    if let Err(error) = sink.start() {
        let _ = ready.send(Err(error));
        return;
    }
    if let Ok(mut status) = shared.status.lock() {
        status.active = true;
        status.device = device_name;
        status.volume = f32::from_bits(volume.load(Ordering::Relaxed));
        status.spatial = "windows".into();
        status.error = None;
    }
    let _ = ready.send(Ok(()));
    analysis.claim(Source::Playback);
    let decoder_shared = shared.clone();
    let decoder = thread::spawn(move || run_decoder(decoder_shared, path, start_at));
    let starting = spatial.lock().map(|config| *config).unwrap_or_default();
    // Windows cannot change an open stream's format, so the sink renders the
    // Atmos bed for the whole track; a different mode applies to the next one.
    let track_spatial = SpatialConfig {
        mode: SpatialMode::Atmos,
        ..starting
    };
    let mut processor = Processor::new(track_spatial, &chain_state);
    let mut local: Vec<[f32; 2]> = Vec::new();
    let mut feeds: Vec<[f32; BED_CHANNELS]> = Vec::new();
    let mut last_status = Instant::now();
    let mut timeouts = 0u32;
    while !shared.stop.load(Ordering::Acquire) {
        if !sink.wait() {
            if shared.stop.load(Ordering::Acquire) {
                break;
            }
            // A format or endpoint change stops the platform's event. Reset the
            // stream; if it keeps happening, report it instead of spinning.
            timeouts += 1;
            if timeouts > 20 {
                if let Ok(mut status) = shared.status.lock() {
                    status.error =
                        Some("Windows spatial output stopped answering; playback was stopped.".into());
                }
                break;
            }
            if let Err(error) = sink.reset() {
                if let Ok(mut status) = shared.status.lock() {
                    status.error = Some(error);
                }
                break;
            }
            continue;
        }
        timeouts = 0;
        let frames = match sink.begin() {
            Ok(frames) => frames,
            Err(error) => {
                if let Ok(mut status) = shared.status.lock() {
                    status.error = Some(error);
                }
                break;
            }
        };
        let paused = shared.paused.load(Ordering::Relaxed);
        let chain_config = chain_state.effective();
        let gain = f32::from_bits(volume.load(Ordering::Relaxed))
            * f32::from_bits(monitor.load(Ordering::Relaxed));
        local.clear();
        if !paused {
            if let Ok(mut ring) = shared.ring.lock() {
                for _ in 0..frames {
                    match ring.pop_front() {
                        Some(frame) => local.push(frame),
                        None => break,
                    }
                }
            }
        }
        let heard = local.len();
        feeds.clear();
        for index in 0..frames {
            let input = local.get(index).copied().unwrap_or([0.0; 2]);
            let output = processor.frame(
                input,
                gain,
                direct.load(Ordering::Relaxed),
                track_spatial,
                &chain_config,
            );
            // The reverb and limiter still run on the padded silence, so a tail
            // rings out instead of stopping at the last decoded frame.
            if !paused {
                processor.meter.push(output);
            }
            feeds.push(processor.feeds(track_spatial, output));
        }
        if let Err(error) = sink.write(&feeds) {
            if let Ok(mut status) = shared.status.lock() {
                status.error = Some(error);
            }
            break;
        }
        if let Err(error) = sink.end() {
            if let Ok(mut status) = shared.status.lock() {
                status.error = Some(error);
            }
            break;
        }
        shared.played.fetch_add(heard as u64, Ordering::Relaxed);
        if last_status.elapsed() >= Duration::from_millis(16) {
            last_status = Instant::now();
            if let Ok(mut status) = shared.status.lock() {
                status.volume = gain;
                status.spatial = "windows".into();
            }
            processor.publish(paused, &analysis, &chain_state, &chain_config);
        }
        let drained = shared
            .ring
            .lock()
            .map(|ring| ring.is_empty())
            .unwrap_or(true);
        if shared.finished.load(Ordering::Relaxed) && drained {
            break;
        }
    }
    shared.stop.store(true, Ordering::Release);
    let _ = decoder.join();
    // Nothing is being measured any more, so no level is reported.
    analysis.idle(Source::Playback);
    if let Ok(mut status) = shared.status.lock() {
        status.active = false;
        status.playing = false;
        let finished = shared.finished.load(Ordering::Relaxed);
        status.finished = finished;
        if finished && status.duration > 0.0 {
            status.position = status.duration;
        }
        status.spatial = String::new();
    }
}

// ---------- commands ----------

#[tauri::command]
pub fn playback_status(state: tauri::State<'_, PlayerState>) -> Result<PlaybackStatus, String> {
    Ok(state.status())
}

#[tauri::command]
pub fn play_track(
    state: tauri::State<'_, PlayerState>,
    path: String,
    device_id: String,
    title: String,
    artist: String,
    album: String,
) -> Result<(), String> {
    start_track(&state, path, device_id, title, artist, album, 0.0, false)
}

/// Opens the output and the decoder for one track. `start_at` is where playback
/// begins and `paused` whether the stream starts held, so a track moved to another
/// output keeps the position already heard instead of starting over.
fn start_track(
    state: &PlayerState,
    path: String,
    device_id: String,
    title: String,
    artist: String,
    album: String,
    start_at: f64,
    paused: bool,
) -> Result<(), String> {
    let track = PathBuf::from(&path);
    if !track.is_file() {
        return Err("That track is no longer on disk".into());
    }
    let (format, duration) = describe(&track)?;
    let title = if title.trim().is_empty() {
        track
            .file_stem()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone())
    } else {
        title
    };
    state.shutdown()?;
    let shared = Arc::new(Shared {
        ring: Mutex::new(VecDeque::with_capacity(RING_FRAMES)),
        status: Mutex::new(PlaybackStatus {
            active: false,
            playing: !paused,
            finished: false,
            title,
            artist,
            album,
            path: path.clone(),
            format,
            device: String::new(),
            position: start_at,
            duration,
            volume: f32::from_bits(state.volume.load(Ordering::Relaxed)),
            spatial: String::new(),
            error: None,
        }),
        stop: AtomicBool::new(false),
        paused: AtomicBool::new(paused),
        seek: Mutex::new(None),
        played: AtomicU64::new(0),
        base: AtomicU64::new(0),
        finished: AtomicBool::new(false),
    });
    let spatial = state.spatial.clone();
    let chain = state.chain.clone();
    let analysis = state.analysis.clone();
    let volume = state.volume.clone();
    let monitor = state.monitor.clone();
    let direct = state.direct.clone();
    let engine_shared = shared.clone();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let handle = thread::spawn(move || {
        run_engine(
            device_id,
            track,
            engine_shared,
            spatial,
            chain,
            analysis,
            volume,
            monitor,
            direct,
            start_at,
            ready_tx,
        )
    });
    {
        let mut inner = state.inner.lock().map_err(fail)?;
        inner.shared = Some(shared);
        inner.thread = Some(handle);
    }
    match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => {
            state.shutdown()?;
            Err(error)
        }
        Err(_) => {
            state.shutdown().ok();
            Err("Playback startup timed out".into())
        }
    }
}

/// What a device change carries over: the loaded track and the place it is at.
struct Resume {
    path: String,
    title: String,
    artist: String,
    album: String,
    position: f64,
    paused: bool,
}

/// Nothing open is not an error: the new choice then applies to the next track.
fn resume_of(status: &PlaybackStatus) -> Option<Resume> {
    if !status.active || status.path.is_empty() {
        return None;
    }
    Some(Resume {
        path: status.path.clone(),
        title: status.title.clone(),
        artist: status.artist.clone(),
        album: status.album.clone(),
        position: status.position,
        paused: !status.playing,
    })
}

/// Moves the running track to another output. Windows cannot switch an open
/// stream's device, so this reopens the sink on the chosen one and continues from
/// the position already heard, paused if it was paused.
#[tauri::command]
pub fn set_playback_device(
    state: tauri::State<'_, PlayerState>,
    device_id: String,
) -> Result<(), String> {
    let Some(resume) = resume_of(&state.status()) else {
        return Ok(());
    };
    start_track(
        &state,
        resume.path,
        device_id,
        resume.title,
        resume.artist,
        resume.album,
        resume.position,
        resume.paused,
    )
}

#[tauri::command]
pub fn pause_playback(state: tauri::State<'_, PlayerState>) -> Result<(), String> {
    if let Ok(inner) = state.inner.lock() {
        if let Some(shared) = &inner.shared {
            shared.paused.store(true, Ordering::Relaxed);
            return Ok(());
        }
    }
    Err("Nothing is playing".into())
}

#[tauri::command]
pub fn resume_playback(state: tauri::State<'_, PlayerState>) -> Result<(), String> {
    if let Ok(inner) = state.inner.lock() {
        if let Some(shared) = &inner.shared {
            shared.paused.store(false, Ordering::Relaxed);
            return Ok(());
        }
    }
    Err("Nothing is loaded".into())
}

#[tauri::command]
pub fn stop_playback(state: tauri::State<'_, PlayerState>) -> Result<(), String> {
    state.shutdown()
}

#[tauri::command]
pub fn seek_playback(state: tauri::State<'_, PlayerState>, position: f64) -> Result<(), String> {
    if !position.is_finite() || position < 0.0 {
        return Err("Seek position must be zero or greater".into());
    }
    if let Ok(inner) = state.inner.lock() {
        if let Some(shared) = &inner.shared {
            if let Ok(mut slot) = shared.seek.lock() {
                *slot = Some(position);
                return Ok(());
            }
        }
    }
    Err("Nothing is playing".into())
}

#[tauri::command]
pub fn set_playback_volume(state: tauri::State<'_, PlayerState>, volume: f32) -> Result<(), String> {
    if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
        return Err("Volume must be between 0 and 1".into());
    }
    state.volume.store(volume.to_bits(), Ordering::Relaxed);
    Ok(())
}

/// What a monitor trim in dB is as a linear gain. Attenuation only: a comparison that
/// had to add gain would be a comparison that could clip.
fn monitor_gain(db: f32) -> Result<f32, String> {
    if !db.is_finite() || !(-24.0..=0.0).contains(&db) {
        return Err("A monitor trim must be between -24 and 0 dB".into());
    }
    Ok(10f32.powf(db / 20.0))
}

/// A gain for a level-matched comparison, applied on top of the user's level and
/// never written to it.
#[tauri::command]
pub fn set_monitor_trim(state: tauri::State<'_, PlayerState>, db: f32) -> Result<(), String> {
    let gain = monitor_gain(db)?;
    state.monitor.store(gain.to_bits(), Ordering::Relaxed);
    Ok(())
}

/// Hear the files themselves for a comparison. The master already carries the rack,
/// so a comparison through it would judge a twice-processed file against a once
/// processed one; this is a listening aid, not a setting, and it is off by default.
#[tauri::command]
pub fn set_monitor_direct(state: tauri::State<'_, PlayerState>, direct: bool) {
    state.direct.store(direct, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tone(rate: u32, hz: f32, frames: usize) -> Vec<[f32; 2]> {
        (0..frames)
            .map(|index| {
                let value =
                    (2.0 * std::f32::consts::PI * hz * index as f32 / rate as f32).sin() * 0.5;
                [value, value]
            })
            .collect()
    }

    fn rms(frames: &[[f32; 2]]) -> f32 {
        if frames.is_empty() {
            return 0.0;
        }
        let sum: f64 = frames.iter().map(|frame| (frame[0] as f64).powi(2)).sum();
        (sum / frames.len() as f64).sqrt() as f32
    }

    fn zero_crossings(frames: &[[f32; 2]]) -> usize {
        frames
            .windows(2)
            .filter(|pair| (pair[0][0] <= 0.0) != (pair[1][0] <= 0.0))
            .count()
    }

    fn empty_shared(duration: f64) -> Shared {
        Shared {
            ring: Mutex::new(VecDeque::with_capacity(RING_FRAMES)),
            status: Mutex::new(PlaybackStatus {
                active: true,
                duration,
                ..PlaybackStatus::default()
            }),
            stop: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            seek: Mutex::new(None),
            played: AtomicU64::new(0),
            base: AtomicU64::new(0),
            finished: AtomicBool::new(false),
        }
    }

    #[test]
    fn resampling_at_the_same_rate_keeps_the_signal() {
        let input = tone(RATE, 440.0, 1000);
        let mut resampler = Resampler::new(RATE, RATE);
        let mut out = Vec::new();
        resampler.process(&input, &mut out);
        // The converter holds a short window back, so the tail is a few frames short.
        assert!(input.len() - out.len() <= 3, "{} frames", out.len());
        // The first emitted frame is the second input sample (the window needs one
        // sample of run-up), and every frame after it lines up exactly.
        for index in 0..out.len() - 1 {
            assert!(
                (out[index][0] - input[index + 1][0]).abs() < 1e-6,
                "frame {index}"
            );
        }
    }

    #[test]
    fn resampling_up_converts_rate_and_keeps_the_tone() {
        let from = 44_100u32;
        let input = tone(from, 440.0, from as usize);
        let mut resampler = Resampler::new(from, RATE);
        let mut out = Vec::new();
        resampler.process(&input, &mut out);
        assert!(
            (out.len() as i64 - RATE as i64).abs() < 120,
            "{} frames",
            out.len()
        );
        // 440 Hz over one second is 880 zero crossings.
        let crossings = zero_crossings(&out);
        assert!((crossings as i64 - 880).abs() < 40, "{crossings} crossings");
        assert!((rms(&out) - 0.3536).abs() < 0.05, "rms {}", rms(&out));
    }

    #[test]
    fn resampling_down_halves_the_frames_and_rejects_aliasing() {
        let from = 96_000u32;
        let clean = tone(from, 1_000.0, from as usize);
        let mut resampler = Resampler::new(from, RATE);
        let mut out = Vec::new();
        resampler.process(&clean, &mut out);
        assert!(
            (out.len() as i64 - RATE as i64).abs() < 120,
            "{} frames",
            out.len()
        );
        assert!((rms(&out) - 0.3536).abs() < 0.06, "rms {}", rms(&out));

        // 40 kHz would alias to 8 kHz without a pre-filter; the low-pass must remove it.
        let alias = tone(from, 40_000.0, from as usize);
        let mut resampler = Resampler::new(from, RATE);
        let mut filtered = Vec::new();
        resampler.process(&alias, &mut filtered);
        assert!(
            rms(&filtered) < 0.1,
            "aliasing leaked through: {}",
            rms(&filtered)
        );
    }

    #[test]
    fn resampler_stays_finite_across_rates() {
        let mut seed = 0x2545_f491u32;
        let noise: Vec<[f32; 2]> = (0..2048)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let value = (seed as f32 / u32::MAX as f32) * 2.0 - 1.0;
                [value, -value]
            })
            .collect();
        for rate in [8_000u32, 22_050, 44_100, 48_000, 96_000, 192_000] {
            let mut resampler = Resampler::new(rate, RATE);
            let mut out = Vec::new();
            resampler.process(&noise, &mut out);
            assert!(!out.is_empty(), "no output at {rate} Hz");
            assert!(
                out.iter()
                    .all(|frame| frame[0].is_finite() && frame[1].is_finite()),
                "non-finite sample at {rate} Hz"
            );
        }
    }

    fn wav_bytes(rate: u32, channels: u16, frames: usize, hz: f32) -> Vec<u8> {
        let mut data = Vec::with_capacity(frames * channels as usize * 2);
        for index in 0..frames {
            let value = (2.0 * std::f32::consts::PI * hz * index as f32 / rate as f32).sin() * 0.4;
            let sample = (value * i16::MAX as f32) as i16;
            for _ in 0..channels {
                data.extend_from_slice(&sample.to_le_bytes());
            }
        }
        let byte_rate = rate * channels as u32 * 2;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&byte_rate.to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("audiorium-player-{name}"));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn write(directory: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = directory.join(name);
        let mut file = File::create(&path).unwrap();
        file.write_all(bytes).unwrap();
        path
    }

    fn decode_all(path: &Path, limit: usize) -> Result<Vec<[f32; 2]>, String> {
        let mut reader = Reader::open(path, 0.0)?;
        let mut out = Vec::new();
        reader.run(&mut |frames| {
            for frame in frames {
                if out.len() >= limit {
                    return Control::Stop;
                }
                out.push(*frame);
            }
            Control::Continue
        })?;
        Ok(out)
    }

    #[test]
    fn shelf_durations_answer_for_real_files_and_skip_missing_ones() {
        let directory = scratch("shelf-durations");
        let path = write(&directory, "one-second.wav", &wav_bytes(44_100, 2, 44_100, 220.0));
        let gone = directory.join("gone.wav");
        let durations = track_durations(vec![
            path.to_string_lossy().into_owned(),
            gone.to_string_lossy().into_owned(),
        ]);
        assert_eq!(durations.len(), 2);
        let first = durations[0].expect("a readable file has a length");
        assert!((first - 1.0).abs() < 0.02, "one second, saw {first}");
        assert!(durations[1].is_none(), "a missing file is unknown, not zero");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_pcm_wav_decodes_to_the_expected_duration() {
        let directory = scratch("wav");
        let path = write(
            &directory,
            "half-second.wav",
            &wav_bytes(44_100, 2, 22_050, 440.0),
        );
        let (label, duration) = describe(&path).unwrap();
        assert!(label.starts_with("WAV · 44.1 kHz · stereo"), "{label}");
        assert!((duration - 0.5).abs() < 0.01, "duration {duration}");
        let frames = decode_all(&path, 48_000).unwrap();
        assert!(
            (frames.len() as i64 - 24_000).abs() < 200,
            "{} frames",
            frames.len()
        );
        assert!((rms(&frames) - 0.2828).abs() < 0.05, "rms {}", rms(&frames));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn mono_files_are_duplicated_to_both_channels() {
        let directory = scratch("mono");
        let path = write(&directory, "mono.wav", &wav_bytes(48_000, 1, 4_800, 300.0));
        let frames = decode_all(&path, 8_000).unwrap();
        assert!(frames.len() > 4_000);
        assert!(frames
            .iter()
            .all(|frame| (frame[0] - frame[1]).abs() < 1e-6));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn seek_starts_decoding_at_the_requested_position() {
        let directory = scratch("seek");
        let path = write(&directory, "tone.wav", &wav_bytes(48_000, 2, 48_000, 100.0));
        let mut reader = Reader::open(&path, 0.5).unwrap();
        let mut heard = Vec::new();
        reader
            .run(&mut |frames| {
                heard.extend_from_slice(frames);
                Control::Stop
            })
            .unwrap();
        assert!(!heard.is_empty());
        // 100 Hz: 0.5 s into the file is 50 cycles, so the wave is back at zero.
        assert!(heard[0][0].abs() < 0.02, "started at {}", heard[0][0]);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn unreadable_files_fail_with_a_clear_message() {
        let directory = scratch("broken");
        let path = write(&directory, "notes.mp3", b"this is not audio");
        assert!(describe(&path).is_err());
        assert!(matches!(decode_all(&path, 10), Err(message) if !message.is_empty()));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn atmos_falls_back_to_the_derived_heights_on_a_stereo_sink() {
        let atmos = SpatialConfig {
            mode: SpatialMode::Atmos,
            width: 0.42,
            ..SpatialConfig::default()
        };
        let fallback = stereo_mode(atmos);
        assert_eq!(fallback.mode, SpatialMode::DerivedHeights);
        assert_eq!(fallback.width, atmos.width);
        let surround = SpatialConfig {
            mode: SpatialMode::Surround71,
            ..SpatialConfig::default()
        };
        assert_eq!(stereo_mode(surround).mode, SpatialMode::Surround71);
    }

    #[test]
    fn position_uses_frames_actually_heard() {
        let shared = empty_shared(30.0);
        shared
            .played
            .store(RATE as u64 * 3 / 2, Ordering::Relaxed);
        shared.base.store(RATE as u64 * 10, Ordering::Relaxed);
        let status = live_status(&shared).unwrap();
        assert!((status.position - 11.5).abs() < 0.001);
        assert!(status.playing);
        shared.paused.store(true, Ordering::Relaxed);
        assert!(!live_status(&shared).unwrap().playing);
    }

    #[test]
    fn a_device_change_carries_the_track_and_its_place() {
        let paused = PlaybackStatus {
            active: true,
            playing: false,
            title: "Says".into(),
            path: "D:\\Music\\a.flac".into(),
            position: 42.5,
            ..PlaybackStatus::default()
        };
        let resume = resume_of(&paused).expect("a paused track can move output");
        assert_eq!(resume.path, "D:\\Music\\a.flac");
        assert_eq!(resume.title, "Says");
        assert!((resume.position - 42.5).abs() < 1e-9);
        assert!(resume.paused);
        let playing = PlaybackStatus {
            playing: true,
            ..paused
        };
        assert!(!resume_of(&playing).unwrap().paused);
        // Nothing open, and nothing finished, are both "no track to move".
        assert!(resume_of(&PlaybackStatus::default()).is_none());
        let finished = PlaybackStatus {
            path: "D:\\Music\\a.flac".into(),
            position: 200.0,
            ..PlaybackStatus::default()
        };
        assert!(resume_of(&finished).is_none());
    }

    /// The level is set before a track is played, which is exactly when there is
    /// no engine to read it from: the status must still report what was chosen,
    /// or the slider following it snaps back to the default on the next poll.
    #[test]
    fn the_playback_level_is_reported_before_anything_has_played() {
        let state = PlayerState::new(
            Arc::new(Mutex::new(SpatialConfig::default())),
            Arc::new(ChainState::memory()),
            Arc::new(AnalysisState::default()),
        );
        assert!((state.status().volume - 0.7).abs() < 1e-6, "the level the app starts at");
        state.volume.store(0.42f32.to_bits(), Ordering::Relaxed);
        assert!(
            (state.status().volume - 0.42).abs() < 1e-6,
            "the level just set, with nothing playing"
        );
        // A full level and silence are both real settings, not "no answer".
        state.volume.store(1.0f32.to_bits(), Ordering::Relaxed);
        assert!((state.status().volume - 1.0).abs() < 1e-6);
        state.volume.store(0.0f32.to_bits(), Ordering::Relaxed);
        assert!((state.status().volume - 0.0).abs() < 1e-6);
    }

    #[test]
    fn pushing_more_than_the_ring_holds_never_unbounds_it() {
        let shared = empty_shared(10.0);
        if let Ok(mut ring) = shared.ring.lock() {
            for _ in 0..RING_FRAMES {
                ring.push_back([0.0, 0.0]);
            }
        }
        // A full ring blocks the decoder; a seek request must win over that wait.
        *shared.seek.lock().unwrap() = Some(12.5);
        match push_frames(&shared, &[[1.0, 1.0]]) {
            Control::Seek(position) => assert!((position - 12.5).abs() < 1e-9),
            _ => panic!("expected the seek request to interrupt backpressure"),
        }
        assert_eq!(shared.ring.lock().unwrap().len(), RING_FRAMES);
    }
    #[test]
    fn a_monitor_trim_only_ever_attenuates() {
        // -6 dB is half the level, and the comparison is level-matched by pulling the
        // louder file down, never by pushing the quieter one up into the ceiling.
        assert!((monitor_gain(0.0).unwrap() - 1.0).abs() < 1e-6);
        assert!((monitor_gain(-6.0).unwrap() - 0.501_187).abs() < 1e-4);
        assert!((monitor_gain(-24.0).unwrap() - 0.063_095_7).abs() < 1e-6);
        assert!(monitor_gain(0.5).is_err(), "a positive trim was accepted");
        assert!(monitor_gain(-25.0).is_err(), "a trim below the range was accepted");
        assert!(monitor_gain(f32::NAN).is_err(), "a non-finite trim was accepted");
    }


}
