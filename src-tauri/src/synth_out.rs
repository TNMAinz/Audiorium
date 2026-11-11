//! The synth's clock: its own output stream, its patch book, and the commands
//! the page calls.
//!
//! The instrument does not share the player's stream. The player reads a file and
//! is paced by the decoder; an instrument has to be paced by the device and be
//! given a note before the next buffer is due. Two streams, one rack: the synth's
//! output runs through the same `Chain` the player uses, which is why the EQ, the
//! compressor, the saturator, the room and the limiter are its effects section.
//!
//! A bounce is the same engine rendering offline, through the rack alone — no
//! spatial stage, because a bounce is a file and the master page renders files
//! through the rack alone too. That is what keeps a bounce and a master comparable.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

use crate::chain::{Chain, ChainState};
use crate::export::{BitDepth, Format, Tags, Writer};
use crate::spatial::SpatialConfig;
use crate::synth::{self, Event, Synth, SynthConfig, RATE};

const FRAME_BYTES: usize = 8;
/// The device is asked for a short buffer: an instrument has to answer a key
/// press, not fill a queue. Ten milliseconds is the largest that still feels
/// immediate, and shared mode mixes it with everything else on the machine.
const BUFFER_HNS: i64 = 100_000;
const MAX_RECORDED_EVENTS: usize = 200_000;
/// The display's own width: one column per pixel across a 96 px trace would be
/// more than the page can draw, and fewer than this reads as a flicker.
const SCOPE_COLUMNS: usize = 96;
/// 24 frames a column is half a millisecond at 48 kHz, so the whole trace is one
/// 48 ms window of the instrument's output — the shape of a note, not a still frame.
const SCOPE_DECIMATION: usize = 24;
/// How often a running stream publishes its trace and peak. A third of the window,
/// so consecutive traces overlap and the display reads as movement instead of a
/// slideshow: at the old 80 ms a four-frame note redrew six times, which is what
/// "laggy" was.
const PUBLISH_MS: u64 = 30;

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SynthStatus {
    pub running: bool,
    pub voices: usize,
    pub peak_db: f32,
    pub device: String,
    pub midi: String,
    pub recording: bool,
    pub recorded_seconds: f32,
    pub error: Option<String>,
    /// The output's own trace, oldest column first, as low/high pairs. The page
    /// draws what the device was fed, not a curve it made up from the patch.
    pub scope: Vec<f32>,
}

/// A peak-held trace of the frames written to the device. Each column keeps the
/// lowest and highest frame of its window, so a waveform that is narrower than a
/// pixel still shows its full height instead of aliasing into a dull line.
#[derive(Clone)]
struct Scope {
    columns: [(f32, f32); SCOPE_COLUMNS],
    write: usize,
    low: f32,
    high: f32,
    count: usize,
}

impl Scope {
    fn new() -> Self {
        Self {
            columns: [(0.0, 0.0); SCOPE_COLUMNS],
            write: 0,
            low: 0.0,
            high: 0.0,
            count: 0,
        }
    }

    fn push(&mut self, frame: [f32; 2]) {
        self.low = self.low.min(frame[0].min(frame[1]));
        self.high = self.high.max(frame[0].max(frame[1]));
        self.count += 1;
        if self.count < SCOPE_DECIMATION {
            return;
        }
        self.columns[self.write] = (self.low, self.high);
        self.write = (self.write + 1) % SCOPE_COLUMNS;
        self.low = 0.0;
        self.high = 0.0;
        self.count = 0;
    }

    /// Oldest first, so the page draws left to right without knowing the buffer wrapped.
    fn read(&self) -> Vec<f32> {
        let mut trace = Vec::with_capacity(SCOPE_COLUMNS * 2);
        for offset in 0..SCOPE_COLUMNS {
            let (low, high) = self.columns[(self.write + offset) % SCOPE_COLUMNS];
            trace.push(low);
            trace.push(high);
        }
        trace
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchSummary {
    pub id: String,
    pub name: String,
    pub builtin: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchList {
    pub patch: SynthConfig,
    pub active: Option<String>,
    pub presets: Vec<PatchSummary>,
    /// The tables an oscillator can be set to, in the order the engine indexes
    /// them: the page names them instead of printing an index.
    pub tables: Vec<String>,
    pub mpe: crate::midi::MpeConfig,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedPatch {
    id: String,
    name: String,
    patch: SynthConfig,
}

/// What `synth.json` holds: the patch in the panel, which preset it came from,
/// and the presets this user saved.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchBook {
    patch: SynthConfig,
    active: Option<String>,
    #[serde(default)]
    saved: Vec<SavedPatch>,
    #[serde(default)]
    mpe: crate::midi::MpeConfig,
}

impl Default for PatchBook {
    fn default() -> Self {
        Self {
            patch: SynthConfig::default(),
            active: Some("builtin:init".into()),
            saved: Vec::new(),
            mpe: crate::midi::MpeConfig::default(),
        }
    }
}

/// `%APPDATA%\AUDIORIUM\synth.json`, beside the chain's `state.json`. The two are
/// separate files because they are written by different parts of the app: a knob
/// drag on the synth may not rewrite the rack, and vice versa.
fn patch_path() -> Option<PathBuf> {
    crate::store::config_path().map(|path| path.with_file_name("synth.json"))
}

fn write_book(path: &Path, book: &PatchBook) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(book).map_err(|error| error.to_string())?;
    let mut temporary = path.to_path_buf();
    temporary.set_extension("json.tmp");
    fs::write(&temporary, text).map_err(|error| error.to_string())?;
    fs::rename(&temporary, path).map_err(|error| error.to_string())
}

/// Writes the book on a thread that keeps only the newest version, the same way
/// the chain's store does: a knob drag must never wait for a disk.
struct BookStore {
    sender: Option<Sender<PatchBook>>,
}

impl BookStore {
    fn off() -> Self {
        Self { sender: None }
    }

    fn at(path: PathBuf) -> Self {
        let (sender, receiver) = mpsc::channel::<PatchBook>();
        let target = path.clone();
        thread::Builder::new()
            .name("audiorium-synth-store".into())
            .spawn(move || {
                while let Ok(mut latest) = receiver.recv() {
                    thread::sleep(Duration::from_millis(90));
                    while let Ok(next) = receiver.try_recv() {
                        latest = next;
                    }
                    if let Err(error) = write_book(&target, &latest) {
                        eprintln!("AUDIORIUM: the synth patch was not saved: {error}");
                    }
                }
            })
            .ok();
        Self {
            sender: Some(sender),
        }
    }

    fn save(&self, book: PatchBook) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(book);
        }
    }
}

