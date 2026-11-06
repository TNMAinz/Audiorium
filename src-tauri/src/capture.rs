//! Measuring what another endpoint carries.
//!
//! Playback measures what the app renders. This is the other source, and it has
//! two shapes: a capture endpoint (a microphone, or the Audiorium device, with
//! another application playing into it), or a **loopback** of a render endpoint —
//! what your speakers, or any output device, are playing right now. Both are read
//! in shared mode at the app's one stream format, Windows converting as needed, so
//! every reading means the same thing whichever source made it.
//!
//! Loopback is how another application's audio reaches the panels without a driver
//! or a change to that application: it keeps playing to its usual output, and this
//! reads that output. Only one source owns the panels, and capture wins a contest,
//! because starting it is a request to see that device.
//!
//! A capture can also be **heard**. The frames read from the endpoint run through the
//! same panner, room, equaliser and limiter playback uses and are written to an
//! output the user picks. That is what makes the app a router: Spotify keeps playing
//! to its own device, Audiorium reads it and plays it somewhere else. The panels keep
//! measuring the raw source, so they show what the input carries rather than the
//! monitor's version of it. Playing a captured output back into that same endpoint
//! would feed back, and is refused with the reason rather than left to ring.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Serialize;
use wasapi::{
    initialize_mta, AudioCaptureClient, AudioClient, AudioRenderClient, Device, DeviceEnumerator,
    Direction, SampleType, StreamMode, WaveFormat,
};

use crate::analysis::{AnalysisState, Ballistics, Meter, Readings, Source};
use crate::chain::{Chain, ChainState};
use crate::spatial::SpatialConfig;

/// The app's one stream format, in the units WASAPI wants.
const RATE: usize = 48_000;
const CHANNELS: usize = 2;
const FRAME_BYTES: usize = CHANNELS * 4;
/// The panels poll every 20 ms; publishing a little faster keeps each poll carrying
/// a window measured since the last one.
const PUBLISH_EVERY: Duration = Duration::from_millis(16);
/// How long to wait when the endpoint has no packet ready.
const IDLE_WAIT: Duration = Duration::from_millis(3);
/// What the monitor's own render stream holds. Long enough to absorb the render
/// thread's polling, short enough that what is heard stays close to the source.
const MONITOR_BUFFER_HNS: i64 = 300_000;
/// Frames handed from the capture thread to the render thread. Beyond this the
/// oldest are dropped, so a stall can never turn into growing delay.
const MONITOR_QUEUE_FRAMES: usize = 4_800;
/// How often the render thread tops the device up.
const MONITOR_POLL: Duration = Duration::from_millis(2);
/// Frames over which a new monitor stream fades in, so it starts on a slope rather
/// than on an arbitrary phase of the source.
const MONITOR_FADE_FRAMES: usize = 480;
/// The same wait, named for the tests that read in a loop.
#[cfg(test)]
const WAIT_BETWEEN_READS: Duration = IDLE_WAIT;

/// What the chosen input is doing right now.
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureStatus {
    /// The read stream is running and its frames are the ones in the panels.
    pub active: bool,
    /// The endpoint being read and the name Windows gives it.
    pub device: String,
    pub device_name: String,
    /// The endpoint is an output read in loopback: what it is playing, not what a
    /// capture endpoint hears.
    pub loopback: bool,
    /// Why the stream ended by itself, when it did.
    pub error: Option<String>,
    /// Whether what is being read is also being heard, and where.
    pub monitor: MonitorStatus,
}

/// The output a running capture is played to. Off is a real state: metering an
/// input without playing it anywhere is how a microphone is read silently.
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorStatus {
    /// A render stream is open and this capture's frames are going to it.
    pub active: bool,
    /// The output's name as Windows gives it, empty when nothing is monitored.
    pub device_name: String,
    /// Why monitoring is not running — a refused loop, or an output that would not
    /// open. The capture itself keeps running when this is set.
    pub error: Option<String>,
}

/// The one running capture, if any. The stop flag is shared with the read thread,
/// which is the only place the device is opened and closed.
pub struct CaptureState {
    stop: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    status: Arc<Mutex<CaptureStatus>>,
    /// The processors a monitored capture passes through — the same ones the
    /// player uses, so what is heard is what this app would render.
    chain: Arc<ChainState>,
    spatial: Arc<Mutex<SpatialConfig>>,
}
impl Default for CaptureState {
    /// In memory and the default renderer: what a test wants, with no settings file.
    fn default() -> Self {
        Self::new(
            Arc::new(ChainState::memory()),
            Arc::new(Mutex::new(SpatialConfig::default())),
        )
    }
}
impl CaptureState {
    pub fn new(chain: Arc<ChainState>, spatial: Arc<Mutex<SpatialConfig>>) -> Self {
        Self {
            stop: Arc::new(Mutex::new(None)),
            status: Arc::new(Mutex::new(CaptureStatus::default())),
            chain,
            spatial,
        }
    }

