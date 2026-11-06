import { invoke, isTauri } from '@tauri-apps/api/core';
import { useCallback, useEffect, useRef, useState } from 'react';
import { type SpatialSettings } from './model';
import { localSpatialStorage, readSpatial, writeSpatial } from './spatialMemory';
import {
  localInputStorage, localOutputStorage, readInputDevice, readMonitor, readOutputDevice,
  usableInputDevice, usableOutputDevice, writeInputDevice, writeMonitor, writeOutputDevice,
} from './deviceMemory';

/**
 * What is being measured: the playback engine measures the frames it wrote to the
 * output after the panner, room, equaliser, limiter and volume, and the input
 * capture measures the endpoint the user chose. `source` names which of the two
 * these readings are, and `playing` is the engine's own answer, so an idle app
 * reports no level at all.
 */
export interface AnalysisStatus {
  playing: boolean;
  /** `playback` or `input`: what these numbers came from. */
  source: string;
  /** Loaded but paused: the readings are the last window, held, and the panels freeze them. */
  paused: boolean;
  format: string;
  rms: number;
  peak: number;
  low: number;
  mid: number;
  high: number;
  /** What the two output channels share: +1 the same signal, 0 unrelated, −1 inverted. */
  correlation: number;
  /** Right minus left of the channel RMS, normalised: 0 centred, ±1 one side only. */
  balance: number;
  /** The measured window's samples as mid, side pairs, for the goniometer. */
  vectorscope: number[];
  spectrum: number[];
}
export interface AudioDevice {
  id: string;
  name: string;
  isDefault: boolean;
  /** An output offered as a capture source: Windows' loopback of it, not a capture
   * endpoint. This is how another application's audio reaches the panels without
   * changing that application or installing a device. */
  loopback?: boolean;
}
/** The optional Audiorium device.
 *
 * Nothing in the app needs it: playback goes to any output and an input is chosen
 * from whatever Windows lists. It exists so other applications can be routed
 * through it, and `installed` is the device node while `active` is the pair of
 * endpoints Windows actually started. */
export interface DriverStatus {
  installed: boolean;
  active: boolean;
  /** The names Windows lists the two endpoints under, empty when absent. */
  renderName: string;
  captureName: string;
  /** Firmware Secure Boot is on, which stops a test-signed driver loading. */
  secureBoot: boolean;
  /** The installer this build carries, empty when it carries none. */
  installer: string;
}
/** What one press of the install button produced: the installer's verdict and its log. */
export interface InstallReport { ok: boolean; log: string; status: DriverStatus }
const noDriver: DriverStatus = {
  installed: false, active: false, renderName: '', captureName: '', secureBoot: false, installer: '',
};
/** What the output can do with Windows Spatial Sound right now. `reason` is the
 * engine's own explanation when `enabled` is false. */
export interface SpatialOutput { enabled: boolean; maxObjects: number; reason: string }
const noSpatial: SpatialOutput = { enabled: false, maxObjects: 0, reason: '' };
/** What a running capture is played to. Off is a real state, and so is an error:
 * a capture played into the output it reads would feed back and is refused. */
export interface MonitorStatus {
  active: boolean;
  /** The output's name, empty when nothing is being heard. */
  deviceName: string;
  error: string | null;
}
/** The input capture: whether it is running, which endpoint it opened, why it
 * stopped when it stopped by itself, and where it is being heard. */
export interface CaptureStatus {
  active: boolean;
  device: string;
  deviceName: string;
  /** The source is an output read in loopback: what it is playing. */
  loopback: boolean;
  error: string | null;
  monitor: MonitorStatus;
}
const noCapture: CaptureStatus = {
  active: false, device: '', deviceName: '', loopback: false, error: null,
  monitor: { active: false, deviceName: '', error: null },
};
/** A capture status from any build of the engine. A build that predates listening
 * answers without `monitor`, and that has to read as "not listening" — not as a
 * missing field on a page that then renders nothing at all. */
export function normalizeCapture(
  incoming: (Partial<Omit<CaptureStatus, 'monitor'>> & { monitor?: Partial<MonitorStatus> | null }) | null | undefined,
): CaptureStatus {
  const status = incoming ?? {};
  return {
    ...noCapture,
    ...status,
    monitor: { ...noCapture.monitor, ...(status.monitor ?? {}) },
  };
}