/// One event and where in the performance it happened, so a take can be played
/// back frame for frame.
#[derive(Clone, Copy, Debug)]
pub struct TimedEvent {
    pub frame: u64,
    pub event: Event,
}

pub struct SynthState {
    book: Mutex<PatchBook>,
    events: Arc<Mutex<Vec<Event>>>,
    recording: Arc<Mutex<Option<Vec<TimedEvent>>>>,
    /// The last finished take. Stopping the capture keeps what was played, so a
    /// bounce is the performance rather than a second guess at it.
    take: Mutex<Vec<TimedEvent>>,
    status: Mutex<SynthStatus>,
    stop: Arc<AtomicBool>,
    /// True while a stream thread exists. One instrument means one stream: two
    /// would split the same note queue between them and answer with two voices
    /// for one key.
    alive: AtomicBool,
    chain: Arc<ChainState>,
    spatial: Arc<Mutex<SpatialConfig>>,
    store: BookStore,
    midi: Mutex<crate::midi::Link>,
    mpe: Arc<Mutex<crate::midi::MpeConfig>>,
    /// The endpoint the output was opened on, as a device id; empty means Windows'
    /// default. The Devices page names one output for the app, so the instrument
    /// plays to the same one the player does.
    device: Mutex<String>,
}

fn fail(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// `%APPDATA%\AUDIORIUM	ables.json`: the frames the user imported, as harmonic
/// levels rather than samples, so a launch rebuilds the same mip chains.
fn tables_path() -> Option<PathBuf> {
    patch_path().map(|path| path.with_file_name("tables.json"))
}

fn write_tables(contents: &str) -> Result<(), String> {
    let path = tables_path().ok_or("No directory is available for wavetable storage.")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(fail)?;
    }
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, contents).map_err(fail)?;
    fs::rename(&temporary, &path).map_err(fail)
}

/// Puts the user's tables back at launch. A file that is absent or unreadable is
/// simply no tables: the shipped four are the default, not a loss.
fn read_tables() {
    if let Some(path) = tables_path() {
        if let Ok(text) = fs::read_to_string(path) {
            synth::restore_tables(&text);
        }
    }
}

impl SynthState {
    pub fn new(chain: Arc<ChainState>, spatial: Arc<Mutex<SpatialConfig>>) -> Arc<Self> {
        read_tables();
        let path = patch_path();
        let mut book = path
            .as_deref()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str::<PatchBook>(&text).ok())
            .unwrap_or_default();
        if book.mpe.validate().is_err() { book.mpe = crate::midi::MpeConfig::default(); }
        let mpe = Arc::new(Mutex::new(book.mpe));
        let store = path.map(BookStore::at).unwrap_or_else(BookStore::off);
        Arc::new(Self {
            book: Mutex::new(book),
            events: Arc::new(Mutex::new(Vec::new())),
            recording: Arc::new(Mutex::new(None)),
            take: Mutex::new(Vec::new()),
            status: Mutex::new(SynthStatus::default()),
            stop: Arc::new(AtomicBool::new(false)),
            alive: AtomicBool::new(false),
            chain,
            spatial,
            store,
            midi: Mutex::new(crate::midi::Link::default()),
            mpe,
            device: Mutex::new(String::new()),
        })
    }

    fn patch(&self) -> SynthConfig {
        self.book.lock().map(|book| book.patch.clone()).unwrap_or_default()
    }

    fn list(&self) -> PatchList {
        let book = self.book.lock().map(|book| book.clone()).unwrap_or_default();
        let mut presets: Vec<PatchSummary> = synth::builtin_presets()
            .into_iter()
            .map(|(id, name, _)| PatchSummary {
                id: id.into(),
                name: name.into(),
                builtin: true,
            })
            .collect();
        presets.extend(book.saved.iter().map(|saved| PatchSummary {
            id: saved.id.clone(),
            name: saved.name.clone(),
            builtin: false,
        }));
        PatchList {
            patch: book.patch,
            active: book.active,
            presets,
            tables: synth::table_names().into_iter().map(String::from).collect(),
            mpe: book.mpe,
        }
    }

    fn push(&self, event: Event) {
        if let Ok(mut queue) = self.events.lock() {
            // A stuck producer must not grow the queue without bound; the oldest
            // events are the ones a listener has already stopped caring about.
            if queue.len() > 512 {
                queue.drain(..256);
            }
            queue.push(event);
        }
    }

    fn set_status(&self, update: impl FnOnce(&mut SynthStatus)) {
        if let Ok(mut status) = self.status.lock() {
            update(&mut status);
        }
    }
}

// ---------- the output stream ----------

