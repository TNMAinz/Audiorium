import { invoke, isTauri } from '@tauri-apps/api/core';
import { useCallback, useEffect, useRef, useState } from 'react';
import {
  defaultMpe, defaultSynth, emptyPackCatalogue, emptySynthPatches, idleSynthStatus,
  type MpeSettings, type SynthPackCatalogue, type SynthPatch, type SynthPatchList, type SynthScale,
  type SynthStatus,
} from './model';

/** A patch from an older build has fields this one added: they take their defaults. */
const normalize = (incoming: Partial<SynthPatch> | null | undefined): SynthPatch => ({
  ...defaultSynth,
  ...incoming,
  osc1: { ...defaultSynth.osc1, ...incoming?.osc1 },
  osc2: { ...defaultSynth.osc2, ...incoming?.osc2 },
  filter: { ...defaultSynth.filter, ...incoming?.filter },
  amp: { ...defaultSynth.amp, ...incoming?.amp },
  filterEnv: { ...defaultSynth.filterEnv, ...incoming?.filterEnv },
  lfo: { ...defaultSynth.lfo, ...incoming?.lfo },
  lfo2: { ...defaultSynth.lfo2, ...incoming?.lfo2 },
  mods: Array.isArray(incoming?.mods)
    ? incoming.mods
      .filter(route => route && typeof route.amount === 'number')
      .map(route => ({ ...route, curve: route.curve ?? 'linear' }))
    : [],
  macros: Array.isArray(incoming?.macros) && incoming.macros.length === 4
    ? incoming.macros.map(value => (Number.isFinite(value) ? value : 0)) as SynthPatch['macros']
    : defaultSynth.macros,
  tuning: Array.isArray(incoming?.tuning)
    ? incoming.tuning.filter(value => Number.isFinite(value)).slice(0, 128)
    : [],
});

const normalizeList = (incoming: Partial<SynthPatchList> | null | undefined): SynthPatchList => ({
  patch: normalize(incoming?.patch),
  active: typeof incoming?.active === 'string' ? incoming.active : null,
  presets: Array.isArray(incoming?.presets)
    ? incoming.presets.filter(preset => preset && typeof preset.id === 'string')
    : [],
  tables: Array.isArray(incoming?.tables) ? incoming.tables.filter(name => typeof name === 'string') : [],
  mpe: { ...defaultMpe, ...incoming?.mpe },
});

/** The packs a page browses: a malformed entry is dropped rather than drawn blank. */
const normalizeCatalogue = (incoming: Partial<SynthPackCatalogue> | null | undefined): SynthPackCatalogue => ({
  coverage: {
    rows: Array.isArray(incoming?.coverage?.rows)
      ? incoming.coverage.rows
        .filter(row => row && typeof row.label === 'string' && Number.isFinite(row.covered) && Number.isFinite(row.total))
        .map(row => ({ label: row.label, covered: row.covered, total: row.total }))
      : [],
    gridPacks: Number.isFinite(incoming?.coverage?.gridPacks) ? incoming?.coverage?.gridPacks ?? 0 : 0,
    gridPatches: Number.isFinite(incoming?.coverage?.gridPatches) ? incoming?.coverage?.gridPatches ?? 0 : 0,
  },
  families: Array.isArray(incoming?.families) ? incoming.families.filter(name => typeof name === 'string') : [],
  packs: Array.isArray(incoming?.packs)
    ? incoming.packs
      .filter(pack => pack && typeof pack.id === 'string' && typeof pack.name === 'string')
      .map(pack => ({
        id: pack.id,
        name: pack.name,
        family: typeof pack.family === 'string' ? pack.family : '',
        blurb: typeof pack.blurb === 'string' ? pack.blurb : '',
        patches: Array.isArray(pack.patches)
          ? pack.patches.filter(patch => patch && typeof patch.id === 'string' && typeof patch.name === 'string')
          : [],
      }))
    : [],
});

const normalizeStatus = (incoming: Partial<SynthStatus> | null | undefined): SynthStatus => ({
  ...idleSynthStatus,
  ...incoming,
  error: typeof incoming?.error === 'string' ? incoming.error : null,
  // A trace is pairs of numbers or it is not a trace: one bad value would shift every
  // column after it, so a malformed buffer falls back to the patch preview entire.
  scope: Array.isArray(incoming?.scope) && incoming.scope.every(value => Number.isFinite(value))
    ? incoming.scope
    : [],
});

