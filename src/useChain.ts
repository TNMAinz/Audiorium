import { invoke, isTauri } from '@tauri-apps/api/core';
import { useCallback, useEffect, useRef, useState } from 'react';
import {
  defaultChain, defaultEq, emptyPresets, eqLimits, idleEqResponse, idleMeters,
  type ChainMeters, type ChainSettings, type EqFilter, type EqPresetList, type EqResponse,
  type EqSettings, type LimiterSettings, type PannerSettings, type ReverbSettings,
} from './model';

const normalizeMeters = (incoming: Partial<ChainMeters>): ChainMeters => ({ ...idleMeters, ...incoming });

/** An equaliser edit and the patch list it produced, as the engine returns them. */
interface EqUpdate { chain: ChainSettings; presets: EqPresetList }

const normalizePresets = (incoming: Partial<EqPresetList> | null | undefined): EqPresetList => ({
  presets: Array.isArray(incoming?.presets)
    ? incoming.presets.filter(preset => preset && typeof preset.id === 'string')
    : [],
  active: typeof incoming?.active === 'string' ? incoming.active : null,
});

/**
 * The panner, reverb, limiter and equaliser the Rust engine is really running.
 * Settings are only ever what the backend accepted: a rejected value reverts the
 * control instead of pretending it took effect. The saved patches come from the
 * same place, so the list cannot claim a copy the engine did not make.
 */