/// The endpoint the instrument plays to: the one chosen on the Devices page, or
/// Windows' default when nothing was chosen. The player resolves its output the
/// same way, so one choice on that page is one output for the whole app.
fn resolve_output(enumerator: &DeviceEnumerator, device_id: &str) -> Result<wasapi::Device, String> {
    if device_id.is_empty() {
        return enumerator
            .get_default_device(&Direction::Render)
            .map_err(|error| format!("No default output: {error}"));
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
    Err("The selected output is unavailable".into())
}

fn run_stream(state: Arc<SynthState>, ready: mpsc::SyncSender<Result<String, String>>) {
    if wasapi::initialize_mta().is_err() {
        let _ = ready.send(Err("COM could not be started for the output.".into()));
        return;
    }
    let enumerator = match DeviceEnumerator::new() {
        Ok(enumerator) => enumerator,
        Err(error) => {
            let _ = ready.send(Err(fail(error)));
            return;
        }
    };
    let chosen = state.device.lock().map(|id| id.clone()).unwrap_or_default();
    let device = match resolve_output(&enumerator, &chosen) {
        Ok(device) => device,
        Err(reason) => {
            let _ = ready.send(Err(reason));
            return;
        }
    };
    let name = device.get_friendlyname().unwrap_or_else(|_| "Output".into());
    let mut client = match device.get_iaudioclient() {
        Ok(client) => client,
        Err(error) => {
            let _ = ready.send(Err(format!("Cannot open the output: {error}")));
            return;
        }
    };
    let format = WaveFormat::new(32, 32, &SampleType::Float, RATE as usize, 2, None);
    let mode = StreamMode::PollingShared {
        autoconvert: true,
        buffer_duration_hns: BUFFER_HNS,
    };
    if let Err(error) = client.initialize_client(&format, &Direction::Render, &mode) {
        let _ = ready.send(Err(format!("Cannot start the instrument output: {error}")));
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
        Ok(size) => (size as usize).max(64),
        Err(error) => {
            let _ = ready.send(Err(fail(error)));
            return;
        }
    };
    if let Err(error) = client.start_stream() {
        let _ = ready.send(Err(format!("Cannot start the instrument stream: {error}")));
        return;
    }

    let mut bytes = vec![0u8; max_frames * FRAME_BYTES];
    let mut synth = Synth::new();
    let mut chain = Chain::new(
        state.spatial.lock().map(|config| *config).unwrap_or_default(),
        state.chain.effective(),
    );
    let mut produced: u64 = 0;
    let mut peak = 0.0f32;
    let mut scope = Scope::new();
    let mut published = Instant::now();
    let mut error: Option<String> = None;

    let _ = ready.send(Ok(name.clone()));
    state.set_status(|status| {
        status.running = true;
        status.device = name;
        status.error = None;
    });

    while !state.stop.load(Ordering::Acquire) {
        let available = match client.get_available_space_in_frames() {
            Ok(frames) => (frames as usize).min(max_frames),
            Err(failure) => {
                error = Some(fail(failure));
                break;
            }
        };
        if available == 0 {
            thread::sleep(Duration::from_millis(2));
            continue;
        }
        let config = state.patch();
        let chain_config = state.chain.effective();
        let spatial = state.spatial.lock().map(|config| *config).unwrap_or_default();

        // The queue is drained once per buffer, not once per frame: a note that
        // arrives mid-buffer is heard at the start of the next one, which is the
        // buffer's own length of latency and nothing more.
        let queued: Vec<Event> = state
            .events
            .lock()
            .map(|mut queue| queue.drain(..).collect())
            .unwrap_or_default();
        if let Ok(mut recording) = state.recording.lock() {
            if let Some(take) = recording.as_mut() {
                for event in &queued {
                    if take.len() < MAX_RECORDED_EVENTS {
                        take.push(TimedEvent {
                            frame: produced,
                            event: *event,
                        });
                    }
                }
            }
        }
        for event in queued { synth.apply_event(event); }

        for slot in bytes[..available * FRAME_BYTES].chunks_exact_mut(FRAME_BYTES) {
            let voice = synth.frame(&config);
            let frame = chain.process(voice, spatial, &chain_config);
            let frame = [frame[0].clamp(-1.0, 1.0), frame[1].clamp(-1.0, 1.0)];
            peak = peak.max(frame[0].abs()).max(frame[1].abs());
            scope.push(frame);
            for (channel, chunk) in slot.chunks_exact_mut(4).enumerate() {
                chunk.copy_from_slice(&frame[channel].to_le_bytes());
            }
        }
        if let Err(failure) = render.write_to_device(available, &bytes[..available * FRAME_BYTES], None) {
            error = Some(fail(failure));
            break;
        }
        produced += available as u64;

        if published.elapsed() >= Duration::from_millis(PUBLISH_MS) {
            published = Instant::now();
            let voices = synth.active_voices();
            let db = if peak > 0.0 {
                (20.0 * peak.log10()).max(-120.0)
            } else {
                -120.0
            };
            // The same decay per second the 80 ms publish had: three publishes now
            // happen in the time two did.
            peak *= 0.83;
            let trace = scope.read();
            state.set_status(|status| {
                status.voices = voices;
                status.peak_db = db;
                status.scope = trace;
            });
        }
        thread::sleep(Duration::from_millis(1));
    }

    let _ = client.stop_stream();
    state.set_status(|status| {
        status.running = false;
        status.voices = 0;
        status.peak_db = -120.0;
        // A stopped instrument has no output, so it has no trace either: the page
        // goes back to drawing the patch rather than the last buffer it saw.
        status.scope.clear();
        if let Some(reason) = error {
            status.error = Some(reason);
        }
    });
}

// ---------- commands ----------

#[tauri::command]
pub fn synth_patch(state: tauri::State<'_, Arc<SynthState>>) -> PatchList {
    state.list()
}

/// The factory packs. They are built from the engine's own seeds rather than read
/// from a file, so the catalogue a page browses cannot drift from the patches the
/// engine plays: an id it shows is an id `synth_apply_preset` resolves.
#[tauri::command]
pub fn synth_packs() -> crate::packs::Catalogue {
    crate::packs::catalogue()
}

#[tauri::command]
pub fn set_synth(
    state: tauri::State<'_, Arc<SynthState>>,
    patch: SynthConfig,
) -> Result<PatchList, String> {
    patch.validate()?;
    {
        let mut book = state.book.lock().map_err(fail)?;
        book.patch = patch;
        // An edit takes the panel off the preset it came from: the patch is now
        // this user's, and naming the shipped patch would be a claim it is not.
        book.active = None;
        state.store.save(book.clone());
    }
    Ok(state.list())
}

#[tauri::command]
pub fn synth_apply_preset(
    state: tauri::State<'_, Arc<SynthState>>,
    id: String,
) -> Result<PatchList, String> {
    let builtin = synth::builtin_presets()
        .into_iter()
        .find(|(preset, _, _)| *preset == id.as_str())
        .map(|(_, _, patch)| patch);
    let patch = match builtin {
        Some(patch) => patch,
        // A pack patch is built from its seed by the same builder that named it.
        None => match crate::packs::find(&id) {
            Some(patch) => patch,
            None => {
                let book = state.book.lock().map_err(fail)?;
                book.saved
                    .iter()
                    .find(|saved| saved.id == id)
                    .map(|saved| saved.patch.clone())
                    .ok_or("That patch is no longer saved.")?
            }
        },
    };
    patch.validate()?;
    {
        let mut book = state.book.lock().map_err(fail)?;
        book.patch = patch;
        book.active = Some(id);
        state.store.save(book.clone());
    }
    Ok(state.list())
}

#[tauri::command]
pub fn synth_save_preset(
    state: tauri::State<'_, Arc<SynthState>>,
    name: String,
) -> Result<PatchList, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("A patch needs a name.".into());
    }
    if name.chars().count() > 40 {
        return Err("A patch name is at most 40 characters.".into());
    }
    let patch = state.patch();
    patch.validate()?;
    let id = format!("saved:{}", epoch_millis());
    {
        let mut book = state.book.lock().map_err(fail)?;
        // Saving a name that is already there replaces it, so a patch keeps one
        // row instead of collecting copies of itself.
        if let Some(existing) = book.saved.iter_mut().find(|saved| saved.name == name) {
            existing.patch = patch;
            book.active = Some(existing.id.clone());
        } else {
            book.saved.push(SavedPatch {
                id: id.clone(),
                name,
                patch,
            });
            book.active = Some(id);
        }
        state.store.save(book.clone());
    }
    Ok(state.list())
}