    pub fn read(&self) -> CaptureStatus {
        self.status.lock().map(|status| status.clone()).unwrap_or_default()
    }

    /// A handle the command can move into a blocking task: the state itself is
    /// borrowed from the caller's request.
    pub fn handle(&self) -> CaptureHandle {
        CaptureHandle {
            stop: self.stop.clone(),
            status: self.status.clone(),
            chain: self.chain.clone(),
            spatial: self.spatial.clone(),
        }
    }

    /// Asks the running capture to stop. The thread notices within a few
    /// milliseconds and closes its own stream.
    pub fn stop(&self) {
        if let Ok(slot) = self.stop.lock() {
            if let Some(flag) = slot.as_ref() {
                flag.store(true, Ordering::Release);
            }
        }
    }
}

/// A capture that has been started, detached from the request that asked for it.
#[derive(Clone)]
pub struct CaptureHandle {
    stop: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    status: Arc<Mutex<CaptureStatus>>,
    chain: Arc<ChainState>,
    spatial: Arc<Mutex<SpatialConfig>>,
}

impl CaptureHandle {
    /// Opens the endpoint on its own thread and answers when the stream has
    /// actually started, so a button press never reports success before the device
    /// agreed. The thread is also the only place the endpoint is closed.
    ///
    /// `monitor` is the output to play what is read to: `None` meters silently, and
    /// an empty id asks for Windows' default output.
    pub fn start(
        &self,
        device_id: String,
        analysis: Arc<AnalysisState>,
        monitor: Option<String>,
    ) -> Result<(), String> {
        // A capture belongs to the endpoint it opened. A new request — another device,
        // another output to listen on — replaces the running one instead of making the
        // user stop and start by hand, and the wait is what keeps two streams from
        // holding the same device at once.
        self.stop_running()?;
        let flag = {
            let mut slot = self
                .stop
                .lock()
                .map_err(|_| "The input capture is unavailable.".to_string())?;
            let flag = Arc::new(AtomicBool::new(false));
            *slot = Some(flag.clone());
            flag
        };
        let (ready, opened) = std::sync::mpsc::channel::<Result<(), String>>();
        let state = self.clone();
        thread::spawn(move || state.run(device_id, analysis, monitor, flag, ready));
        opened
            .recv()
            .unwrap_or_else(|_| Err("The input stopped before it started.".into()))
    }

