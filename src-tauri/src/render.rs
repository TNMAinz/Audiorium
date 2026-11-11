//! Rendering a file through the rack, offline and faster than realtime.
//!
//! Playback shows the rack working on one stream; this is the same rack done to a
//! whole file, so a master is the processors the page is showing and not a
//! second, private implementation of them. It runs on its own thread with its own
//! `Chain`: playback keeps playing while a render runs, and neither can move the
//! other's state. The spatial layout is deliberately left out — a master is the
//! rack's work, not the mode the speakers happen to be in.
//!
//! The measurements come from the same pass as the audio: a source meter before
//! the rack and an output meter after it, so the page's before/after figures are
//! the file it just wrote rather than a second read of it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use serde::Serialize;

use crate::chain::{Chain, ChainConfig, ChainState};
use crate::export::{BitDepth, Format, SampleRate, Tags, Writer};
use crate::loudness::{self, Loudness, LoudnessReadings};
use crate::player;
use crate::resample::Resampler;
use crate::spatial::SpatialConfig;

const RATE: f64 = 48_000.0;
/// Whatever the rack still has to say when the file ends: the limiter's look-ahead,
/// the saturator's oversampler, and the room's tail. Rendering the tail is what
/// keeps a mastered file from cutting off the reverb the rack added.
fn tail_frames(config: &ChainConfig) -> usize {
    let room = if config.reverb.enabled {
        crate::reverb::reverb_time(config.reverb.decay) * RATE as f32
    } else {
        0.0
    };
    (room as usize) + crate::fx::tail_frames(&config.effects) + 72 + 48 + 2_400
}

/// What the master page shows while and after a render.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderStatus {
    pub running: bool,
    /// Named, not numbered: "starting", "rendering", "done", "cancelled", "failed".
    pub phase: String,
    pub progress: f32,
    pub frames: u64,
    pub seconds: f64,
    pub bytes: u64,
    pub destination: String,
    /// The file's own description: "16-bit PCM", "32-bit float".
    pub depth: String,
    /// The rate the master is written at: "44.1 kHz", "48 kHz", "96 kHz".
    pub rate: String,
    /// Which track of a queue is being written, and what it is called. A single
    /// master reads 1 of 1; an album counts through its tracks.
    pub track: String,
    pub track_index: usize,
    pub track_total: usize,
    /// The whole queue's programme loudness, once every file has been written.
    /// A single master leaves it at the floor: there is no album to judge.
    pub album_lufs: f32,
    pub source: LoudnessReadings,
    pub output: LoudnessReadings,
    pub error: String,
}

/// The finished render, as its own value so the thread never returns a status it
/// did not also publish.
#[derive(Debug)]
struct Report {
    frames: u64,
    seconds: f64,
    bytes: u64,
    destination: String,
    source: LoudnessReadings,
    output: LoudnessReadings,
    /// The output meter's 400 ms block energies, concatenated across a queue so an
    /// album is judged as one programme rather than as an average of averages.
    blocks: Vec<f64>,
}

/// One file of a render: where it comes from, where it goes, and what the page
/// calls it while it is being written.
#[derive(Clone, Debug)]
pub struct Job {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub title: String,
}

/// The one render the process can be running. A second start is refused rather
/// than queued: two writers on one path would interleave into one broken file.
pub struct RenderState {
    /// Shared with the worker thread, which owns the render and publishes into it.
    status: Arc<Mutex<RenderStatus>>,
    cancel: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
}