#[tauri::command]
pub fn synth_delete_preset(
    state: tauri::State<'_, Arc<SynthState>>,
    id: String,
) -> Result<PatchList, String> {
    {
        let mut book = state.book.lock().map_err(fail)?;
        let before = book.saved.len();
        book.saved.retain(|saved| saved.id != id);
        if book.saved.len() == before {
            return Err("A built-in patch is not deleted; it ships with the app.".into());
        }
        if book.active.as_deref() == Some(id.as_str()) {
            book.active = None;
        }
        state.store.save(book.clone());
    }
    Ok(state.list())
}

#[tauri::command]
pub fn synth_start(
    state: tauri::State<'_, Arc<SynthState>>,
    device: Option<String>,
) -> Result<SynthStatus, String> {
    let wanted = device.unwrap_or_default();
    let running = state.status.lock().map(|status| status.running).unwrap_or(false);
    let current = state.device.lock().map(|chosen| chosen.clone()).unwrap_or_default();
    if running {
        // The same endpoint is already open, so asking again changes nothing.
        if current == wanted {
            return state.status.lock().map(|status| status.clone()).map_err(fail);
        }
        // A WASAPI stream cannot be repointed at another endpoint: moving output is
        // closing this stream and opening the chosen one. Notes being held end with
        // the old stream, because there is no longer an output carrying them.
        shutdown(&state);
    }
    open(state.inner(), wanted)
}

/// Closes the output and waits for its thread, so the device it held is free again.
fn shutdown(state: &SynthState) {
    state.stop.store(true, Ordering::Release);
    if let Ok(mut queue) = state.events.lock() {
        queue.push(Event::AllNotesOff);
    }
    let mut waited = 0u32;
    while state.alive.load(Ordering::Acquire) && waited < 600 {
        thread::sleep(Duration::from_millis(20));
        waited += 20;
    }
}

/// Opens the output on `device` and answers with the state once the stream did.
fn open(state: &Arc<SynthState>, device: String) -> Result<SynthStatus, String> {
    // A stream that is closing still owns the device. Waiting for it is what keeps
    // a double press from leaving two readers of one note queue.
    let mut waited = 0u32;
    while state.alive.load(Ordering::Acquire) {
        if waited >= 1_000 {
            return Err("The instrument output is still closing. Try again.".into());
        }
        thread::sleep(Duration::from_millis(20));
        waited += 20;
    }
    if let Ok(mut chosen) = state.device.lock() {
        *chosen = device;
    }
    state.alive.store(true, Ordering::Release);
    state.stop.store(false, Ordering::Release);
    let (ready, wait) = mpsc::sync_channel::<Result<String, String>>(1);
    let running = Arc::clone(state);
    let spawned = thread::Builder::new()
        .name("audiorium-synth".into())
        .spawn(move || {
            run_stream(running.clone(), ready);
            running.alive.store(false, Ordering::Release);
        });
    if let Err(error) = spawned {
        state.alive.store(false, Ordering::Release);
        return Err(fail(error));
    }
    match wait.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(_device)) => {}
        Ok(Err(reason)) => return Err(reason),
        Err(_) => {
            state.stop.store(true, Ordering::Release);
            return Err("The instrument output did not answer.".into());
        }
    }
    state.status.lock().map(|status| status.clone()).map_err(fail)
}

#[tauri::command]
pub fn synth_stop(state: tauri::State<'_, Arc<SynthState>>) -> SynthStatus {
    // The stream closes on its own thread; this waits for it so a stop answers
    // with the state the caller can act on, not the one it just interrupted.
    shutdown(&state);
    snapshot(&state)
}

#[tauri::command]
pub fn synth_status(state: tauri::State<'_, Arc<SynthState>>) -> SynthStatus {
    snapshot(&state)
}