    /// Asks a running capture to stop and waits for its thread to release the slot.
    /// The thread closes its own streams on the way out, so returning from here means
    /// the device is free.
    fn stop_running(&self) -> Result<(), String> {
        let running = self
            .stop
            .lock()
            .map_err(|_| "The input capture is unavailable.".to_string())?
            .as_ref()
            .cloned();
        let Some(running) = running else {
            return Ok(());
        };
        running.store(true, Ordering::Release);
        let deadline = Instant::now() + Duration::from_millis(1500);
        loop {
            if self.stop.lock().map(|slot| slot.is_none()).unwrap_or(true) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("The running capture did not let go of its device.".into());
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn run(
        &self,
        device_id: String,
        analysis: Arc<AnalysisState>,
        monitor: Option<String>,
        stop: Arc<AtomicBool>,
        ready: Sender<Result<(), String>>,
    ) {
        let mut session = match open(&device_id) {
            Ok(session) => session,
            Err(error) => {
                self.finish(Some(error.clone()));
                let _ = ready.send(Err(error));
                return;
            }
        };
        // Listening is opened after the source, so a monitor that cannot open is
        // reported against a capture that is already reading.
        if let Some(target) = monitor {
            match Monitor::open(&target, self.chain.clone(), self.spatial.clone()) {
                Ok(mut monitor) => {
                    if feeds_back(session.loopback, &session.device_id, &monitor.device_id) {
                        monitor.close();
                        session.monitor_error = Some(
                            "Monitoring is off: playing this output back into itself would feed \
                             back. Choose another output to hear it on."
                                .into(),
                        );
                    } else {
                        session.monitor = Some(monitor);
                    }
                }
                Err(error) => session.monitor_error = Some(error),
            }
        }
        if let Ok(mut status) = self.status.lock() {
            status.active = true;
            status.device = session.device_id.clone();
            status.device_name = session.device_name.clone();
            status.loopback = session.loopback;
            status.error = None;
            status.monitor = session.monitor_status();
        }
        analysis.claim(Source::Input);
        let _ = ready.send(Ok(()));

        let mut meter = Meter::new();
        let mut ballistics = Ballistics::new();
        let mut last = Instant::now();
        let mut failure = None;
        while !stop.load(Ordering::Acquire) {
            match session.read(&mut meter) {
                // Nothing was waiting: the endpoint is quiet, which is a moment,
                // not a failure.
                Ok(0) => thread::sleep(IDLE_WAIT),
                Ok(_) => {}
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
            if last.elapsed() >= PUBLISH_EVERY {
                let since = last.elapsed().as_secs_f32();
                last = Instant::now();
                let readings = Readings {
                    source: Source::Input.label().to_string(),
                    ..meter.readings()
                };
                analysis.publish(ballistics.apply(readings, since));
                // A monitor that stopped mid-capture is reported here, on the poll
                // the page is already reading, without ending the capture.
                if let Ok(mut status) = self.status.lock() {
                    status.monitor = session.monitor_status();
                }
            }
        }
        session.close();
        // The panels must not keep a level from a device that stopped answering.
        analysis.idle(Source::Input);
        self.finish(failure);
    }

    /// One exit: no stream, no level, and the slot freed for the next start.
    fn finish(&self, error: Option<String>) {
        if let Ok(mut status) = self.status.lock() {
            status.active = false;
            status.device.clear();
            status.device_name.clear();
            status.loopback = false;
            status.error = error;
            status.monitor = MonitorStatus::default();
        }
        if let Ok(mut slot) = self.stop.lock() {
            *slot = None;
        }
    }
}

/// An open endpoint, its buffer, and the names it reports.
struct Session {
    client: AudioClient,
    capture: AudioCaptureClient,
    buffer: Vec<u8>,
    device_id: String,
    device_name: String,
    /// The endpoint is an output, so the stream is Windows' loopback of it.
    loopback: bool,
    /// The output this capture is played to, when the user asked to hear it.
    monitor: Option<Monitor>,
    /// Why monitoring is not running, when it is not: a refused loop, or a stream
    /// that would not open. The capture is unaffected either way.
    monitor_error: Option<String>,
}

impl Session {
    /// Reads every packet the endpoint has queued and returns how many frames were
    /// measured.
    fn read(&mut self, meter: &mut Meter) -> Result<usize, String> {
        self.pump(meter, None)
    }

    /// The same read, keeping the frames as they arrived: how a hole in what comes
    /// back is measured rather than guessed at.
    #[cfg(test)]
    fn read_into(&mut self, meter: &mut Meter, out: &mut Vec<[f32; 2]>) -> Result<usize, String> {
        self.pump(meter, Some(out))
    }

    fn pump(
        &mut self,
        meter: &mut Meter,
        mut out: Option<&mut Vec<[f32; 2]>>,
    ) -> Result<usize, String> {
        let mut measured = 0usize;
        loop {
            match self.capture.get_next_packet_size().map_err(fail)? {
                None | Some(0) => break,
                Some(_) => {
                    let (frames, _info) = self
                        .capture
                        .read_from_device(&mut self.buffer[..])
                        .map_err(fail)?;
                    if frames == 0 {
                        break;
                    }
                    let bytes = &self.buffer[..frames as usize * FRAME_BYTES];
                    measured += push_frames(bytes, meter)?;
                    if let Some(out) = out.as_deref_mut() {
                        for frame in bytes.chunks_exact(FRAME_BYTES) {
                            out.push([
                                f32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]),
                                f32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]),
                            ]);
                        }
                    }
                    // Handing frames over never blocks and never writes the device:
                    // the monitor's own thread owns the render stream.
                    if let Some(monitor) = self.monitor.as_ref() {
                        monitor.feed(bytes);
                    }
                }
            }
        }
        Ok(measured)
    }

    /// The monitor's state, after dropping a render thread that ended by itself —
    /// an unplugged output, or a device taken into exclusive use elsewhere.
    fn monitor_status(&mut self) -> MonitorStatus {
        if let Some(error) = self.monitor.as_ref().and_then(|monitor| monitor.failure()) {
            if let Some(mut monitor) = self.monitor.take() {
                monitor.close();
            }
            self.monitor_error = Some(error);
        }
        MonitorStatus {
            active: self.monitor.is_some(),
            device_name: self
                .monitor
                .as_ref()
                .map(|monitor| monitor.device_name.clone())
                .unwrap_or_default(),
            error: self.monitor_error.clone(),
        }
    }

    fn close(&mut self) {
        if let Some(mut monitor) = self.monitor.take() {
            monitor.close();
        }
        let _ = self.client.stop_stream();
    }
}

/// A captured frame played to an output.
///
/// It is the same path playback takes — spatial stage, panner, room, equaliser,
/// limiter — so the app renders what it hears the way it renders a file, and the
/// limiter is what keeps a loud source from clipping the output it is sent to.
///
/// The capture thread only ever **queues** frames. Its own thread owns the render
/// stream and keeps the device fed, writing silence when the queue runs dry, so a
/// late or missing capture packet is a short gap rather than an underrun — which
/// is what a click or a burst of static actually is.
struct Monitor {
    device_id: String,
    device_name: String,
    stop: Arc<AtomicBool>,
    queue: Arc<Mutex<VecDeque<[f32; 2]>>>,
    /// Why the render thread ended, when it ended by itself.
    failure: Arc<Mutex<Option<String>>>,
    thread: Option<JoinHandle<()>>,
}