const idle: AnalysisStatus = {
  playing: false, source: 'playback', paused: false, format: '48 kHz · 32-bit float · stereo',
  rms: 0, peak: 0, low: 0, mid: 0, high: 0, correlation: 0, balance: 0, vectorscope: [], spectrum: [],
};
const normalize = (incoming: Partial<AnalysisStatus>): AnalysisStatus => ({
  ...idle,
  ...incoming,
  vectorscope: Array.isArray(incoming.vectorscope) ? incoming.vectorscope : [],
  spectrum: Array.isArray(incoming.spectrum) ? incoming.spectrum : [],
});

export interface CaptureRequest { input: string; output: string; listen: boolean }
/** Whether a device change has to re-open a running capture.
 *
 * A WASAPI stream cannot be repointed at another endpoint, and this app opens one
 * stream per capture, so a new endpoint, a new output to listen on or a change of
 * listening means the capture has to be opened again on the new request. Nothing
 * has to happen while no capture runs: the choice is simply the next one used. */
export function reopensCapture(
  running: boolean,
  previous: CaptureRequest,
  next: CaptureRequest,
): boolean {
  if (!running) return false;
  return previous.input !== next.input
    || previous.output !== next.output
    || previous.listen !== next.listen;
}

/** How often the interface asks the engine for new readings, in milliseconds.
 *
 * The engine publishes every 16 ms and the panels read every 20 ms, so a poll
 * carries a measurement taken since the last one. Polling slower than the engine
 * publishes is what makes a spectrum look like it is stepping: at 120 ms the
 * curve moved eight times a second while the window drew thirty. At 20 ms the
 * readings move on every painted frame this WebView can produce. The trace and
 * every readout that names its own span use this number, so the figure they show
 * is the interval the app actually polls at. */
export const ANALYSIS_POLL_MS = 20;

/** How often the panels ask while reduced motion is on, in milliseconds.
 *
 * A tenth of a second still moves every reading — the numbers are the engine's own, and
 * only the rate at which the page picks them up changes — for a fifth of the drawing.
 * Nothing is held or frozen: a quieter field, not a stopped instrument. */
export const ANALYSIS_REDUCED_POLL_MS = 100;

/** The interval the panels poll at. Every readout that names its own span uses it, so
 * the figure it shows is the interval the app actually polls at. */
export function pollInterval(reducedMotion: boolean): number {
  return reducedMotion ? ANALYSIS_REDUCED_POLL_MS : ANALYSIS_POLL_MS;
}

/** How far the level trace reaches back: about 12 s at the live rate. */
export const TRACE_SPAN_MS = 12_000;
/** How many published frames the landscape draws behind the live curve: 640 ms at the
 * live rate, so the throttle keeps the tail it claims rather than stretching it. */
export const SURFACE_SPAN_MS = 640;

/** The level trace's points for a given interval: the span stays the same at either rate. */
export function historyFrames(intervalMs: number): number {
  return Math.max(2, Math.round(TRACE_SPAN_MS / Math.max(1, intervalMs)));
}

/** The landscape's layers for a given interval, so the tail is the same stretch of time. */
export function surfaceFrames(intervalMs: number): number {
  return Math.max(4, Math.round(SURFACE_SPAN_MS / Math.max(1, intervalMs)));
}
/** One measurement of the level, for the trace the Analysis page draws. */
export interface LevelPoint { rms: number; peak: number }

/**
 * Playback output selection, the spatial renderer and the output's own readings.
 * An empty output id means the Windows default, which is what plays unless another
 * output device is chosen on the Devices page.
 */