/// The status the page reads: the stream's own report, the MIDI port that is
/// open, and how long the take being recorded is.
fn snapshot(state: &SynthState) -> SynthStatus {
    let mut status = state.status.lock().map(|status| status.clone()).unwrap_or_default();
    status.midi = state
        .midi
        .lock()
        .map(|link| link.device.clone())
        .unwrap_or_default();
    if let Ok(recording) = state.recording.lock() {
        status.recording = recording.is_some();
        status.recorded_seconds = recording
            .as_ref()
            .and_then(|take| match (take.first(), take.last()) {
                (Some(first), Some(last)) => Some((last.frame - first.frame) as f32 / RATE),
                _ => None,
            })
            .or_else(|| {
                // The stored take is already rebased, so its last frame is its length.
                state
                    .take
                    .lock()
                    .ok()
                    .and_then(|take| take.last().map(|event| event.frame as f32 / RATE))
            })
            .unwrap_or(0.0);
    }
    status
}

#[tauri::command]
pub fn synth_note_on(state: tauri::State<'_, Arc<SynthState>>, note: u8, velocity: f32) {
    state.push(Event::NoteOn {
        note: note.min(127),
        velocity: velocity.clamp(0.0, 1.0),
    });
}

#[tauri::command]
pub fn synth_note_off(state: tauri::State<'_, Arc<SynthState>>, note: u8) {
    state.push(Event::NoteOff { note: note.min(127) });
}

#[tauri::command]
pub fn synth_all_notes_off(state: tauri::State<'_, Arc<SynthState>>) {
    state.push(Event::AllNotesOff);
}

#[tauri::command]
pub fn synth_bend(state: tauri::State<'_, Arc<SynthState>>, semitones: f32) {
    state.push(Event::Bend(semitones));
}

#[tauri::command]
pub fn synth_mod_wheel(state: tauri::State<'_, Arc<SynthState>>, amount: f32) {
    state.push(Event::ModWheel(amount));
}

#[tauri::command]
pub fn synth_record(state: tauri::State<'_, Arc<SynthState>>, on: bool) -> SynthStatus {
    if on {
        if let Ok(mut take) = state.take.lock() {
            take.clear();
        }
        if let Ok(mut recording) = state.recording.lock() {
            if recording.is_none() {
                *recording = Some(Vec::new());
            }
        }
    } else {
        // The capture stops; the performance is kept for the bounce.
        let captured = state.recording.lock().ok().and_then(|mut take| take.take());
        if let (Some(captured), Ok(mut take)) = (captured, state.take.lock()) {
            *take = rebase(&captured);
        }
    }
    snapshot(&state)
}

/// The phrase a bounce plays when nothing was recorded: an arpeggio an octave
/// and a fifth up, so a patch with a filter envelope shows what it does.
fn audition() -> Vec<TimedEvent> {
    let mut events = Vec::new();
    let notes = [48u8, 55, 60, 64, 67, 72, 67, 64];
    for (index, note) in notes.iter().enumerate() {
        let at = index as u64 * (RATE as u64 / 4);
        events.push(TimedEvent {
            frame: at,
            event: Event::NoteOn {
                note: *note,
                velocity: 0.85,
            },
        });
        events.push(TimedEvent {
            frame: at + (RATE as u64 * 3) / 4,
            event: Event::NoteOff { note: *note },
        });
    }
    events
}

/// A take starts at its first event, not at the stream's own clock: the capture
/// may begin seconds after the output was started, and a bounce should not open
/// with that silence.
fn rebase(take: &[TimedEvent]) -> Vec<TimedEvent> {
    let base = take.first().map(|event| event.frame).unwrap_or(0);
    take.iter()
        .map(|event| TimedEvent {
            frame: event.frame.saturating_sub(base),
            event: event.event,
        })
        .collect()
}

fn epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0)
}

/// A tuning as the page needs it: a name for the picker, and the cents of offset
/// per key the engine reads. Equal temperament carries 128 zeroes rather than an
/// empty list, so the page can hand any choice straight to the patch.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScaleInfo {
    pub name: String,
    pub cents: Vec<f32>,
}

fn scale_info(scale: crate::scale::Scale) -> ScaleInfo {
    // Equal temperament is held as an empty list, which is how a patch says "no
    // retuning"; the picker still needs a full table so its choice can be handed
    // straight to the patch like any other scale.
    let equal = scale.is_equal();
    let mut cents = scale.cents;
    if equal || cents.is_empty() {
        cents = vec![0.0; 128];
    }
    ScaleInfo {
        name: scale.name,
        cents,
    }
}

/// The tunings the picker offers, equal temperament first.
#[tauri::command]
pub fn synth_scales() -> Vec<ScaleInfo> {
    crate::scale::builtin().into_iter().map(scale_info).collect()
}

/// Reads a Scala file into a tuning. The parser is the whole format: a description,
/// a count, then ratios or cents, with the period as the last interval.
#[tauri::command]
pub fn synth_import_scale(path: String) -> Result<ScaleInfo, String> {
    let text = fs::read_to_string(&path).map_err(|error| format!("Cannot read that scale: {error}"))?;
    crate::scale::parse_scl(&text).map(scale_info)
}

/// Chooses a `.scl` file and reads it, or returns nothing when the picker is
/// cancelled. The picker runs on a blocking thread because a synchronous Tauri
/// command would hold the webview's IPC thread and deadlock the dialog.
#[tauri::command]
pub async fn synth_choose_scale(window: tauri::Window) -> Result<Option<ScaleInfo>, String> {
    let owner = window.hwnd().ok().map(|handle| handle.0 as isize);
    let path = tauri::async_runtime::spawn_blocking(move || {
        crate::library::pick_file(owner, "Choose a Scala tuning", "Scala tuning", &["scl"])
    })
    .await
    .map_err(|_| "The scale picker failed.".to_string())??;
    if path.is_empty() {
        return Ok(None);
    }
    synth_import_scale(path).map(Some)
}