/**
 * How often the instrument is asked what its output is doing. A running engine
 * publishes its trace every 30 ms, so the page follows it at 33 ms — a third of a
 * trace window, which reads as a scrolling waveform rather than a slideshow. A
 * stopped engine has nothing to move: its display comes from the patch, so it is
 * asked four times a second and no more.
 */
export const SYNTH_POLL_MS = 33;
export const SYNTH_REDUCED_POLL_MS = 165;
export const SYNTH_IDLE_POLL_MS = 400;

/** The readout rate as the page is set up: reduced motion asks a fifth as often. */
export function synthPollInterval(running: boolean, reducedMotion: boolean): number {
  if (!running) return SYNTH_IDLE_POLL_MS;
  return reducedMotion ? SYNTH_REDUCED_POLL_MS : SYNTH_POLL_MS;
}

/**
 * The instrument the Rust engine is running: its patch, its preset book, its
 * notes and its own output stream. The panel only ever shows what the engine
 * accepted — a refused value reverts rather than pretending it took.
 *
 * `output` is the endpoint the Devices page names for the app, as a device id, or
 * empty for Windows' default. The instrument plays to that one: it is part of this
 * app, not a second mixer beside it.
 */
export function useSynth(suspended = false, reducedMotion = false, output = '') {
  const native = isTauri();
  const [patch, setPatch] = useState<SynthPatch>(defaultSynth);
  const [patches, setPatches] = useState<SynthPatchList>(emptySynthPatches);
  const [status, setStatus] = useState<SynthStatus>(idleSynthStatus);
  const [held, setHeld] = useState<ReadonlySet<number>>(() => new Set<number>());
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [bounced, setBounced] = useState('');
  const [devices, setDevices] = useState<string[]>([]);
  const [scales, setScales] = useState<SynthScale[]>([]);
  const [catalogue, setCatalogue] = useState<SynthPackCatalogue>(emptyPackCatalogue);
  const [deviceError, setDeviceError] = useState('');
  // A drag sends one call per pointer move; a slow earlier reply must not land
  // after a newer edit and snap a knob back to the old value.
  const sent = useRef(0);
  // The endpoint the output is open on: the engine is told once, and a choice made
  // later is compared against this so the stream moves only when the choice moved.
  const opened = useRef(output);

  useEffect(() => {
    if (!native) return;
    let active = true;
    invoke<SynthPatchList>('synth_patch')
      .then(list => {
        if (!active) return;
        const normalized = normalizeList(list);
        setPatches(normalized);
        setPatch(normalized.patch);
      })
      .catch(reason => { if (active) setError(String(reason)); });
    return () => { active = false; };
  }, [native]);

  const running = status.running;
  useEffect(() => {
    // The stream reports its own voices, peak and trace. With the window in the tray
    // nothing shows them, so nothing asks; a stopped engine has only a still preview,
    // so the page settles to a slow poll instead of asking 30 times a second.
    if (!native || suspended) return;
    const interval = synthPollInterval(running, reducedMotion);
    let active = true;
    let inFlight = false;
    const tick = async () => {
      if (inFlight) return;
      inFlight = true;
      try {
        const next = normalizeStatus(await invoke<Partial<SynthStatus>>('synth_status'));
        if (active) setStatus(next);
      } catch (reason) {
        if (active) setError(String(reason));
      } finally {
        inFlight = false;
      }
    };
    void tick();
    const timer = window.setInterval(() => void tick(), interval);
    return () => { active = false; window.clearInterval(timer); };
  }, [native, suspended, running, reducedMotion]);

  const adopt = useCallback((list: SynthPatchList) => {
    setPatches(list);
    setPatch(list.patch);
  }, []);

  /** The whole patch goes to the engine; the answer is what it is running. */
  const apply = useCallback((next: SynthPatch) => {
    setPatch(next);
    if (!native) { setError('The instrument runs in the Windows app.'); return; }
    const token = ++sent.current;
    invoke<SynthPatchList>('set_synth', { patch: next })
      .then(list => {
        if (token !== sent.current) return;
        adopt(normalizeList(list));
        setError('');
      })
      .catch(async reason => {
        if (token !== sent.current) return;
        setError(String(reason));
        try {
          adopt(normalizeList(await invoke<SynthPatchList>('synth_patch')));
        } catch {
          // The message above already names the reason.
        }
      });
  }, [adopt, native]);

  /** One section of the patch, merged onto the rest. */
  const edit = useCallback((change: Partial<SynthPatch>) => {
    apply(normalize({ ...patch, ...change }));
  }, [apply, patch]);

  const noteOn = useCallback((note: number, velocity = 0.85) => {
    setHeld(current => new Set(current).add(note));
    if (native) void invoke('synth_note_on', { note, velocity }).catch(reason => setError(String(reason)));
  }, [native]);

  const noteOff = useCallback((note: number) => {
    setHeld(current => {
      if (!current.has(note)) return current;
      const next = new Set(current);
      next.delete(note);
      return next;
    });
    if (native) void invoke('synth_note_off', { note }).catch(reason => setError(String(reason)));
  }, [native]);

  const allNotesOff = useCallback(() => {
    setHeld(new Set<number>());
    if (native) void invoke('synth_all_notes_off').catch(reason => setError(String(reason)));
  }, [native]);

  const bend = useCallback((semitones: number) => {
    if (native) void invoke('synth_bend', { semitones }).catch(reason => setError(String(reason)));
  }, [native]);

  const start = useCallback(async () => {
    if (!native) { setError('The instrument runs in the Windows app.'); return; }
    setBusy(true);
    try {
      // The engine resolves this id the same way the player does; empty is the
      // Windows default, and a stream can only be moved by closing it.
      setStatus(normalizeStatus(await invoke<Partial<SynthStatus>>('synth_start', { device: output })));
      opened.current = output;
      setError('');
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }, [native, output]);

  // The Devices page names one output for the app, and the instrument is part of it:
  // a different endpoint while it runs is a different stream, so the engine closes
  // this one and opens the chosen device. A held note ends with the old stream.
  useEffect(() => {
    if (!native || !running || opened.current === output) return;
    void start();
  }, [native, running, output, start]);

  const stop = useCallback(async () => {
    if (!native) return;
    try {
      setStatus(normalizeStatus(await invoke<Partial<SynthStatus>>('synth_stop')));
    } catch (reason) {
      setError(String(reason));
    }
  }, [native]);

  const record = useCallback(async (on: boolean) => {
    if (!native) { setError('The instrument runs in the Windows app.'); return; }
    try {
      setStatus(normalizeStatus(await invoke<Partial<SynthStatus>>('synth_record', { on })));
      if (on) setBounced('');
    } catch (reason) {
      setError(String(reason));
    }
  }, [native]);

  const applyPreset = useCallback((id: string) => {
    if (!native) { setError('Patches are stored by the Windows app.'); return; }
    invoke<SynthPatchList>('synth_apply_preset', { id })
      .then(list => { adopt(normalizeList(list)); setError(''); })
      .catch(reason => setError(String(reason)));
  }, [adopt, native]);

  const savePreset = useCallback(async (name: string) => {
    if (!native) { setError('Patches are stored by the Windows app.'); return false; }
    try {
      adopt(normalizeList(await invoke<SynthPatchList>('synth_save_preset', { name })));
      setError('');
      return true;
    } catch (reason) {
      setError(String(reason));
      return false;
    }
  }, [adopt, native]);

  const deletePreset = useCallback(async (id: string) => {
    if (!native) { setError('Patches are stored by the Windows app.'); return; }
    try {
      adopt(normalizeList(await invoke<SynthPatchList>('synth_delete_preset', { id })));
      setError('');
    } catch (reason) {
      setError(String(reason));
    }
  }, [adopt, native]);

  /** A bounce is written into the library, where the Library and Master pages see it. */
  const bounce = useCallback(async (root: string) => {
    if (!native) { setError('The instrument runs in the Windows app.'); return; }
    setBusy(true);
    try {
      const path = await invoke<string>('synth_bounce', { root });
      setBounced(path);
      setError('');
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }, [native]);

  const refreshDevices = useCallback(async () => {
    // A browser tab has no MIDI ports to offer; that is the panel's own line, not
    // an error on the faceplate.
    if (!native) { setDeviceError('MIDI runs in the Windows app.'); return; }
    try {
      setDevices(await invoke<string[]>('synth_midi_devices'));
      setDeviceError('');
    } catch (reason) {
      setDeviceError(String(reason));
    }
  }, [native]);

  const openDevice = useCallback(async (port: number) => {
    if (!native) return;
    try {
      setStatus(normalizeStatus(await invoke<Partial<SynthStatus>>('synth_midi_open', { port })));
      setDeviceError('');
    } catch (reason) {
      setDeviceError(String(reason));
    }
  }, [native]);

  const setMpe = useCallback(async (settings: MpeSettings) => {
    if (!native) return;
    try {
      const list = normalizeList(await invoke<SynthPatchList>('synth_set_mpe', { settings }));
      adopt(list);
      setDeviceError('');
    } catch (reason) { setDeviceError(String(reason)); }
  }, [native, adopt]);

  const closeDevice = useCallback(async () => {
    if (!native) return;
    try {
      setStatus(normalizeStatus(await invoke<Partial<SynthStatus>>('synth_midi_close')));
    } catch (reason) {
      setDeviceError(String(reason));
    }
  }, [native]);

  /// The factory packs. They ship inside the engine, so they are read once when the
  /// page opens rather than on every patch edit — the catalogue cannot change while
  /// the app runs, and eight hundred patch names do not belong in a knob's reply.
  useEffect(() => {
    if (!native) return;
    let active = true;
    invoke<Partial<SynthPackCatalogue>>('synth_packs')
      .then(found => { if (active) setCatalogue(normalizeCatalogue(found)); })
      .catch(() => { if (active) setCatalogue(emptyPackCatalogue); });
    return () => { active = false; };
  }, [native]);

  /// One cycle of a table at the patch's morph, for the display. The page asks
  /// when the table or the morph changes, not per frame: the shape is the frame the
  /// engine is about to read, sampled once.
  /// Every tuning the engine offers, read once: the list is built into the engine
  /// and does not change while the app runs.
  useEffect(() => {
    if (!native) return;
    let active = true;
    invoke<SynthScale[]>('synth_scales')
      .then(found => {
        if (!active) return;
        setScales(Array.isArray(found)
          ? found.filter(scale => scale && typeof scale.name === 'string' && Array.isArray(scale.cents))
          : []);
      })
      .catch(() => { if (active) setScales([]); });
    return () => { active = false; };
  }, [native]);

  /// Brings a wavetable file in and returns the index it was given, or null when
  /// the picker was cancelled. The list the engine answers with is adopted whole,
  /// so the page's table names and its own copy never disagree.
  const chooseTable = useCallback(async (): Promise<number | null> => {
    if (!native) return null;
    try {
      const answer = await invoke<SynthPatchList | null>('synth_choose_table');
      if (!answer) return null;
      const list = normalizeList(answer);
      adopt(list);
      setError('');
      return Math.max(0, list.tables.length - 1);
    } catch (reason) {
      setError(String(reason));
      return null;
    }
  }, [native, adopt]);

  /// Reads a Scala file into a tuning, or nothing when the picker is cancelled.
  const chooseScale = useCallback(async (): Promise<SynthScale | null> => {
    if (!native) return null;
    try {
      const answer = await invoke<SynthScale | null>('synth_choose_scale');
      if (!answer || !Array.isArray(answer.cents)) return null;
      setError('');
      return answer;
    } catch (reason) {
      setError(String(reason));
      return null;
    }
  }, [native]);

  const tableShape = useCallback(async (table: number, morph: number) => {
    if (!native) return [] as number[];
    try {
      const shape = await invoke<number[]>('synth_table_shape', { table, morph });
      return Array.isArray(shape) && shape.every(value => Number.isFinite(value)) ? shape : [];
    } catch {
      return [];
    }
  }, [native]);

  return {
    native, patch, patches, status, held, error, busy, bounced,
    devices, deviceError, tableShape, scales, catalogue, chooseTable, chooseScale,
    apply, edit, noteOn, noteOff, allNotesOff, bend,
    start, stop, record, bounce,
    applyPreset, savePreset, deletePreset,
    refreshDevices, openDevice, closeDevice, setMpe,
  };
}