impl Monitor {
    /// Opens the output by id, or Windows' default output when the id is empty.
    fn open(
        device_id: &str,
        chain_state: Arc<ChainState>,
        spatial: Arc<Mutex<SpatialConfig>>,
    ) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let failure = Arc::new(Mutex::new(None));
        let (ready, opened) = mpsc::channel::<Result<(String, String), String>>();
        let target = device_id.to_string();
        let thread = {
            let (queue, stop, failure) = (queue.clone(), stop.clone(), failure.clone());
            thread::spawn(move || {
                monitor_stream(target, queue, stop, failure, chain_state, spatial, ready)
            })
        };
        // The stream is opened on its own thread — WASAPI's interfaces belong to the
        // thread that opened them — so this waits for that thread's answer rather than
        // reporting a stream that does not exist yet.
        match opened.recv() {
            Ok(Ok((id, name))) => Ok(Self {
                device_id: id,
                device_name: name,
                stop,
                queue,
                failure,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                let _ = thread.join();
                Err("The monitor output stopped before it started.".into())
            }
        }
    }

    /// Hands the frames just read to the render thread. Frames beyond the queue's
    /// bound are dropped oldest-first, so a stall can never build up delay.
    fn feed(&self, frame_bytes: &[u8]) {
        let Ok(mut queue) = self.queue.lock() else {
            return;
        };
        for frame in frame_bytes.chunks_exact(FRAME_BYTES) {
            queue.push_back([
                f32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]),
                f32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]),
            ]);
        }
        while queue.len() > MONITOR_QUEUE_FRAMES {
            queue.pop_front();
        }
    }

    fn failure(&self) -> Option<String> {
        self.failure.lock().ok().and_then(|failure| failure.clone())
    }

    fn close(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.close();
    }
}

/// The monitor's own render thread: one stream, opened and closed here, fed from the
/// queue and topped up with silence whenever the queue has less than the device's
/// free space.
fn monitor_stream(
    target: String,
    queue: Arc<Mutex<VecDeque<[f32; 2]>>>,
    stop: Arc<AtomicBool>,
    failure: Arc<Mutex<Option<String>>>,
    chain_state: Arc<ChainState>,
    spatial: Arc<Mutex<SpatialConfig>>,
    ready: Sender<Result<(String, String), String>>,
) {
    let opened = open_render_stream(&target);
    let (client, render, max_frames, id, name) = match opened {
        Ok(opened) => opened,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let starting = spatial.lock().map(|config| *config).unwrap_or_default();
    let mut chain = Chain::new(starting, chain_state.effective());
    let mut bytes = vec![0u8; max_frames * FRAME_BYTES];
    let _ = ready.send(Ok((id, name)));
    // A stream that starts on an arbitrary phase of the source clicks; the first few
    // milliseconds of this one are a ramp rather than a step.
    let step = 1.0 / MONITOR_FADE_FRAMES as f32;
    let mut gain = 0.0f32;
    while !stop.load(Ordering::Acquire) {
        let space = match client.get_available_space_in_frames() {
            Ok(space) => (space as usize).min(max_frames),
            Err(error) => {
                set_failure(&failure, format!("The monitor output stopped: {error}"));
                break;
            }
        };
        if space == 0 {
            thread::sleep(MONITOR_POLL);
            continue;
        }
        let config = chain_state.effective();
        let current = spatial.lock().map(|config| *config).unwrap_or_default();
        let mut queued = match queue.lock() {
            Ok(queued) => queued,
            Err(_) => break,
        };
        for slot in bytes[..space * FRAME_BYTES].chunks_exact_mut(FRAME_BYTES) {
            // A starved queue is silence, never a repeat or a leftover: a gap in the
            // capture is a gap in what is heard, not a reason to loop a sample.
            let input = queued.pop_front().unwrap_or([0.0; 2]);
            let processed = chain.process(input, current, &config);
            gain = (gain + step).min(1.0);
            slot[0..4].copy_from_slice(&(processed[0].clamp(-1.0, 1.0) * gain).to_le_bytes());
            slot[4..8].copy_from_slice(&(processed[1].clamp(-1.0, 1.0) * gain).to_le_bytes());
        }
        drop(queued);
        if let Err(error) = render.write_to_device(space, &bytes[..space * FRAME_BYTES], None) {
            set_failure(&failure, format!("The monitor output stopped: {error}"));
            break;
        }
        thread::sleep(MONITOR_POLL);
    }
    let _ = client.stop_stream();
}

fn set_failure(slot: &Arc<Mutex<Option<String>>>, error: String) {
    if let Ok(mut slot) = slot.lock() {
        *slot = Some(error);
    }
}

/// The render stream, its buffer and the names it reports: opened on the thread that
/// will own it.
fn open_render_stream(
    device_id: &str,
) -> Result<(AudioClient, AudioRenderClient, usize, String, String), String> {
    initialize_mta().ok().map_err(fail)?;
    let device = resolve_render(device_id)?;
    let name = device
        .get_friendlyname()
        .unwrap_or_else(|_| "Output".into());
    let id = device.get_id().map_err(fail)?;
    let mut client = device
        .get_iaudioclient()
        .map_err(|error| format!("Cannot open the monitor output: {error}"))?;
    let format = WaveFormat::new(32, 32, &SampleType::Float, RATE, CHANNELS, None);
    let mode = StreamMode::PollingShared {
        autoconvert: true,
        buffer_duration_hns: MONITOR_BUFFER_HNS,
    };
    client
        .initialize_client(&format, &Direction::Render, &mode)
        .map_err(|error| {
            format!("Cannot open the monitor output (it may be in exclusive use): {error}")
        })?;
    let render = client.get_audiorenderclient().map_err(fail)?;
    let max_frames = client.get_buffer_size().map_err(fail)? as usize;
    // A render stream started on an empty buffer underruns before its first block;
    // priming it with silence is what the player does.
    let silence = vec![0u8; max_frames * FRAME_BYTES];
    render
        .write_to_device(max_frames, &silence, None)
        .map_err(|error| format!("Cannot prime the monitor output: {error}"))?;
    client
        .start_stream()
        .map_err(|error| format!("Cannot start the monitor output: {error}"))?;
    Ok((client, render, max_frames, id, name))
}

/// Whether listening would close a loop: a captured output played back into the
/// same output is read again by the capture, and the gain around that ring is one.
fn feeds_back(loopback: bool, captured: &str, monitored: &str) -> bool {
    loopback && captured == monitored
}

/// An output endpoint by id, or Windows' default output.
fn resolve_render(device_id: &str) -> Result<Device, String> {
    let enumerator = DeviceEnumerator::new().map_err(fail)?;
    if device_id.is_empty() {
        return enumerator
            .get_default_device(&Direction::Render)
            .map_err(|error| format!("The monitor output is unavailable: {error}"));
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
    Err("The chosen monitor output is not connected".into())
}

/// Interleaved little-endian f32 stereo frames, exactly as the stream delivers
/// them, appended to the meter. Split out so the conversion is testable without a
/// device.
fn push_frames(bytes: &[u8], meter: &mut Meter) -> Result<usize, String> {
    if bytes.len() % FRAME_BYTES != 0 {
        return Err("The input delivered a partial frame.".into());
    }
    for frame in bytes.chunks_exact(FRAME_BYTES) {
        meter.push([
            f32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]),
            f32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]),
        ]);
    }
    Ok(bytes.len() / FRAME_BYTES)
}