export function useAnalysis(suspended = false, reducedMotion = false) {
  const native = isTauri();
  // Reduced motion asks less often rather than holding a reading: the panels stay true
  // to the engine, and the page draws a fifth of the curves for the same numbers.
  const pollMs = pollInterval(reducedMotion);
  // The reading and its trace are one state, so a poll is one render rather than two:
  // a second setState from an effect doubled the commits at fifty a second.
  const [snapshot, setSnapshot] = useState<{ status: AnalysisStatus; history: LevelPoint[] }>({ status: idle, history: [] });
  const status = snapshot.status;
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [inputs, setInputs] = useState<AudioDevice[]>([]);
  const [error, setError] = useState('');
  // The devices chosen last time, applied on the first launch after they are picked.
  const [remembered, setRemembered] = useState(() => readOutputDevice(localOutputStorage()));
  const [output, setOutput] = useState(remembered);
  const [rememberedInput, setRememberedInput] = useState(() => readInputDevice(localInputStorage()));
  const [input, setInput] = useState(rememberedInput);
  const [driver, setDriver] = useState<DriverStatus>(noDriver);
  const [capture, setCapture] = useState<CaptureStatus>(noCapture);
  // Listening is remembered like a device choice: it is how this user wants a
  // capture to behave, and pressing Capture on a fresh launch should be heard.
  const [monitor, setMonitorState] = useState(() => readMonitor(localOutputStorage()));
  const [captureBusy, setCaptureBusy] = useState(false);
  const [captureError, setCaptureError] = useState('');
  const [installing, setInstalling] = useState(false);
  const [installLog, setInstallLog] = useState('');
  const [installError, setInstallError] = useState('');
  // The field is a setting, not a reading: it comes back the way it was left.
  const [spatial, setSpatialState] = useState<SpatialSettings>(() => readSpatial(localSpatialStorage()));
  const [spatialOutput, setSpatialOutput] = useState<SpatialOutput>(noSpatial);
  const levels = useRef<AnalysisStatus>(idle);
  /** The choices as they are right now, for the callbacks that change them: a
   * device change has to know what the running capture was opened with. */
  const live = useRef<CaptureRequest>({ input, output, listen: monitor });
  live.current = { input, output, listen: monitor };
  const captureLive = useRef(capture);
  captureLive.current = capture;
  /** The last frames the landscape draws behind the live curve, oldest first. */
  const surface = useRef<number[][]>([]);

  /** Both directions are read together, so the two lists on the Devices page are
   * a picture of the same moment. One direction failing must not blank the other:
   * each list keeps whatever it got, and the failure is reported on its own. */
  const refreshDevices = useCallback(async () => {
    if (!native) return;
    const [outputs, captures] = await Promise.allSettled([
      invoke<AudioDevice[]>('list_audio_outputs'),
      invoke<AudioDevice[]>('list_audio_inputs'),
    ]);
    if (outputs.status === 'fulfilled') setDevices(outputs.value);
    if (captures.status === 'fulfilled') setInputs(captures.value);
    const failure = outputs.status === 'rejected'
      ? outputs.reason
      : captures.status === 'rejected' ? captures.reason : '';
    setError(failure ? String(failure) : '');
  }, [native]);
  useEffect(() => { void refreshDevices(); }, [refreshDevices]);

  /** What this machine has of the Audiorium device, and what installing it would run. */
  const refreshDriver = useCallback(async () => {
    if (!native) { setDriver(noDriver); return; }
    try { setDriver(await invoke<DriverStatus>('audiorium_device_status')); }
    catch { setDriver(noDriver); }
  }, [native]);
  useEffect(() => { void refreshDriver(); }, [refreshDriver]);

  /** One press: Windows asks for permission, the bundled installer runs, and the
   * log it wrote comes back whether it worked or not. */
  const installDriver = useCallback(async () => {
    if (!native) return;
    setInstalling(true);
    setInstallError('');
    setInstallLog('');
    try {
      const report = await invoke<InstallReport>('install_audiorium_device');
      setDriver(report.status);
      setInstallLog(report.log);
      if (!report.ok) setInstallError('The installer reported a failure; its log is below.');
      // A fresh install only shows up as endpoints once Windows loads the driver.
      void refreshDevices();
    } catch (failure) { setInstallError(String(failure)); }
    finally { setInstalling(false); }
  }, [native, refreshDevices]);

  // A remembered device that is no longer connected must not be played to, and an
  // input that is gone cannot stay chosen: use the Windows default for this
  // session, and keep the memory for when the device comes back.
  useEffect(() => {
    if (output && devices.length && !usableOutputDevice(output, devices)) setOutput('');
  }, [devices, output]);
  useEffect(() => {
    if (!input) return;
    // An output can be a source too, so a remembered id is only missing when it is
    // in neither list.
    const present = [...inputs, ...devices];
    if (present.length && !usableInputDevice(input, present)) setInput('');
  }, [devices, inputs, input]);

  /** The capture's own answer, so the page never claims a stream it does not have. */
  const refreshCapture = useCallback(async () => {
    if (!native) { setCapture(noCapture); return; }
    try { setCapture(normalizeCapture(await invoke<Partial<CaptureStatus>>('capture_status'))); }
    catch { setCapture(noCapture); }
  }, [native]);
  useEffect(() => {
    if (!native || suspended) return;
    void refreshCapture();
    // Four times a second is enough to notice the device going away, and cheap
    // enough not to compete with the 20 ms readings poll.
    const timer = window.setInterval(() => void refreshCapture(), 250);
    return () => window.clearInterval(timer);
  }, [native, refreshCapture, suspended]);

  /** Opens a capture on one endpoint and one output, replacing any capture already
   * running: the engine makes the new request take the device before it opens it. */
  const startCaptureOn = useCallback(async (inputId: string, outputId: string, listening: boolean) => {
    if (!native) return;
    setCaptureBusy(true);
    setCaptureError('');
    try {
      await invoke('start_input_capture', { deviceId: inputId, monitor: listening, monitorDevice: outputId });
      await refreshCapture();
    } catch (failure) { setCaptureError(String(failure)); }
    finally { setCaptureBusy(false); }
  }, [native, refreshCapture]);

  /** A new choice is the remembered one, including "Windows default" as a choice. */
  const chooseOutput = useCallback((id: string) => {
    setOutput(id);
    setRemembered(id);
    writeOutputDevice(localOutputStorage(), id);
    if (reopensCapture(captureLive.current.active, live.current, { ...live.current, output: id })) {
      void startCaptureOn(live.current.input, id, live.current.listen);
    }
  }, [startCaptureOn]);

  const chooseInput = useCallback((id: string) => {
    setInput(id);
    setRememberedInput(id);
    writeInputDevice(localInputStorage(), id);
    if (reopensCapture(captureLive.current.active, live.current, { ...live.current, input: id })) {
      void startCaptureOn(id, live.current.output, live.current.listen);
    }
  }, [startCaptureOn]);

  const chooseMonitor = useCallback((listening: boolean) => {
    setMonitorState(listening);
    writeMonitor(localOutputStorage(), listening);
    if (reopensCapture(captureLive.current.active, live.current, { ...live.current, listen: listening })) {
      void startCaptureOn(live.current.input, live.current.output, listening);
    }
  }, [startCaptureOn]);

  /** Reads the chosen input, and the panels show it while it runs. What is read is
   * also played to the chosen output unless listening is off, so an application
   * playing into that input is heard where the output sends it. */
  const startCapture = useCallback(
    () => startCaptureOn(input, output, monitor),
    [startCaptureOn, input, output, monitor],
  );

  const stopCapture = useCallback(async () => {
    if (!native) return;
    setCaptureBusy(true);
    setCaptureError('');
    try { await invoke('stop_input_capture'); }
    catch (failure) { setCaptureError(String(failure)); }
    finally { setCaptureBusy(false); await refreshCapture(); }
  }, [native, refreshCapture]);

  useEffect(() => {
    // Nothing on screen is being watched while the window is in the tray, so the
    // engine is not asked fifty times a second for a reading nobody reads. The
    // trace holds where it was, and the first poll after the window comes back
    // carries on from there.
    if (!native || suspended) return;
    let active = true;
    let inFlight = false;
    // Both spans are time, not counts, so a slower poll keeps the same 12 s of trace
    // and the same 640 ms of spectrum behind the live curve.
    const traceFrames = historyFrames(pollMs);
    const surfaceLimit = surfaceFrames(pollMs);
    const poll = async () => {
      if (inFlight) return;
      inFlight = true;
      try {
        const current = normalize(await invoke<Partial<AnalysisStatus>>('analysis_status'));
        if (active) {
          levels.current = current;
          // A pause holds the surface and the trace where they are; a stop clears
          // them, because a stopped engine has no landscape, only a floor.
          if (current.playing && current.spectrum.length) {
            surface.current = [...surface.current.slice(-(surfaceLimit - 1)), current.spectrum];
          } else if (!current.paused && surface.current.length) {
            surface.current = [];
          }
          setSnapshot(previous => {
            let history = previous.history;
            if (!current.paused) {
              history = current.playing
                ? [...previous.history.slice(-(traceFrames - 1)), { rms: current.rms, peak: current.peak }]
                : previous.history.length ? [] : previous.history;
            }
            return { status: current, history };
          });
        }
      } catch (e) { if (active) setError(String(e)); }
      finally { inFlight = false; }
    };
    void poll();
    const timer = window.setInterval(() => void poll(), pollMs);
    return () => { active = false; window.clearInterval(timer); };
  }, [native, suspended, pollMs]);

  // The probe is the endpoint's own answer, not a guess: it runs only while
  // Atmos is the selected mode, against the device playback would use.
  const refreshSpatialOutput = useCallback(async () => {
    if (!native) { setSpatialOutput(noSpatial); return; }
    try { setSpatialOutput(await invoke<SpatialOutput>('spatial_output', { deviceId: output })); }
    catch (reason) { setSpatialOutput({ enabled: false, maxObjects: 0, reason: String(reason) }); }
  }, [native, output]);
  useEffect(() => {
    if (native && spatial.mode === 'atmos') void refreshSpatialOutput();
    else setSpatialOutput(noSpatial);
  }, [native, spatial.mode, refreshSpatialOutput]);

  const setSpatial = async (next: SpatialSettings) => {
    setSpatialState(next);
    writeSpatial(localSpatialStorage(), next);
    if (!native) return;
    try {
      await invoke('set_spatial', {
        mode: next.mode, width: next.width, crossfeed: next.crossfeed,
        height: next.height, rotation: next.rotation,
      });
      setError('');
    } catch (e) { setError(String(e)); }
  };

  // A remembered field is applied to the renderer once on launch; without this the
  // restored numbers would be drawn on the page while the engine still ran the
  // factory setting. A launch with nothing saved applies the factory field, which
  // is what the engine starts with anyway.
  const appliedSpatial = useRef(false);
  useEffect(() => {
    if (!native || appliedSpatial.current) return;
    appliedSpatial.current = true;
    void setSpatial(spatial);
  }, [native, spatial]);

  const chosen = devices.find(device => device.id === output);
  const fallback = devices.find(device => device.isDefault);
  /** Every output, offered as a loopback source: what that device is playing. */
  const loopbacks: AudioDevice[] = devices.map(device => ({
    ...device, isDefault: false, loopback: true,
  }));
  const sources = [...inputs, ...loopbacks];
  const chosenInput = sources.find(device => device.id === input);
  const fallbackInput = inputs.find(device => device.isDefault);
  return {
    native, status, history: snapshot.history, levels, surface, pollMs, devices, output, setOutput: chooseOutput,
    /** The remembered choice is not on this machine right now; the default is playing. */
    rememberedMissing: !!remembered && devices.length > 0 && !usableOutputDevice(remembered, devices),
    /** The output device playback names in its readouts: the chosen one, or Windows' default. */
    outputName: chosen?.name ?? (output ? 'Unavailable device' : fallback?.name ?? ''),
    inputs, loopbacks, input, setInput: chooseInput,
    /** The remembered input is not on this machine right now; the default is named. */
    rememberedInputMissing: !!rememberedInput && sources.length > 0 && !usableInputDevice(rememberedInput, sources),
    /** The input device the capture path would read: the chosen one, or Windows' default. */
    inputName: chosenInput?.name ?? (input ? 'Unavailable device' : fallbackInput?.name ?? ''),
    driver, refreshDriver, installDriver, installing, installLog, installError,
    capture, captureBusy, captureError, startCapture, stopCapture, monitor, setMonitor: chooseMonitor,
    spatial, setSpatial, spatialOutput, refreshSpatialOutput, error, refreshDevices,
  };
}