/// Cuts decoded audio into table frames: each frame is exactly one cycle of
/// `TABLE_SAMPLES`, resampled when the file's own frame is another length, and
/// turned into harmonic levels so the engine can band-limit it. A file shorter than
/// a frame is one frame of the whole file.
fn frames_from(samples: &[f32]) -> Vec<Vec<f32>> {
    let size = synth::TABLE_SAMPLES;
    // Rounded up: a decoder can hand back a frame or two short of the file's own
    // length, and the last cycle is still a cycle.
    let frames = if samples.len() < size {
        1
    } else {
        samples.len().div_ceil(size).clamp(1, synth::MAX_FRAMES)
    };
    let span = if samples.len() < size { samples.len() } else { size };
    (0..frames)
        .map(|frame| {
            let start = frame * size;
            let cycle: Vec<f32> = (0..size)
                .map(|index| {
                    let position = start as f32 + index as f32 * span as f32 / size as f32;
                    let low = position.floor() as usize;
                    let high = (low + 1).min(samples.len().saturating_sub(1));
                    let mix = position - low as f32;
                    let a = samples.get(low).copied().unwrap_or(0.0);
                    let b = samples.get(high).copied().unwrap_or(a);
                    a + (b - a) * mix
                })
                .collect();
            cycle
        })
        .collect()
}

/// Reads a wavetable file: one cycle, or a stack of them one after another, the way
/// a wavetable file is laid out. The engine keeps the harmonic levels, so a table
/// can be read at any pitch without aliasing.
fn load_table(path: &Path) -> Result<(String, Vec<Vec<f32>>), String> {
    let mut mono: Vec<f32> = Vec::new();
    let limit = synth::TABLE_SAMPLES * synth::MAX_FRAMES;
    crate::player::decode(path, &mut |block| {
        for frame in block {
            mono.push((frame[0] + frame[1]) * 0.5);
        }
        mono.len() < limit * 4
    })?;
    if mono.is_empty() {
        return Err("That file holds no audio.".into());
    }
    if mono.iter().all(|sample| sample.abs() < 1e-5) {
        return Err("That file is silent, so its cycle has no harmonics.".into());
    }
    let name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Imported table")
        .to_string();
    Ok((name, frames_from(&mono)))
}

/// Imports a wavetable into the engine and returns the list with it in place. The
/// table is appended, so the indices a saved patch already uses keep their meaning.
#[tauri::command]
pub fn synth_import_table(
    state: tauri::State<'_, Arc<SynthState>>,
    path: String,
    name: Option<String>,
) -> Result<PatchList, String> {
    let (found, frames) = load_table(Path::new(&path))?;
    let name = name.filter(|name| !name.trim().is_empty()).unwrap_or(found);
    synth::add_sample_table_and_persist(name, frames, write_tables)?;
    Ok(state.list())
}

/// Chooses a wavetable file, imports it, and answers with the list; the page reads
/// the last name as the table it just added.
#[tauri::command]
pub async fn synth_choose_table(
    state: tauri::State<'_, Arc<SynthState>>,
    window: tauri::Window,
) -> Result<Option<PatchList>, String> {
    let owner = window.hwnd().ok().map(|handle| handle.0 as isize);
    let path = tauri::async_runtime::spawn_blocking(move || {
        crate::library::pick_file(
            owner,
            "Choose a wavetable",
            "Wavetable or audio",
            &["wav", "flac", "mp3", "ogg", "aif", "aiff"],
        )
    })
    .await
    .map_err(|_| "The table picker failed.".to_string())??;
    if path.is_empty() {
        return Ok(None);
    }
    synth_import_table(state, path, None).map(Some)
}

/// Retrieve the cycles of an imported table without altering it.
#[tauri::command]
pub fn synth_editable_table(table: usize) -> Result<Vec<Vec<f32>>, String> {
    synth::editable_table(table)
}

/// Apply edited cycles and persist them for the next launch.
#[tauri::command]
pub fn synth_save_table(table: usize, frames: Vec<Vec<f32>>) -> Result<(), String> {
    synth::edit_table_and_persist(table, frames, write_tables)
}

#[tauri::command]
pub fn synth_midi_devices() -> Result<Vec<String>, String> {
    crate::midi::devices()
}

/// One cycle of a table at the patch's morph, for the page's display.
#[tauri::command]
pub fn synth_table_shape(table: u8, morph: f32) -> Vec<f32> {
    synth::table_shape(table, morph, 256)
}