impl Default for RenderState {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderState {
    pub fn new() -> Self {
        Self {
            status: Arc::new(Mutex::new(RenderStatus {
                phase: "idle".into(),
                ..RenderStatus::default()
            })),
            cancel: Arc::new(AtomicBool::new(false)),
            busy: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn status(&self) -> RenderStatus {
        self.status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_default()
    }

    fn publish(&self, change: impl FnOnce(&mut RenderStatus)) {
        if let Ok(mut status) = self.status.lock() {
            change(&mut status);
        }
    }

    /// Start one master. A second start is refused rather than queued: two writers
    /// on one path would interleave into one broken file.
    pub fn start(
        &self,
        config: ChainConfig,
        source: PathBuf,
        destination: PathBuf,
        depth: BitDepth,
        format: Format,
        rate: SampleRate,
    ) -> Result<RenderStatus, String> {
        let title = source
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("track")
            .to_string();
        self.begin(
            config,
            vec![Job {
                source,
                destination,
                title,
            }],
            depth,
            format,
            rate,
        )
    }

    /// Start a whole album: every track in the order the queue names them, through
    /// the same rack, at the same depth and rate. The tracks are written one after
    /// another by one worker, so a cancelled album stops where it is and leaves no
    /// half-written file behind.
    pub fn start_album(
        &self,
        config: ChainConfig,
        jobs: Vec<Job>,
        depth: BitDepth,
        format: Format,
        rate: SampleRate,
    ) -> Result<RenderStatus, String> {
        if jobs.len() < 2 {
            return Err("An album needs more than one track".into());
        }
        self.begin(config, jobs, depth, format, rate)
    }

    fn begin(
        &self,
        config: ChainConfig,
        jobs: Vec<Job>,
        depth: BitDepth,
        format: Format,
        rate: SampleRate,
    ) -> Result<RenderStatus, String> {
        if self.busy.swap(true, Ordering::SeqCst) {
            return Err("A render is already running".into());
        }
        if jobs.is_empty() {
            self.busy.store(false, Ordering::SeqCst);
            return Err("There is nothing to render".into());
        }
        for job in &jobs {
            if !job.source.is_file() {
                self.busy.store(false, Ordering::SeqCst);
                return Err(format!("{} is not there any more", job.title));
            }
            if job.destination == job.source {
                self.busy.store(false, Ordering::SeqCst);
                return Err("A master cannot overwrite the file it is made from".into());
            }
        }
        let tail = tail_frames(&config) as u64;
        let total: u64 = jobs
            .iter()
            .map(|job| {
                let seconds = player::describe(&job.source)
                    .map(|(_, seconds)| seconds)
                    .unwrap_or(0.0);
                (seconds * RATE) as u64 + tail
            })
            .sum();
        let total_tracks = jobs.len();
        let first = &jobs[0];
        self.cancel.store(false, Ordering::SeqCst);
        self.publish(|status| {
            *status = RenderStatus {
                running: true,
                phase: "rendering".into(),
                progress: 0.0,
                destination: first.destination.to_string_lossy().to_string(),
                depth: crate::export::describe(depth, format),
                rate: rate.label().to_string(),
                track: first.title.clone(),
                track_index: 1,
                track_total: total_tracks,
                ..RenderStatus::default()
            };
        });

        let cancel = Arc::clone(&self.cancel);
        let busy = Arc::clone(&self.busy);
        // The thread owns the queue; it publishes through the shared status and
        // clears the busy flag even on an error path.
        let shared = Arc::clone(&self.status);
        thread::Builder::new()
            .name("audiorium-render".into())
            .spawn(move || {
                let mut done: u64 = 0;
                let mut blocks: Vec<f64> = Vec::new();
                let mut last: Option<Report> = None;
                let mut failure: Option<String> = None;
                for (index, job) in jobs.iter().enumerate() {
                    if cancel.load(Ordering::SeqCst) {
                        failure = Some("Render cancelled".into());
                        break;
                    }
                    // The page names the file being written before its first block
                    // arrives, so an album reads as a queue rather than as one
                    // anonymous bar.
                    if let Ok(mut status) = shared.lock() {
                        status.track_index = index + 1;
                        status.track = job.title.clone();
                        status.destination = job.destination.to_string_lossy().to_string();
                    }
                    let result = render_file(
                        &config,
                        &job.source,
                        &job.destination,
                        depth,
                        format,
                        rate,
                        &cancel,
                        &|frames| {
                            if let Ok(mut status) = shared.lock() {
                                status.frames = done + frames;
                                status.progress = if total == 0 {
                                    0.0
                                } else {
                                    ((done + frames) as f32 / total as f32).min(1.0)
                                };
                            }
                        },
                    );
                    match result {
                        Ok(report) => {
                            done += report.frames;
                            blocks.extend(report.blocks.iter().copied());
                            last = Some(report);
                        }
                        Err(error) => {
                            failure = Some(error);
                            break;
                        }
                    }
                }
                let album_lufs = if total_tracks > 1 && failure.is_none() {
                    loudness::integrated_of(&blocks)
                } else {
                    -70.0
                };
                if let Ok(mut status) = shared.lock() {
                    status.running = false;
                    match (failure, last) {
                        (None, Some(report)) => {
                            status.phase = "done".into();
                            status.progress = 1.0;
                            status.frames = report.frames;
                            status.seconds = report.seconds;
                            status.bytes = report.bytes;
                            status.destination = report.destination;
                            status.source = report.source;
                            status.output = report.output;
                            status.track_index = total_tracks;
                            status.album_lufs = album_lufs;
                            status.error.clear();
                        }
                        (Some(error), _) if cancel.load(Ordering::SeqCst) => {
                            status.phase = "cancelled".into();
                            status.error = error;
                        }
                        (Some(error), _) => {
                            status.phase = "failed".into();
                            status.error = error;
                        }
                        (None, None) => {
                            status.phase = "failed".into();
                            status.error = "There was nothing to render".into();
                        }
                    }
                }
                busy.store(false, Ordering::SeqCst);
            })
            .map_err(|error| {
                self.busy.store(false, Ordering::SeqCst);
                self.publish(|status| {
                    status.running = false;
                    status.phase = "failed".into();
                    status.error = format!("Cannot start the render: {error}");
                });
                format!("Cannot start the render: {error}")
            })?;
        Ok(self.status())
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

/// Rendering one file end to end. `progress` is called once per decoded block with
/// the frames written so far; `cancel` is checked between blocks, so cancelling
/// stops within one packet and leaves no half-written file behind.
fn render_file(
    config: &ChainConfig,
    source: &Path,
    destination: &Path,
    depth: BitDepth,
    format: Format,
    rate: SampleRate,
    cancel: &AtomicBool,
    progress: &dyn Fn(u64),
) -> Result<Report, String> {
    // A master keeps the description of the file it came from: the same findability in a
    // library, and the same cover, so an export is not a nameless rectangle. The tags are
    // read from the source itself rather than from whatever the page happens to display.
    let read = crate::library::tags_of(source);
    let (cover, cover_mime) = crate::artwork::original_cover(source);
    let tags = Tags {
        title: read.as_ref().and_then(|tags| tags.title.clone()).unwrap_or_default(),
        artist: read.as_ref().and_then(|tags| tags.artist.clone()).unwrap_or_default(),
        album: read.as_ref().and_then(|tags| tags.album.clone()).unwrap_or_default(),
        cover,
        cover_mime,
    };
    let out_rate = rate.hz();
    let mut writer = Writer::create(destination, format, depth, &tags, out_rate)?;
    // The rack runs at 48 kHz; a master at another rate is its output converted
    // once, here, rather than a second copy of every processor.
    let mut converter = Resampler::new(RATE as u32, out_rate);
    // A failed render must not leave a partial file where a master should be.
    let remove_partial = |error: String| {
        drop(std::fs::remove_file(destination));
        Err(error)
    };
    let mut chain = Chain::new(SpatialConfig::default(), config.clone());
    let mut source_meter = Loudness::new();
    let mut output_meter = Loudness::new();
    // Frames written, at the file's own rate, and frames pushed through the rack at
    // 48 kHz — the second is what the progress bar is a fraction of, so a 44.1 kHz
    // master still ends at 100 % instead of stalling at 92 %.
    let mut frames: u64 = 0;
    let mut pushed: u64 = 0;
    let mut buffer: Vec<[f32; 2]> = Vec::with_capacity(4_096);
    let mut converted: Vec<[f32; 2]> = Vec::with_capacity(4_096);

    let decoded = player::decode(source, &mut |block| {
        if cancel.load(Ordering::SeqCst) {
            return false;
        }
        for frame in block {
            source_meter.push(*frame);
            let out = chain.process_rack(*frame, config);
            // The figures are the rack's own output at the engine's rate: loudness is
            // a property of the programme, not of the rate it is stored at.
            output_meter.push(out);
            converter.push(out, &mut converted);
            pushed += 1;
        }
        if converted.len() >= 4_096 {
            let wrote = writer.write(&converted).is_ok();
            frames += converted.len() as u64;
            converted.clear();
            progress(pushed);
            if !wrote {
                return false;
            }
        }
        true
    });
    if let Err(error) = decoded {
        return remove_partial(error);
    }
    if cancel.load(Ordering::SeqCst) {
        return remove_partial("Render cancelled".into());
    }
    if !buffer.is_empty() {
        if let Err(error) = writer.write(&buffer) {
            return remove_partial(error);
        }
        frames += buffer.len() as u64;
        buffer.clear();
    }
    // The tail: the room and the delay lines still hold audio the file has not
    // heard yet, so the render runs past the last decoded frame by exactly their
    // length and then stops.
    let tail = tail_frames(config) as u64;
    for index in 0..tail {
        if cancel.load(Ordering::SeqCst) {
            return remove_partial("Render cancelled".into());
        }
        let out = chain.process_rack([0.0; 2], config);
        output_meter.push(out);
        converter.push(out, &mut converted);
        pushed += 1;
        if converted.len() >= 4_096 {
            if let Err(error) = writer.write(&converted) {
                return remove_partial(error);
            }
            frames += converted.len() as u64;
            converted.clear();
        }
        if index + 1 == tail {
            if !converted.is_empty() {
                if let Err(error) = writer.write(&converted) {
                    return remove_partial(error);
                }
                frames += converted.len() as u64;
                converted.clear();
            }
            progress(pushed);
        }
    }
    let bytes = writer.finish()?;
    Ok(Report {
        frames,
        seconds: frames as f64 / out_rate as f64,
        bytes,
        destination: destination.to_string_lossy().to_string(),
        source: source_meter.readings(),
        output: output_meter.readings(),
        blocks: output_meter.block_powers().to_vec(),
    })
}

/// The folder a master goes into, so a library stays the music that came into it and
/// the prints made from it sit in one place instead of beside every track.
pub const MASTERED_FOLDER: &str = "Mastered";

/// Where a master of this file goes: a `Mastered` folder in the library the file came
/// from, named for what it is. A file from outside the library is mastered into a
/// `Mastered` folder beside it, because writing into a library it does not belong to
/// would put a print somewhere the user never pointed at. A render never writes over
/// its source.
pub fn mastered_path(source: &str, format: Format, library: &str) -> String {
    let path = Path::new(source);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("track");
    let name = format!("{stem} (mastered).{}", format.extension());
    let root = library.trim();
    let folder = if !root.is_empty() && path.starts_with(root) {
        PathBuf::from(root).join(MASTERED_FOLDER)
    } else {
        match path.parent() {
            Some(parent) => parent.join(MASTERED_FOLDER),
            None => PathBuf::from(MASTERED_FOLDER),
        }
    };
    folder.join(name).to_string_lossy().to_string()
}

// ---------- commands ----------

/// Where a master of this file will be written. The page shows it before the
/// render starts, so the file never lands somewhere unannounced.
#[tauri::command]
pub fn master_destination(source: String, format: String, library: String) -> String {
    mastered_path(
        &source,
        Format::parse(&format).unwrap_or(Format::Wav),
        &library,
    )
}

/// Start a render of one library file through the rack as it is configured now.
#[tauri::command]
pub fn render_start(
    state: tauri::State<'_, Arc<RenderState>>,
    chain: tauri::State<'_, Arc<ChainState>>,
    source: String,
    bit_depth: String,
    format: String,
    rate: String,
    library: String,
) -> Result<RenderStatus, String> {
    let depth = BitDepth::parse(&bit_depth).ok_or("Unknown bit depth")?;
    let shape = Format::parse(&format).ok_or("Unknown format")?;
    let rate = SampleRate::parse(&rate).ok_or("Unknown sample rate")?;
    let destination = mastered_path(&source, shape, &library);
    // The folder is the engine's to make: a first master in a new library must not
    // fail with "cannot write that file" because the place does not exist yet.
    if let Some(parent) = Path::new(&destination).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Cannot write that folder: {error}"))?;
    }
    state.start(
        chain.config(),
        PathBuf::from(source),
        PathBuf::from(destination),
        depth,
        shape,
        rate,
    )
}

/// Start an album: the tracks in the order given, through the same rack, at the same
/// depth and rate, each written to its own master in the library's `Mastered` folder.
/// The order is the page's, because a release is a sequence and not a set.
#[tauri::command]
pub fn render_album(
    state: tauri::State<'_, Arc<RenderState>>,
    chain: tauri::State<'_, Arc<ChainState>>,
    tracks: Vec<String>,
    bit_depth: String,
    format: String,
    rate: String,
    library: String,
) -> Result<RenderStatus, String> {
    let depth = BitDepth::parse(&bit_depth).ok_or("Unknown bit depth")?;
    let shape = Format::parse(&format).ok_or("Unknown format")?;
    let rate = SampleRate::parse(&rate).ok_or("Unknown sample rate")?;
    let mut jobs = Vec::with_capacity(tracks.len());
    for source in &tracks {
        let destination = mastered_path(source, shape, &library);
        if let Some(parent) = Path::new(&destination).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("Cannot write that folder: {error}"))?;
        }
        let title = Path::new(source)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("track")
            .to_string();
        jobs.push(Job {
            source: PathBuf::from(source),
            destination: PathBuf::from(destination),
            title,
        });
    }
    state.start_album(chain.config(), jobs, depth, shape, rate)
}

/// Measure a file as a reference: its loudness, true peak, LRA and the rest, from
/// the same meter a render uses. A reference is not processed and not written — it
/// is the figure the master is compared with.
#[tauri::command]
pub fn reference_measure(source: String) -> Result<LoudnessReadings, String> {
    let path = Path::new(&source);
    if !path.is_file() {
        return Err("That file is not there any more".into());
    }
    let mut meter = Loudness::new();
    player::decode(path, &mut |block| {
        for frame in block {
            meter.push(*frame);
        }
        true
    })?;
    Ok(meter.readings())
}

/// Whether the render path itself is transparent: the source decoded and passed
/// through the rack with every stage bypassed, compared sample by sample with what
/// came out. A non-zero residual would mean the export colours a file it was never
/// asked to touch.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NullReport {
    pub frames: u64,
    pub differing: u64,
    pub max_difference_db: f32,
}

#[tauri::command]
pub fn null_test(source: String) -> Result<NullReport, String> {
    let path = Path::new(&source);
    if !path.is_file() {
        return Err("That file is not there any more".into());
    }
    // Every stage off: this measures the decode and the rack's own wiring, not the
    // processors. A stage that is switched off must be bit-exact pass-through.
    let config = ChainConfig::default();
    let mut chain = Chain::new(SpatialConfig::default(), config.clone());
    let mut frames: u64 = 0;
    let mut differing: u64 = 0;
    let mut worst: f32 = 0.0;
    player::decode(path, &mut |block| {
        for frame in block {
            let out = chain.process_rack(*frame, &config);
            for channel in 0..2 {
                let difference = (out[channel] - frame[channel]).abs();
                if difference > 0.0 {
                    differing += 1;
                }
                if difference > worst {
                    worst = difference;
                }
            }
            frames += 1;
        }
        true
    })?;
    Ok(NullReport {
        frames,
        differing,
        max_difference_db: if worst <= 0.0 {
            -120.0
        } else {
            20.0 * worst.log10()
        },
    })
}

#[tauri::command]
pub fn render_status(state: tauri::State<'_, Arc<RenderState>>) -> RenderStatus {
    state.status()
}

/// Stop the running render. The partial file is removed by the worker itself, so
/// nothing half-written is left for the user to mistake for a master.
#[tauri::command]
pub fn render_cancel(state: tauri::State<'_, Arc<RenderState>>) {
    state.cancel();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limiter::LimiterConfig;

    fn scratch(name: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("audiorium-render-{name}-{stamp}"));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    fn write_tone(path: &Path) {
        let mut writer = Writer::create(path, Format::Wav, BitDepth::Float32, &Tags::default(), 48_000)
            .expect("a source file");
        let frames: Vec<[f32; 2]> = (0..48_000)
            .map(|index| {
                let value =
                    (2.0 * std::f32::consts::PI * 440.0 * index as f32 / 48_000.0).sin() * 0.5;
                [value, value]
            })
            .collect();
        writer.write(&frames).expect("the tone");
        writer.finish().expect("a finished source");
    }

    #[test]
    fn a_render_writes_a_master_of_the_file_it_read() {
        let dir = scratch("master");
        let source = dir.join("tone.wav");
        write_tone(&source);
        let destination = dir.join("tone (mastered).wav");
        let config = ChainConfig {
            limiter: LimiterConfig {
                enabled: true,
                threshold_db: -12.0,
                auto_makeup: false,
                ..LimiterConfig::default()
            },
            ..ChainConfig::default()
        };
        let report = render_file(
            &config,
            &source,
            &destination,
            BitDepth::Sixteen,
            Format::Wav,
            SampleRate::Hz48000,
            &AtomicBool::new(false),
            &|_| {},
        )
        .expect("a finished render");
        assert!(destination.is_file(), "the master was not written");
        assert!(report.bytes > 44, "a header with no audio: {} bytes", report.bytes);
        assert!(report.frames >= 48_000, "only {} frames", report.frames);
        // The limiter the page shows is the limiter the file got.
        assert!(
            report.output.true_peak_db <= -11.5,
            "the master left {} dBTP",
            report.output.true_peak_db
        );
        assert!(
            report.output.integrated_lufs < report.source.integrated_lufs - 5.0,
            "source {} LUFS, master {} LUFS",
            report.source.integrated_lufs,
            report.output.integrated_lufs
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    fn write_tagged_tone(path: &Path) -> Vec<u8> {
        let cover = {
            use image::ImageEncoder;
            let mut png = Vec::new();
            let encoder = image::codecs::png::PngEncoder::new(&mut png);
            encoder
                .write_image(&[12u8, 34, 56], 1, 1, image::ExtendedColorType::Rgb8)
                .expect("a one-pixel cover");
            png
        };
        let tags = Tags {
            title: "First Light".into(),
            artist: "Nova".into(),
            album: "Night".into(),
            cover: cover.clone(),
            cover_mime: "image/png".into(),
        };
        let mut writer = Writer::create(path, Format::Wav, BitDepth::Float32, &tags, 48_000)
            .expect("a source file");
        let frames: Vec<[f32; 2]> = (0..48_000)
            .map(|index| {
                let value =
                    (2.0 * std::f32::consts::PI * 440.0 * index as f32 / 48_000.0).sin() * 0.5;
                [value, value]
            })
            .collect();
        writer.write(&frames).expect("the tone");
        writer.finish().expect("a finished source");
        cover
    }

    #[test]
    fn a_master_keeps_its_source_description_and_still_plays() {
        let dir = scratch("tags");
        let source = dir.join("tagged tone.wav");
        let cover = write_tagged_tone(&source);
        // The source really carries them, or the check below would prove nothing.
        let before = crate::library::tags_of(&source).expect("the source's tags");
        assert_eq!(before.title.as_deref(), Some("First Light"));

        let destination = dir.join("tagged tone (mastered).wav");
        render_file(
            &ChainConfig::default(),
            &source,
            &destination,
            BitDepth::TwentyFour,
            Format::Wav,
            SampleRate::Hz48000,
            &AtomicBool::new(false),
            &|_| {},
        )
        .expect("a finished render");

        // Title, artist and album travelled with the audio.
        let after = crate::library::tags_of(&destination).expect("the master's tags");
        assert_eq!(after.title.as_deref(), Some("First Light"));
        assert_eq!(after.artist.as_deref(), Some("Nova"));
        assert_eq!(after.album.as_deref(), Some("Night"));

        // So did the cover, byte for byte, and it is still a cover the app can draw.
        let (written, mime) = crate::artwork::original_cover(&destination);
        assert_eq!(written, cover, "the cover changed on the way out");
        assert_eq!(mime, "image/png");
        let thumbnail = crate::artwork::load(
            dir.to_str().unwrap(),
            destination.to_str().unwrap(),
        )
        .expect("the master's artwork");
        assert!(!thumbnail.is_empty(), "the master's cover cannot be read back");

        // And the metadata chunks did not put the audio out of reach of the decoder.
        let mut frames = 0_u64;
        crate::player::decode(&destination, &mut |block| {
            frames += block.len() as u64;
            true
        })
        .expect("the master decodes");
        assert!(frames >= 48_000, "the master decoded {frames} frames");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_flac_master_is_the_same_audio_smaller_and_still_tagged() {
        let dir = scratch("flac");
        let source = dir.join("tagged tone.wav");
        let cover = write_tagged_tone(&source);
        let wav = dir.join("tagged tone (mastered).wav");
        let flac = dir.join("tagged tone (mastered).flac");
        let config = ChainConfig::default();
        let render = |destination: &Path, format: Format| {
            render_file(
                &config,
                &source,
                destination,
                BitDepth::TwentyFour,
                format,
                SampleRate::Hz48000,
            &AtomicBool::new(false),
                &|_| {},
            )
            .expect("a finished render")
        };
        let wav_report = render(&wav, Format::Wav);
        let flac_report = render(&flac, Format::Flac);

        // A FLAC stream, and the length it reports is the length on disk.
        let bytes = std::fs::read(&flac).expect("the master");
        assert_eq!(&bytes[0..4], b"fLaC", "the master is not a FLAC stream");
        assert_eq!(flac_report.bytes, bytes.len() as u64);
        // The same rack, the same frames: only the container differs.
        assert_eq!(flac_report.frames, wav_report.frames);
        assert!(
            flac_report.bytes < wav_report.bytes,
            "FLAC {} bytes, WAV {} bytes",
            flac_report.bytes,
            wav_report.bytes
        );

        // The tags and the cover travel with the audio, unchanged.
        let after = crate::library::tags_of(&flac).expect("the master's tags");
        assert_eq!(after.title.as_deref(), Some("First Light"));
        assert_eq!(after.artist.as_deref(), Some("Nova"));
        assert_eq!(after.album.as_deref(), Some("Night"));
        let (written, mime) = crate::artwork::original_cover(&flac);
        assert_eq!(written, cover, "the cover changed on the way out");
        assert_eq!(mime, "image/png");
        assert!(!crate::artwork::load(dir.to_str().unwrap(), flac.to_str().unwrap())
            .expect("the master's artwork")
            .is_empty());

        // And it is still a stream the playback path reads from end to end, carrying the
        // same samples as the WAV of the same render: that is what lossless means here.
        let decode = |path: &Path| {
            let mut samples: Vec<[f32; 2]> = Vec::new();
            crate::player::decode(path, &mut |block| {
                samples.extend_from_slice(block);
                true
            })
            .expect("the master decodes");
            samples
        };
        let from_flac = decode(&flac);
        let from_wav = decode(&wav);
        assert!(from_flac.len() >= 48_000, "the master decoded {} frames", from_flac.len());
        assert_eq!(from_flac.len(), from_wav.len(), "the two containers hold different lengths");
        for (index, (flac, wav)) in from_flac.iter().zip(from_wav.iter()).enumerate() {
            for channel in 0..2 {
                assert_eq!(
                    flac[channel].to_bits(),
                    wav[channel].to_bits(),
                    "sample {index}/{channel} differs between the FLAC and the WAV"
                );
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_master_is_named_in_one_folder_and_never_over_its_source() {
        let named = mastered_path("/music/Album/Track 01.flac", Format::Wav, "/music");
        let expected = Path::new("/music")
            .join(MASTERED_FOLDER)
            .join("Track 01 (mastered).wav")
            .to_string_lossy()
            .to_string();
        assert_eq!(named, expected);
        assert_ne!(named, "/music/Album/Track 01.flac");
        // Every master of the library lands in the same folder, whichever album it
        // came from, so a master is never mixed in with the music.
        assert_eq!(
            Path::new(&mastered_path("/music/Other/Deep/Track.flac", Format::Wav, "/music"))
                .parent()
                .unwrap(),
            Path::new("/music").join(MASTERED_FOLDER)
        );
        // A FLAC master says so in its own extension.
        assert!(mastered_path("/music/Track.wav", Format::Flac, "/music").ends_with("Track (mastered).flac"));
        // A file from outside the library is mastered beside itself, not into a
        // library it was never scanned from.
        assert_eq!(
            mastered_path("/elsewhere/Album/Track.flac", Format::Wav, "/music"),
            Path::new("/elsewhere/Album")
                .join(MASTERED_FOLDER)
                .join("Track (mastered).wav")
                .to_string_lossy()
                .to_string()
        );
        // With no library chosen, the same folder beside the source.
        let fallback = PathBuf::from(mastered_path("Track.flac", Format::Wav, ""));
        assert_eq!(fallback.file_name().and_then(|name| name.to_str()), Some("Track (mastered).wav"));
        assert_eq!(
            fallback.parent().and_then(|parent| parent.file_name()).and_then(|name| name.to_str()),
            Some(MASTERED_FOLDER)
        );
    }

    #[test]
    fn a_first_master_makes_its_folder() {
        let dir = scratch("folder");
        let library = dir.join("music");
        std::fs::create_dir_all(library.join("Album")).unwrap();
        let source = library.join("Album").join("tone.wav");
        write_tone(&source);
        let destination = PathBuf::from(mastered_path(
            source.to_str().unwrap(),
            Format::Wav,
            library.to_str().unwrap(),
        ));
        assert_eq!(destination.parent().unwrap(), library.join(MASTERED_FOLDER));
        assert!(!destination.parent().unwrap().exists(), "the folder already existed");
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        render_file(
            &ChainConfig::default(),
            &source,
            &destination,
            BitDepth::Sixteen,
            Format::Wav,
            SampleRate::Hz48000,
            &AtomicBool::new(false),
            &|_| {},
        )
        .expect("a finished render");
        assert!(destination.is_file(), "the master did not land in the new folder");
        assert!(source.is_file(), "the source was moved");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_render_of_a_file_that_is_not_there_fails_with_a_reason() {
        let dir = scratch("missing");
        let config = ChainConfig::default();
        let error = render_file(
            &config,
            &dir.join("nothing.wav"),
            &dir.join("nothing (mastered).wav"),
            BitDepth::Sixteen,
            Format::Wav,
            SampleRate::Hz48000,
            &AtomicBool::new(false),
            &|_| {},
        )
        .expect_err("a missing source cannot render");
        assert!(error.to_lowercase().contains("read") || error.to_lowercase().contains("cannot"), "{error}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_master_can_be_written_at_441_and_96() {
        let dir = scratch("rates");
        let source = dir.join("tone.wav");
        write_tone(&source);
        let at_441 = dir.join("tone (mastered).wav");
        let report = render_file(
            &ChainConfig::default(),
            &source,
            &at_441,
            BitDepth::TwentyFour,
            Format::Wav,
            SampleRate::Hz44100,
            &AtomicBool::new(false),
            &|_| {},
        )
        .expect("a 44.1 kHz master");
        let bytes = std::fs::read(&at_441).expect("the master");
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(
            u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]),
            44_100,
            "the header does not name 44.1 kHz"
        );
        // One second of programme plus the rack's tail, at the rate it was written.
        assert!(
            (1.02..1.12).contains(&report.seconds),
            "a one-second source became {} s",
            report.seconds
        );

        let at_96 = dir.join("tone (mastered).flac");
        let report = render_file(
            &ChainConfig::default(),
            &source,
            &at_96,
            BitDepth::TwentyFour,
            Format::Flac,
            SampleRate::Hz96000,
            &AtomicBool::new(false),
            &|_| {},
        )
        .expect("a 96 kHz FLAC master");
        let bytes = std::fs::read(&at_96).expect("the FLAC");
        assert_eq!(&bytes[0..4], b"fLaC");
        // STREAMINFO's rate is the top 20 bits of the eight bytes after its ten bytes
        // of block and frame sizes. A player reads this figure, so it is the one to pin.
        let stream = u64::from_be_bytes([
            bytes[18], bytes[19], bytes[20], bytes[21], bytes[22], bytes[23], bytes[24], bytes[25],
        ]);
        assert_eq!(stream >> 44, 96_000, "STREAMINFO does not name 96 kHz");
        // The duration is the source's, at any rate: a 96 kHz master is not longer,
        // it carries the same second in more samples.
        assert!(
            (1.02..1.12).contains(&report.seconds),
            "a one-second source became {} s at 96 kHz",
            report.seconds
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    fn write_tone_at(path: &Path, amplitude: f32) {
        let mut writer =
            Writer::create(path, Format::Wav, BitDepth::Float32, &Tags::default(), 48_000)
                .expect("a source file");
        let frames: Vec<[f32; 2]> = (0..48_000)
            .map(|index| {
                let value = (2.0 * std::f32::consts::PI * 440.0 * index as f32 / 48_000.0).sin()
                    * amplitude;
                [value, value]
            })
            .collect();
        writer.write(&frames).expect("the tone");
        writer.finish().expect("a finished source");
    }

    #[test]
    fn an_album_is_written_in_order_and_judged_as_one_programme() {
        let dir = scratch("album");
        let first = dir.join("01 First.wav");
        let second = dir.join("02 Second.wav");
        write_tone_at(&first, 0.5);
        write_tone_at(&second, 0.08);
        let first_master = dir.join("01 First (mastered).wav");
        let second_master = dir.join("02 Second (mastered).wav");
        let state = RenderState::new();
        let status = state
            .start_album(
                ChainConfig::default(),
                vec![
                    Job {
                        source: first.clone(),
                        destination: first_master.clone(),
                        title: "01 First".into(),
                    },
                    Job {
                        source: second.clone(),
                        destination: second_master.clone(),
                        title: "02 Second".into(),
                    },
                ],
                BitDepth::Sixteen,
                Format::Wav,
                SampleRate::Hz48000,
            )
            .expect("an album render");
        assert_eq!(status.track_total, 2);
        // The worker runs on its own thread; the test waits for its own answer.
        let mut finished = status;
        for _ in 0..400 {
            finished = state.status();
            if !finished.running {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(finished.phase, "done", "{}", finished.error);
        assert_eq!(finished.track_index, 2);
        assert!(first_master.is_file() && second_master.is_file(), "an album track was not written");
        // The quiet track must not be able to hide behind the loud one: the album's
        // loudness sits between the two, because it is one programme's gated figure.
        // The album is one programme, so it is louder than its quiet last track and
        // quieter than a full-scale one — the loud first track is in the figure.
        assert!(
            finished.album_lufs > finished.output.integrated_lufs + 3.0
                && finished.album_lufs < 0.0,
            "album {} LUFS against the last track's {} LUFS",
            finished.album_lufs,
            finished.output.integrated_lufs
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_reference_is_measured_with_the_same_meter_a_master_is() {
        let dir = scratch("reference");
        let source = dir.join("tone.wav");
        write_tone(&source);
        let readings = reference_measure(source.to_string_lossy().to_string()).expect("a reference");
        assert!(readings.blocks > 0, "the reference was not measured");
        // A 0.5-amplitude sine in both channels reads about -6.7 LUFS: each channel
        // is -9 dBFS RMS and the loudness formula sums the pair. Its peak is -6 dBTP.
        assert!(
            (readings.integrated_lufs + 6.7).abs() < 1.5,
            "reference {} LUFS",
            readings.integrated_lufs
        );
        assert!(
            (readings.true_peak_db + 6.0).abs() < 0.5,
            "reference peak {} dBTP",
            readings.true_peak_db
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_bypassed_rack_returns_the_samples_it_was_given() {
        let dir = scratch("null");
        let source = dir.join("tone.wav");
        write_tone(&source);
        let report = null_test(source.to_string_lossy().to_string()).expect("a null test");
        assert!(report.frames > 40_000, "only {} frames were compared", report.frames);
        assert_eq!(report.differing, 0, "{} samples came back changed", report.differing);
        assert!(report.max_difference_db <= -119.0, "residual {} dBFS", report.max_difference_db);
        let _ = std::fs::remove_dir_all(dir);
    }
}
