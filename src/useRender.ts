import { invoke, isTauri } from '@tauri-apps/api/core';
import { useCallback, useEffect, useRef, useState } from 'react';
import {
  idleLoudness, idleRender, idleScan, type BitDepth, type FileScan, type LoudnessReadings,
  type MasterFormat, type NullReport, type RenderStatus, type SampleRate,
} from './model';

const normalize = (incoming: Partial<RenderStatus> | null | undefined): RenderStatus => ({
  ...idleRender,
  ...incoming,
  source: { ...idleLoudness, ...incoming?.source },
  output: { ...idleLoudness, ...incoming?.output },
});

/**
 * The one render the engine can be running, and what it measured. The page never
 * invents a figure: every number here came from the engine's own status, and a
 * render that has not measured anything yet shows a dash instead of a zero.
 */
export function useRender(library = '') {
  const native = isTauri();
  const [status, setStatus] = useState<RenderStatus>(idleRender);
  const [error, setError] = useState('');
  const [source, setSource] = useState('');
  // The container and rate are the page's choices, and the engine names and writes
  // the file from them.
  const [format, setFormat] = useState<MasterFormat>('wav');
  const [rate, setRate] = useState<SampleRate>('48');
  const [destination, setDestination] = useState('');
  // A file looked at whole, and the result of the bypass test on it.
  const [scan, setScan] = useState<FileScan>(idleScan);
  const [scanError, setScanError] = useState('');
  const [nullReport, setNullReport] = useState<NullReport | null>(null);
  const [nullError, setNullError] = useState('');
  // A reply from an earlier start must not overwrite a newer one, the way a
  // knob drag is protected in useChain.
  const sent = useRef(0);

  // The destination is the engine's answer, not a name built here: the file the
  // page promises is the file the render will write.
  useEffect(() => {
    if (!native || !source) { setDestination(''); return; }
    let active = true;
    invoke<string>('master_destination', { source, format, library })
      .then(next => { if (active) setDestination(next); })
      .catch(reason => { if (active) setError(String(reason)); });
    return () => { active = false; };
  }, [native, source, format, library]);

  // Poll while a render runs, and once when it stops so the last figures land.
  // Idle pages ask the engine nothing.
  useEffect(() => {
    if (!native) return;
    let active = true;
    let inFlight = false;
    const tick = async () => {
      if (inFlight) return;
      inFlight = true;
      try {
        const next = normalize(await invoke<Partial<RenderStatus>>('render_status'));
        if (active) setStatus(next);
      } catch (reason) {
        if (active) setError(String(reason));
      } finally {
        inFlight = false;
      }
    };
    void tick();
    if (!status.running) return () => { active = false; };
    const timer = window.setInterval(() => void tick(), 200);
    return () => { active = false; window.clearInterval(timer); };
  }, [native, status.running]);

  const chooseSource = useCallback((path: string) => {
    setSource(path);
    setError('');
    // A new file is a new picture; the old one would describe something else.
    setScan(idleScan);
    setScanError('');
    setNullReport(null);
    setNullError('');
  }, []);

  const start = useCallback(async (path: string, bitDepth: BitDepth) => {
    if (!native) { setError('Exporting runs in the Windows app.'); return; }
    const token = ++sent.current;
    setError('');
    // The page shows a render from the first frame rather than a stale result.
    setStatus({
      ...idleRender,
      running: true,
      phase: 'rendering',
      depth: bitDepth,
      rate,
      destination,
      track: '',
      trackIndex: 1,
      trackTotal: 1,
      source: idleLoudness,
      output: idleLoudness,
    });
    try {
      const next = normalize(await invoke<Partial<RenderStatus>>('render_start', {
        source: path, bitDepth, format, rate, library,
      }));
      if (token !== sent.current) return;
      setStatus(next);
    } catch (reason) {
      if (token !== sent.current) return;
      setError(String(reason));
      setStatus(idleRender);
    }
  }, [destination, format, library, native, rate]);

  /** An album is the same render, its tracks in the order the page gives them. */
  const startAlbum = useCallback(async (tracks: string[], bitDepth: BitDepth) => {
    if (!native) { setError('Exporting runs in the Windows app.'); return; }
    if (tracks.length < 2) { setError('An album needs more than one track.'); return; }
    const token = ++sent.current;
    setError('');
    setStatus({
      ...idleRender,
      running: true,
      phase: 'rendering',
      depth: bitDepth,
      rate,
      trackIndex: 1,
      trackTotal: tracks.length,
      source: idleLoudness,
      output: idleLoudness,
    });
    try {
      const next = normalize(await invoke<Partial<RenderStatus>>('render_album', {
        tracks, bitDepth, format, rate, library,
      }));
      if (token !== sent.current) return;
      setStatus(next);
    } catch (reason) {
      if (token !== sent.current) return;
      setError(String(reason));
      setStatus(idleRender);
    }
  }, [format, library, native, rate]);

  const cancel = useCallback(async () => {
    if (!native) return;
    try {
      await invoke('render_cancel');
    } catch (reason) {
      setError(String(reason));
    }
  }, [native]);

  /** The whole file, drawn: envelope, loudness curve and spectrogram. */
  const scanFile = useCallback(async (path: string) => {
    if (!native) { setScanError('Reading a file whole runs in the Windows app.'); return; }
    setScanError('');
    try {
      setScan(await invoke<FileScan>('scan_file', { path }));
    } catch (reason) {
      setScanError(String(reason));
    }
  }, [native]);

  /** A reference is measured, never processed: the figure a master is compared with. */
  const measureReference = useCallback(async (path: string): Promise<LoudnessReadings | null> => {
    if (!native) return null;
    try {
      return await invoke<LoudnessReadings>('reference_measure', { source: path });
    } catch (reason) {
      setError(String(reason));
      return null;
    }
  }, [native]);

  /** Whether the render path itself colours a file it was never asked to touch. */
  const nullTest = useCallback(async (path: string) => {
    if (!native) { setNullError('The bypass test runs in the Windows app.'); return; }
    setNullError('');
    setNullReport(null);
    try {
      setNullReport(await invoke<NullReport>('null_test', { source: path }));
    } catch (reason) {
      setNullError(String(reason));
    }
  }, [native]);

  return {
    native, status, error, source, format, setFormat, rate, setRate, destination,
    chooseSource, start, startAlbum, cancel,
    scan, scanError, scanFile, nullReport, nullError, nullTest, measureReference,
  };
}