#[tauri::command]
pub fn synth_midi_open(
    state: tauri::State<'_, Arc<SynthState>>,
    port: usize,
) -> Result<SynthStatus, String> {
    let link = crate::midi::open(port, state.events.clone(), state.mpe.clone())?;
    {
        let mut slot = state.midi.lock().map_err(fail)?;
        *slot = link;
    }
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn synth_set_mpe(state: tauri::State<'_, Arc<SynthState>>, settings: crate::midi::MpeConfig) -> Result<PatchList, String> {
    settings.validate()?;
    {
        // MIDI callbacks take the queue before reading settings, so changing both
        // under that lock makes the release precede every event in the new zones.
        let mut queue = state.events.lock().map_err(fail)?;
        let mut book = state.book.lock().map_err(fail)?;
        let mut current = state.mpe.lock().map_err(fail)?;
        *current = settings;
        book.mpe = settings;
        state.store.save(book.clone());
        queue.push(Event::AllNotesOff);
    }
    Ok(state.list())
}

#[tauri::command]
pub fn synth_midi_close(state: tauri::State<'_, Arc<SynthState>>) -> SynthStatus {
    if let Ok(mut link) = state.midi.lock() {
        *link = crate::midi::Link::default();
    }
    snapshot(&state)
}

/// Renders the take heard in the panel — or, when nothing was recorded, the
/// audition phrase — through the rack and writes it into the library as a WAV,
/// where the Library and Master pages can pick it up. Returns the path written.
#[tauri::command]
pub fn synth_bounce(
    state: tauri::State<'_, Arc<SynthState>>,
    root: String,
) -> Result<String, String> {
    if root.trim().is_empty() {
        return Err("Choose a library folder first: a bounce is written into it.".into());
    }
    let patch = state.patch();
    patch.validate()?;
    let stored = state.take.lock().map_err(fail)?.clone();
    let take = if stored.is_empty() { audition() } else { stored };
    let mut events: Vec<(u64, Event)> = take
        .iter()
        .map(|timed| (timed.frame, timed.event))
        .collect();
    let end = events.iter().map(|(frame, _)| *frame).max().unwrap_or(0);
    // A note still held when the take ends is released here, so the file has a
    // tail instead of a cut.
    events.push((end, Event::AllNotesOff));
    let tail = (patch.amp.release.max(patch.filter_env.release) * RATE) as u64;
    let chain_config = state.chain.effective();
    let effects_tail = crate::fx::tail_frames(&chain_config.effects) as u64;
    let room_tail = if chain_config.reverb.enabled { (crate::reverb::reverb_time(chain_config.reverb.decay) * RATE) as u64 } else { 0 };
    let frames = (end + tail + effects_tail + room_tail + RATE as u64 / 4) as usize;
    let voice_frames = synth::render_events(&patch, &events, frames);

    let mut chain = Chain::new(SpatialConfig::default(), chain_config.clone());
    let rendered: Vec<[f32; 2]> = voice_frames
        .into_iter()
        .map(|frame| chain.process_rack(frame, &chain_config))
        .collect();

    let dir = PathBuf::from(root.trim()).join("Synth");
    fs::create_dir_all(&dir).map_err(|error| format!("Cannot write into the library: {error}"))?;
    let path = dir.join(format!("Synth take {}.wav", epoch_millis()));
    let tags = Tags {
        title: "Synth take".into(),
        artist: "AUDIORIUM".into(),
        ..Tags::default()
    };
    let mut writer = Writer::create(&path, Format::Wav, BitDepth::Float32, &tags, RATE as u32)?;
    writer.write(&rendered)?;
    writer.finish()?;
    Ok(path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scope_column_waits_until_its_window_is_full() {
        let mut scope = Scope::new();
        for _ in 0..SCOPE_DECIMATION - 1 {
            scope.push([0.5, 0.5]);
        }
        assert!(scope.read().iter().all(|value| *value == 0.0));
        scope.push([0.25, -0.75]);
        let trace = scope.read();
        // The newest column is the last pair: the low frame first, then the high. The
        // half that filled the window before it is the high one.
        assert_eq!(trace[trace.len() - 2], -0.75);
        assert_eq!(trace[trace.len() - 1], 0.5);
    }

    #[test]
    fn a_full_scope_reports_its_oldest_column_first() {
        let mut scope = Scope::new();
        for column in 0..(SCOPE_COLUMNS + 3) {
            for _ in 0..SCOPE_DECIMATION {
                scope.push([column as f32, column as f32]);
            }
        }
        let trace = scope.read();
        assert_eq!(trace.len(), SCOPE_COLUMNS * 2);
        // Each column is a low/high pair, and a column written from a positive signal
        // only keeps zero as its floor. 99 columns into 96 places: the oldest is 3.
        assert_eq!(trace[0], 0.0);
        assert_eq!(trace[1], 3.0);
        assert_eq!(trace[trace.len() - 1], (SCOPE_COLUMNS + 2) as f32);
    }

    #[test]
    fn a_silent_instrument_reads_as_a_flat_line() {
        let mut scope = Scope::new();
        for _ in 0..(SCOPE_COLUMNS * SCOPE_DECIMATION) {
            scope.push([0.0, 0.0]);
        }
        assert!(scope.read().iter().all(|value| *value == 0.0));
    }

    #[test]
    fn a_column_keeps_the_peak_of_the_window_it_covered() {
        let mut scope = Scope::new();
        scope.push([0.9, -0.2]);
        for _ in 0..SCOPE_DECIMATION - 1 {
            scope.push([0.0, 0.0]);
        }
        let trace = scope.read();
        assert_eq!(trace[trace.len() - 2], -0.2);
        assert_eq!(trace[trace.len() - 1], 0.9);
    }

    #[test]
    fn the_patch_book_survives_a_rewrite() {
        let stamp = epoch_millis();
        let path = std::env::temp_dir().join(format!("audiorium-synth-{stamp}.json"));
        let mut book = PatchBook::default();
        book.patch.filter.cutoff = 880.0;
        book.active = Some("builtin:pad".into());
        book.mpe = crate::midi::MpeConfig { lower_members: 7, upper_members: 4, member_bend: 24, master_bend: 3 };
        write_book(&path, &book).expect("the patch is written");
        let text = fs::read_to_string(&path).expect("the file is there");
        let loaded: PatchBook = serde_json::from_str(&text).expect("the file is read");
        assert_eq!(loaded.patch, book.patch);
        assert_eq!(loaded.active.as_deref(), Some("builtin:pad"));
        assert_eq!(loaded.mpe, book.mpe);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_missing_patch_file_falls_back_to_the_init_patch() {
        let book = PatchBook::default();
        assert_eq!(book.patch, SynthConfig::default());
        assert_eq!(book.active.as_deref(), Some("builtin:init"));
    }

    #[test]
    fn a_take_starts_at_its_first_note() {
        let take = [
            TimedEvent { frame: 96_000, event: Event::NoteOn { note: 60, velocity: 1.0 } },
            TimedEvent { frame: 144_000, event: Event::NoteOff { note: 60 } },
        ];
        let rebased = rebase(&take);
        assert_eq!(rebased[0].frame, 0);
        assert_eq!(rebased[1].frame, 48_000);
        assert_eq!(rebase(&[]).len(), 0);
    }

    #[test]
    fn the_audition_is_musical_and_finite() {
        let events = audition();
        assert!(events.len() >= 8, "the phrase is too short to hear a patch");
        let frames = events.last().unwrap().frame as usize + 24_000;
        let rendered = synth::render_events(&SynthConfig::default(), &events.iter().map(|e| (e.frame, e.event)).collect::<Vec<_>>(), frames);
        assert!(rendered.iter().flatten().any(|value| value.abs() > 0.01));
        assert!(rendered.iter().flatten().all(|value| value.is_finite()));
    }
}

/// The table import, tested through the same helpers the command uses: a file cut
/// into cycles, and each cycle turned into the harmonics the engine stores.
#[cfg(test)]
mod import_tests {
    use super::*;

    /// One cycle of a sine at `harmonic` over `len` samples, at `amplitude`.
    fn cycle(len: usize, harmonic: f32, amplitude: f32) -> Vec<f32> {
        (0..len)
            .map(|index| {
                amplitude * (std::f32::consts::TAU * harmonic * index as f32 / len as f32).sin()
            })
            .collect()
    }

    /// Two whole cycles, one after another, the way a table file is laid out.
    fn two_cycles(harmonic: f32, amplitude: f32) -> Vec<f32> {
        let mut samples = cycle(synth::TABLE_SAMPLES, harmonic, amplitude);
        samples.extend(cycle(synth::TABLE_SAMPLES, harmonic, amplitude));
        samples
    }

    /// A 16-bit PCM mono WAV, written by hand: a 44-byte header and the samples,
    /// which is the plain file every decoder reads without a library to write it.
    fn write_wav(path: &Path, samples: &[f32], rate: u32) {
        let data: Vec<u8> = samples
            .iter()
            .flat_map(|sample| ((sample.clamp(-1.0, 1.0) * 32_767.0) as i16).to_le_bytes())
            .collect();
        let mut bytes = Vec::with_capacity(44 + data.len());
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&((36 + data.len()) as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&data);
        fs::write(path, bytes).expect("the wav is written");
    }

    #[test]
    fn a_file_of_one_cycle_is_one_frame() {
        let frames = frames_from(&cycle(synth::TABLE_SAMPLES, 1.0, 0.9));
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].len(), synth::TABLE_SAMPLES);
        let levels = synth::analyze_cycle(&frames[0], 128);
        assert!(levels[0] > 0.5);
        assert!(levels.iter().skip(1).all(|level| *level < levels[0]));
    }

    #[test]
    fn import_retains_phase_for_the_editor() {
        let samples: Vec<f32> = (0..synth::TABLE_SAMPLES)
            .map(|i| (std::f32::consts::TAU * i as f32 / synth::TABLE_SAMPLES as f32).cos())
            .collect();
        let frames = frames_from(&samples);
        assert!((frames[0][0] - 1.0).abs() < 0.001);
        assert!(frames[0][synth::TABLE_SAMPLES / 4].abs() < 0.001);
    }

    #[test]
    fn a_file_of_two_cycles_is_two_frames() {
        let frames = frames_from(&two_cycles(1.0, 0.9));
        assert_eq!(frames.len(), 2);
        assert!(frames
            .iter()
            .all(|frame| frame.len() == synth::TABLE_SAMPLES && frame.iter().all(|sample| sample.is_finite())));
        let first = synth::analyze_cycle(&frames[0], 1)[0];
        let second = synth::analyze_cycle(&frames[1], 1)[0];
        assert!(first > 0.5);
        assert!((first - second).abs() < 0.01);
    }

    #[test]
    fn a_file_shorter_than_a_frame_is_stretched_into_one() {
        let frames = frames_from(&cycle(600, 1.0, 0.9));
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].len(), synth::TABLE_SAMPLES);
        assert!(frames[0].iter().all(|sample| sample.is_finite()));
        assert!(synth::analyze_cycle(&frames[0], 1)[0] > 0.5);
    }

    #[test]
    fn a_wav_file_is_cut_into_its_cycles_and_named_after_itself() {
        let stamp = epoch_millis();
        let path = std::env::temp_dir().join(format!("audiorium-table-{stamp}.wav"));
        write_wav(&path, &two_cycles(1.0, 0.5), RATE as u32);
        let (name, frames) = load_table(&path).expect("the wav is a table");
        assert_eq!(name, format!("audiorium-table-{stamp}"));
        assert_eq!(frames.len(), 2, "two cycles are two frames");
        assert!(frames.iter().all(|frame| frame.len() == synth::TABLE_SAMPLES));
        assert!(synth::analyze_cycle(&frames[0], 1)[0] > 0.3);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn the_instrument_resolves_the_chosen_output_rather_than_the_default() {
        // Enumeration is real: a test that invented an id would only prove the error
        // path, and the question here is whether a listed endpoint is opened as itself.
        if wasapi::initialize_mta().is_err() {
            return;
        }
        let Ok(enumerator) = DeviceEnumerator::new() else {
            return;
        };
        let Ok(devices) = enumerator.get_device_collection(&Direction::Render) else {
            return;
        };
        let ids: Vec<String> = (&devices)
            .into_iter()
            .filter_map(|device| device.ok()?.get_id().ok())
            .collect();
        if ids.is_empty() {
            return;
        }
        // No choice is Windows' default, which is a real endpoint on this machine.
        let default = resolve_output(&enumerator, "").expect("the default output should resolve");
        let default_id = default.get_id().unwrap_or_default();
        assert!(!default_id.is_empty(), "the default output has no id");
        // A chosen id is that endpoint, not whatever Windows made default.
        let chosen = resolve_output(&enumerator, &ids[0]).expect("a listed output should resolve");
        assert_eq!(chosen.get_id().unwrap_or_default(), ids[0]);
        // An id that is gone is refused instead of quietly playing to the default.
        let missing = resolve_output(&enumerator, "{00000000-0000-0000-0000-000000000000}");
        assert!(missing.is_err(), "a missing endpoint resolved");
    }

    #[test]
    fn a_silent_file_is_refused_rather_than_made_into_an_empty_table() {
        let stamp = epoch_millis();
        let path = std::env::temp_dir().join(format!("audiorium-silence-{stamp}.wav"));
        write_wav(&path, &vec![0.0; synth::TABLE_SAMPLES], RATE as u32);
        let reason = load_table(&path).expect_err("silence is not a table");
        assert!(reason.contains("silent"), "{reason}");
        let _ = fs::remove_file(&path);
    }
}