fn fail(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// The endpoint by id, or Windows' default input when nothing was chosen.
fn open(device_id: &str) -> Result<Session, String> {
    initialize_mta().ok().map_err(fail)?;
    let enumerator = DeviceEnumerator::new().map_err(fail)?;
    let (device, loopback) = resolve(&enumerator, device_id)?;
    let name = device
        .get_friendlyname()
        .unwrap_or_else(|_| "Input".into());
    let id = device.get_id().map_err(fail)?;
    let mut client = device
        .get_iaudioclient()
        .map_err(|error| format!("Cannot open the chosen input: {error}"))?;
    // The app measures at one format. Windows converts the endpoint's own mix
    // format into it, so a 44.1 kHz microphone and a 48 kHz virtual device are
    // both measured as the same stream.
    let format = WaveFormat::new(32, 32, &SampleType::Float, RATE, CHANNELS, None);
    let mode = StreamMode::PollingShared {
        autoconvert: true,
        buffer_duration_hns: 200_000,
    };
    client
        .initialize_client(&format, &Direction::Capture, &mode)
        .map_err(|error| {
            format!("Cannot open the chosen input (it may be in exclusive use): {error}")
        })?;
    let capture = client.get_audiocaptureclient().map_err(fail)?;
    let buffer_frames = client.get_buffer_size().map_err(fail)?.max(2048) as usize;
    let buffer = vec![0u8; buffer_frames * FRAME_BYTES];
    client
        .start_stream()
        .map_err(|error| format!("Cannot start the chosen input: {error}"))?;
    Ok(Session {
        client,
        capture,
        buffer,
        device_id: id,
        device_name: name,
        loopback,
        monitor: None,
        monitor_error: None,
    })
}

/// The endpoint by id, or Windows' default input when nothing was chosen.
///
/// An id that is not a capture endpoint is looked for among the outputs: opening a
/// render endpoint's client and initialising it for capture is what Windows calls
/// loopback, and it is how the audio of an application that never changed its own
/// output reaches the panels. The second value says which of the two it was.
fn resolve(enumerator: &DeviceEnumerator, device_id: &str) -> Result<(wasapi::Device, bool), String> {
    if device_id.is_empty() {
        let device = enumerator
            .get_default_device(&Direction::Capture)
            .map_err(fail)?;
        return Ok((device, false));
    }
    // A capture endpoint first: that is what an input id means.
    for direction in [Direction::Capture, Direction::Render] {
        let devices = enumerator.get_device_collection(&direction).map_err(fail)?;
        for device in &devices {
            let device = device.map_err(fail)?;
            if device.get_id().map_err(fail)? == device_id {
                return Ok((device, direction == Direction::Render));
            }
        }
    }
    Err("The chosen source is not connected".into())
}

#[tauri::command]
pub async fn start_input_capture(
    state: tauri::State<'_, CaptureState>,
    analysis: tauri::State<'_, Arc<AnalysisState>>,
    device_id: String,
    monitor: bool,
    monitor_device: String,
) -> Result<(), String> {
    let capture = state.handle();
    let analysis = analysis.inner().clone();
    // Listening is a choice made before the stream starts: an output that would
    // open is not the same as one that should.
    let target = monitor.then_some(monitor_device);
    // Opening a device blocks until the driver answers, which is not the
    // interface's thread to spend.
    tauri::async_runtime::spawn_blocking(move || capture.start(device_id, analysis, target))
        .await
        .map_err(|error| format!("The input capture task failed: {error}"))?
}

#[tauri::command]
pub fn stop_input_capture(state: tauri::State<'_, CaptureState>) -> CaptureStatus {
    state.stop();
    state.read()
}

#[tauri::command]
pub fn capture_status(state: tauri::State<'_, CaptureState>) -> CaptureStatus {
    state.read()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stereo sine as the capture stream delivers it: f32 little-endian pairs.
    fn tone_bytes(frequency: f32, amplitude: f32, frames: usize) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(frames * FRAME_BYTES);
        for index in 0..frames {
            let value = amplitude
                * (2.0 * std::f32::consts::PI * frequency * index as f32 / RATE as f32).sin();
            for _ in 0..CHANNELS {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn the_stream_is_measured_as_the_frames_it_carries() {
        let mut meter = Meter::new();
        let frames = push_frames(&tone_bytes(1_000.0, 0.5, 4800), &mut meter).unwrap();
        assert_eq!(frames, 4800);
        let readings = meter.readings();
        // A 0.5 amplitude sine has an RMS of 0.5/√2, measured from the bytes as
        // they arrived, in both channels.
        assert!((readings.rms - 0.3536).abs() < 0.02, "rms {}", readings.rms);
        assert!((readings.peak - 0.5).abs() < 0.02, "peak {}", readings.peak);
        assert!(readings.spectrum.iter().cloned().fold(0.0f32, f32::max) > 0.8);
    }

    #[test]
    fn silence_reads_as_the_floor_not_as_a_level() {
        let mut meter = Meter::new();
        push_frames(&vec![0u8; FRAME_BYTES * 4800], &mut meter).unwrap();
        let readings = meter.readings();
        assert_eq!(readings.rms, 0.0);
        assert_eq!(readings.peak, 0.0);
    }

    #[test]
    fn a_partial_frame_is_refused_rather_than_read_as_noise() {
        let mut meter = Meter::new();
        assert!(push_frames(&[0u8; FRAME_BYTES - 1], &mut meter).is_err());
    }

    #[test]
    fn an_idle_engine_reports_no_stream() {
        let state = CaptureState::default();
        let status = state.read();
        assert!(!status.active);
        assert!(status.device.is_empty());
        assert!(status.device_name.is_empty());
        assert!(!status.loopback);
        assert!(status.error.is_none());
        assert!(!status.monitor.active);
        assert!(status.monitor.error.is_none());
    }

    /// Playing a captured output back into that same endpoint closes the ring, and
    /// the ring's gain is one: it must be refused, not left to build up.
    #[test]
    fn listening_to_the_source_is_refused_and_any_other_output_is_not() {
        let speakers = "{0.0.0.00000000}.{speakers}";
        let headphones = "{0.0.0.00000000}.{headphones}";
        assert!(feeds_back(true, speakers, speakers));
        assert!(!feeds_back(true, speakers, headphones));
        // A capture endpoint has nothing of its own playing, so any output is fair.
        assert!(!feeds_back(false, "{0.0.1.00000000}.{mic}", speakers));
    }

    /// Windows reports output endpoints under a render id and inputs under a
    /// capture id, so an id only resolves in one of the two collections. This is
    /// the split that decides whether a stream is a loopback.
    #[test]
    #[ignore = "enumerates the real audio endpoints"]
    fn an_id_resolves_in_one_direction_and_names_its_shape() {
        initialize_mta().ok().unwrap();
        let enumerator = DeviceEnumerator::new().unwrap();
        let outputs = enumerator.get_device_collection(&Direction::Render).unwrap();
        let inputs = enumerator.get_device_collection(&Direction::Capture).unwrap();
        let output_id = outputs
            .into_iter()
            .next()
            .expect("an output endpoint")
            .unwrap()
            .get_id()
            .unwrap();
        let input_id = inputs
            .into_iter()
            .next()
            .expect("an input endpoint")
            .unwrap()
            .get_id()
            .unwrap();
        let (_, loopback) = resolve(&enumerator, &output_id).unwrap();
        println!("output {output_id} -> loopback {loopback}");
        assert!(loopback, "an output id was not opened as a loopback");
        let (_, loopback) = resolve(&enumerator, &input_id).unwrap();
        assert!(!loopback, "an input id was opened as a loopback");
    }

    /// The whole path the button takes: start the chosen endpoint, let it publish
    /// into the panels, then stop and give the panels back. A quiet room still
    /// publishes frames, so the promise is that the stream runs and is owned, not
    /// that anything is audible.
    #[test]
    #[ignore = "opens a real input endpoint"]
    fn a_running_capture_publishes_and_gives_the_panels_back() {
        let state = CaptureState::default();
        let analysis = Arc::new(AnalysisState::default());
        let handle = state.handle();
        handle
            .start(String::new(), analysis.clone(), None)
            .expect("the default input should open");
        // A second start replaces the running capture instead of opening a second
        // stream: the old thread lets go of the device before the new one opens it.
        handle
            .start(String::new(), analysis.clone(), None)
            .expect("a running capture should be replaced, not refused");
        assert!(state.read().active, "the replacement capture is not running");
        let deadline = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < deadline && !analysis.read().playing {
            thread::sleep(Duration::from_millis(10));
        }
        let readings = analysis.read();
        println!(
            "capture: source {}, rms {:.4}, peak {:.4}, device {}",
            readings.source,
            readings.rms,
            readings.peak,
            state.read().device_name
        );
        assert!(state.read().active, "the capture is not reported as running");
        assert!(readings.playing, "no window was published");
        assert_eq!(readings.source, "input");

        state.stop();
        let deadline = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < deadline && state.read().active {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(!state.read().active, "the capture did not stop");
        assert!(!analysis.read().playing, "a stopped capture kept its level");
    }

    /// The no-driver path: the default output plays a tone, and the loopback of
    /// that output is measured. Nothing in this test asks the operating system for
    /// a virtual device, which is the point — loopback is how another app's audio
    /// reaches the panels with no install at all.
    #[test]
    #[ignore = "plays a tone through the default output"]
    fn a_loopback_capture_reads_what_the_output_plays() {
        initialize_mta().ok().unwrap();
        let enumerator = DeviceEnumerator::new().unwrap();
        let output_id = enumerator
            .get_default_device(&Direction::Render)
            .unwrap()
            .get_id()
            .unwrap();
        let tone = std::env::temp_dir().join("audiorium-capture-tone.wav");
        std::fs::write(&tone, tone_wav(440.0, 0.5, 3)).unwrap();
        // Windows' own player, so the audio goes through the audio engine like any
        // other application's would.
        let mut player = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!("(New-Object Media.SoundPlayer '{}').PlaySync()", tone.display()),
            ])
            .spawn()
            .expect("a player for the tone");
        thread::sleep(Duration::from_millis(400));

        let mut session = open(&output_id).expect("the default output's loopback should open");
        assert!(session.loopback, "an output was not opened as a loopback");
        let mut meter = Meter::new();
        let deadline = Instant::now() + Duration::from_millis(1500);
        let mut frames = 0usize;
        while Instant::now() < deadline {
            frames += session.read(&mut meter).unwrap_or(0);
            thread::sleep(WAIT_BETWEEN_READS);
        }
        let readings = meter.readings();
        session.close();
        let _ = player.kill();
        println!(
            "loopback of {}: {frames} frames, rms {:.4}, peak {:.4}",
            session.device_name, readings.rms, readings.peak
        );
        assert!(frames > 0, "the loopback delivered no frames");
        assert!(readings.rms > 0.01, "the loopback measured only silence");
    }

    /// Choosing another source while a capture runs must move the capture, not
    /// leave the old endpoint measured until the user stops and starts by hand.
    #[test]
    #[ignore = "opens real input and output endpoints"]
    fn another_source_replaces_the_running_capture() {
        initialize_mta().ok().unwrap();
        let enumerator = DeviceEnumerator::new().unwrap();
        let output_id = enumerator
            .get_default_device(&Direction::Render)
            .unwrap()
            .get_id()
            .unwrap();
        let state = CaptureState::default();
        let analysis = Arc::new(AnalysisState::default());
        let handle = state.handle();
        handle
            .start(String::new(), analysis.clone(), None)
            .expect("the default input should open");
        let first = state.read();
        handle
            .start(output_id.clone(), analysis.clone(), None)
            .expect("the new source should replace the running capture");
        let second = state.read();
        println!(
            "{} -> {} (loopback {})",
            first.device_name, second.device_name, second.loopback
        );
        assert!(second.active, "the replacement capture is not running");
        assert_eq!(second.device, output_id, "the capture kept the old endpoint");
        assert!(second.loopback, "the output was not opened as a loopback");
        state.stop();
        let deadline = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < deadline && state.read().active {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(!state.read().active, "the capture did not stop");
    }

    /// The other half of the promise: what a capture reads can be heard. Nothing
    /// else is playing, the monitor writes a tone to Windows' default output, and
    /// that same output's loopback is measured — so the sound really left the app.
    #[test]
    #[ignore = "plays a tone through the default output"]
    fn listening_plays_the_captured_frames_to_an_output() {
        initialize_mta().ok().unwrap();
        let chain = Arc::new(ChainState::memory());
        let spatial = Arc::new(Mutex::new(SpatialConfig::default()));
        let mut monitor = Monitor::open("", chain, spatial).expect("the default output should open");
        let mut loopback = open(&monitor.device_id).expect("that output's loopback should open");
        // Twenty milliseconds at a time, which is what a capture packet carries, so
        // the monitor is fed at the rate the device consumes it.
        let packet = 960 * FRAME_BYTES;
        let tone = tone_bytes(440.0, 0.5, 60 * 960);
        let mut meter = Meter::new();
        let mut heard: Vec<[f32; 2]> = Vec::new();
        let mut frames = 0usize;
        // Fed on a deadline rather than a fixed sleep: a test that falls behind real
        // time would starve the monitor and blame the stream for its own lag.
        let start = Instant::now();
        for (index, chunk) in tone.chunks(packet).enumerate() {
            monitor.feed(chunk);
            frames += loopback.read_into(&mut meter, &mut heard).unwrap_or(0);
            let due = start + Duration::from_millis(20 * (index as u64 + 1));
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
        }
        // What is still in the output's own buffer has to leave it before it can be
        // measured; the monitor's thread keeps the stream running while it drains.
        for _ in 0..20 {
            frames += loopback.read(&mut meter).unwrap_or(0);
            thread::sleep(Duration::from_millis(20));
        }
        let readings = meter.readings();
        let name = monitor.device_name.clone();
        monitor.close();
        loopback.close();
        // A stream that starves or drops frames leaves holes in what comes back: a run
        // of exact silence inside a continuous tone is an underrun, which is what the
        // clicks and bursts of static were. The head is skipped because the output was
        // primed with silence before the first frame was fed.
        // The output was primed with silence and the device has a buffer of its own,
        // so what comes back starts with silence; the tone's first frame is where the
        // measurement of the tone begins.
        let head = heard
            .iter()
            .position(|frame| frame[0] != 0.0 || frame[1] != 0.0)
            .unwrap_or(heard.len());
        let body = &heard[head..];
        let (gap_at, gap) = longest_silence(body);
        println!(
            "monitor on {name}: {frames} frames measured back, head {head} frames ({:.1} ms), rms {:.4}, peak {:.4}, longest gap {gap} frames at {} ({:.1} ms)",
            head as f32 / 48.0,
            readings.rms,
            readings.peak,
            gap_at,
            gap_at as f32 / 48.0
        );
        assert!(frames > 0, "the monitored output delivered no frames");
        assert!(
            readings.rms > 0.01,
            "the monitored output measured only silence"
        );
        assert!(
            gap < 240,
            "the monitored output dropped out for {gap} frames ({:.1} ms)",
            gap as f32 / 48.0
        );
    }

    /// The longest run of exact silence and where it starts, in frames: a dropout
    /// inside a tone.
    fn longest_silence(frames: &[[f32; 2]]) -> (usize, usize) {
        let (mut longest, mut at) = (0usize, 0usize);
        let mut run = 0usize;
        for (index, frame) in frames.iter().enumerate() {
            if frame[0] == 0.0 && frame[1] == 0.0 {
                run += 1;
                if run > longest {
                    longest = run;
                    at = index + 1 - run;
                }
            } else {
                run = 0;
            }
        }
        (at, longest)
    }

    /// A 16-bit mono WAV of a sine, for the loopback test to play.
    fn tone_wav(frequency: f32, amplitude: f32, seconds: u32) -> Vec<u8> {
        let rate = 44_100u32;
        let samples = rate * seconds;
        let data: Vec<u8> = (0..samples)
            .flat_map(|index| {
                let value = amplitude
                    * (2.0 * std::f32::consts::PI * frequency * index as f32 / rate as f32).sin();
                ((value * i16::MAX as f32) as i16).to_le_bytes()
            })
            .collect();
        let mut wav = Vec::with_capacity(44 + data.len());
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        wav
    }

    /// Reads this machine's default input for a moment. Silenced rooms still hand
    /// over frames, so the promise is that the endpoint opens and delivers, not
    /// that anything is audible.
    #[test]
    #[ignore = "opens a real input endpoint"]
    fn the_default_input_opens_and_delivers_frames() {
        let mut session = open("").expect("the default input should open");
        let mut meter = Meter::new();
        let deadline = Instant::now() + Duration::from_millis(600);
        let mut frames = 0usize;
        while Instant::now() < deadline && frames == 0 {
            frames += session.read(&mut meter).unwrap_or(0);
            thread::sleep(IDLE_WAIT);
        }
        let level = meter.readings().rms;
        session.close();
        // A quiet room still delivers frames; how loud they are is the room's answer.
        println!(
            "{}: {frames} frames in 600 ms, rms {level:.4}",
            session.device_name
        );
        assert!(frames > 0, "no frames arrived from {}", session.device_name);
    }
}