export function useChain(suspended = false) {
  const native = isTauri();
  const [chain, setChain] = useState<ChainSettings>(defaultChain);
  const [meters, setMeters] = useState<ChainMeters>(idleMeters);
  const [eqCurve, setEqCurve] = useState<EqResponse>(idleEqResponse);
  const [presets, setPresets] = useState<EqPresetList>(emptyPresets);
  const [error, setError] = useState('');
  const [patchError, setPatchError] = useState('');
  // A drag sends one call per pointer move. Without this, a slow earlier reply
  // could land after a newer edit and snap the control back to the old value.
  const sent = useRef(0);

  useEffect(() => {
    if (!native) return;
    let active = true;
    invoke<ChainSettings>('chain_config')
      .then(next => { if (active) setChain({ ...defaultChain, ...next }); })
      .catch(reason => { if (active) setError(String(reason)); });
    invoke<EqPresetList>('eq_presets')
      .then(next => { if (active) setPresets(normalizePresets(next)); })
      .catch(reason => { if (active) setPatchError(String(reason)); });
    return () => { active = false; };
  }, [native]);

  useEffect(() => {
    // The meters are the processors' own answers, read five times a second. With the
    // window in the tray nothing displays them, so nothing asks.
    if (!native || suspended) return;
    let active = true;
    let inFlight = false;
    const tick = async () => {
      if (inFlight) return;
      inFlight = true;
      try {
        const next = normalizeMeters(await invoke<Partial<ChainMeters>>('chain_meters'));
        if (active) setMeters(next);
      } catch (reason) {
        if (active) setError(String(reason));
      } finally {
        inFlight = false;
      }
    };
    void tick();
    const timer = window.setInterval(() => void tick(), 200);
    return () => { active = false; window.clearInterval(timer); };
  }, [native, suspended]);

  // The curve is the engine's answer, not a drawing of what the sliders say: it
  // is re-read whenever the band list changes, and the graph has nothing else.
  const refreshCurve = useCallback(async () => {
    if (!native) return;
    try {
      const next = await invoke<EqResponse>('eq_response', { points: 160 });
      setEqCurve({
        frequencies: Array.isArray(next.frequencies) ? next.frequencies : [],
        gains: Array.isArray(next.gains) ? next.gains : [],
        maxBoostDb: next.maxBoostDb ?? 0,
        headroomDb: next.headroomDb ?? 0,
      });
    } catch (reason) {
      setError(String(reason));
    }
  }, [native]);

  useEffect(() => { void refreshCurve(); }, [refreshCurve, chain.eq]);

  const send = useCallback(async (run: () => Promise<ChainSettings>) => {
    if (!native) { setError('Processors run in the Windows app.'); return; }
    const token = ++sent.current;
    try {
      const next = await run();
      if (token !== sent.current) return;
      setChain({ ...defaultChain, ...next });
      setError('');
    } catch (reason) {
      if (token !== sent.current) return;
      setError(String(reason));
      // The engine kept its previous settings; show what is actually running
      // rather than the value that was refused.
      try {
        const actual = await invoke<ChainSettings>('chain_config');
        setChain({ ...defaultChain, ...actual });
      } catch {
        // Nothing more to report: the error above already names the reason.
      }
    }
  }, [native]);

  const setPanner = useCallback((patch: Partial<PannerSettings>) => {
    const next = { ...chain.panner, ...patch };
    setChain(current => ({ ...current, panner: next }));
    void send(() => invoke<ChainSettings>('set_panner', {
      enabled: next.enabled,
      azimuth: next.azimuth,
      elevation: next.elevation,
      width: next.width,
      room: next.room,
      reflectionLength: next.reflectionLength,
      reflectionLevel: next.reflectionLevel,
      timbre: next.timbre,
      focus: next.focus,
    }));
  }, [chain.panner, send]);

  const setReverb = useCallback((patch: Partial<ReverbSettings>) => {
    const next = { ...chain.reverb, ...patch };
    setChain(current => ({ ...current, reverb: next }));
    void send(() => invoke<ChainSettings>('set_reverb', {
      enabled: next.enabled, decay: next.decay, dry: next.dry, wet: next.wet, network: next.network,
    }));
  }, [chain.reverb, send]);

  const setLimiter = useCallback((patch: Partial<LimiterSettings>) => {
    const next = { ...chain.limiter, ...patch };
    setChain(current => ({ ...current, limiter: next }));
    void send(() => invoke<ChainSettings>('set_limiter', {
      enabled: next.enabled,
      thresholdDb: next.thresholdDb,
      ceilingDb: next.ceilingDb,
      release: next.release,
      detector: next.detector,
      softClip: next.softClip,
      autoMakeup: next.autoMakeup,
    }));
  }, [chain.limiter, send]);

  /**
   * The whole band list goes to the engine in one call. The answer carries the
   * patch list too: if the edit changed a shipped patch, the engine has already
   * saved a copy of it, and this is where that copy first appears.
   */
  const setEq = useCallback((next: EqSettings) => {
    setChain(current => ({ ...current, eq: next }));
    if (!native) { setError('Processors run in the Windows app.'); return; }
    const token = ++sent.current;
    invoke<EqUpdate>('set_eq', { enabled: next.enabled, preampDb: next.preampDb, filters: next.filters })
      .then(update => {
        if (token !== sent.current) return;
        setChain({ ...defaultChain, ...update.chain });
        setPresets(normalizePresets(update.presets));
        setError('');
      })
      .catch(async reason => {
        if (token !== sent.current) return;
        setError(String(reason));
        try {
          setChain({ ...defaultChain, ...(await invoke<ChainSettings>('chain_config')) });
        } catch {
          // The message above already names the reason.
        }
      });
  }, [native]);

  /** Load a patch. The engine switches the stage on with it. */
  const applyPreset = useCallback((id: string) => {
    if (!native) { setPatchError('Patches are stored by the Windows app.'); return; }
    setPatchError('');
    invoke<EqUpdate>('apply_eq_preset', { id })
      .then(update => {
        setChain({ ...defaultChain, ...update.chain });
        setPresets(normalizePresets(update.presets));
      })
      .catch(reason => setPatchError(String(reason)));
  }, [native]);

  /**
   * Rename a saved copy. Resolves false when the engine refused the name, which
   * is what keeps the row in its editing state instead of closing over a name
   * that was never stored.
   */
  const renamePreset = useCallback(async (id: string, name: string) => {
    if (!native) { setPatchError('Patches are stored by the Windows app.'); return false; }
    try {
      setPresets(normalizePresets(await invoke<EqPresetList>('rename_eq_preset', { id, name })));
      setPatchError('');
      return true;
    } catch (reason) {
      setPatchError(String(reason));
      return false;
    }
  }, [native]);

  const deletePreset = useCallback((id: string) => {
    if (!native) { setPatchError('Patches are stored by the Windows app.'); return; }
    invoke<EqPresetList>('delete_eq_preset', { id })
      .then(next => { setPresets(normalizePresets(next)); setPatchError(''); })
      .catch(reason => setPatchError(String(reason)));
  }, [native]);

  const updateFilter = useCallback((index: number, patch: Partial<EqFilter>) => {
    setEq({
      ...chain.eq,
      filters: chain.eq.filters.map((filter, at) => (at === index ? { ...filter, ...patch } : filter)),
    });
  }, [chain.eq, setEq]);

  /** A new band lands at 1 kHz, the frequency every source is loudest in. */
  const addFilter = useCallback((at?: Partial<EqFilter>) => {
    if (chain.eq.filters.length >= eqLimits.bands) return;
    const filter: EqFilter = {
      kind: 'peaking', frequency: 1_000, gainDb: 0, q: 1.4, enabled: true, ...at,
    };
    setEq({ ...chain.eq, enabled: true, filters: [...chain.eq.filters, filter] });
  }, [chain.eq, setEq]);

  const removeFilter = useCallback((index: number) => {
    setEq({ ...chain.eq, filters: chain.eq.filters.filter((_, at) => at !== index) });
  }, [chain.eq, setEq]);

  /** Back to a straight wire: no bands, no preamp, stage off, no patch. */
  const resetEq = useCallback(() => setEq(defaultEq), [setEq]);

  /** Give the biggest boost its own headroom back, so nothing has to be limited. */
  const matchHeadroom = useCallback(() => {
    const boost = Math.max(0, eqCurve.maxBoostDb);
    const room = Math.abs(eqLimits.preamp.min);
    setEq({ ...chain.eq, preampDb: -Math.min(room, Math.ceil(boost * 10) / 10) });
  }, [chain.eq, eqCurve.maxBoostDb, setEq]);

  /** `structureOnly` keeps the current structure and reseeds its primes. */
  const randomizeReverb = useCallback((structureOnly: boolean) => {
    if (!native) { setError('Processors run in the Windows app.'); return; }
    const seed = Math.floor(Math.random() * 0x7fff_fffe) + 1;
    void invoke<string>('random_reverb_network', { seed, current: chain.reverb.network, structureOnly })
      .then(network => setReverb({ network }))
      .catch(reason => setError(String(reason)));
  }, [native, chain.reverb.network, setReverb]);

  return {
    native, chain, meters, eqCurve, error, presets, patchError,
    setPanner, setReverb, setLimiter, randomizeReverb,
    setEq, updateFilter, addFilter, removeFilter, resetEq, matchHeadroom,
    applyPreset, renamePreset, deletePreset,
  };
}
