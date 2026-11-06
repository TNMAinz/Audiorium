import { useEffect, useRef, type CSSProperties } from 'react';
import { Settings2, SlidersHorizontal, X } from 'lucide-react';
import type { useAnalysis } from '../useAnalysis';
import type { useChain } from '../useChain';
import { formatGain, reverbTime, sourceTitle, spatialModes, themes, type Theme } from '../model';
import { PeakMeter, SpectrumLandscape } from './SignalLandscape';
import { ModeSelector } from './instrument/ModeSelector';

interface Props {
  analysis: ReturnType<typeof useAnalysis>;
  chain: ReturnType<typeof useChain>;
  theme: Theme;
  onTheme: (theme: Theme) => void;
  reducedMotion: boolean;
  onReducedMotion: () => void;
  /** Bumped by the player's Audio controls button to open the setup dialog. */
  setupRequest?: number;
  /** Bumped by the pages that open the processor rack. */
  onOpenRack?: () => void;
  volume: number;
  onVolume: (volume: number) => void;
  /** The loaded track, or empty when nothing is playing. */
  track: string;
  /** Which renderer the open track uses; empty while idle. */
  spatialPath?: string;
}

export function AudioInspector({ analysis, chain, theme, onTheme, reducedMotion, onReducedMotion, setupRequest = 0, onOpenRack = () => {}, volume, onVolume, track, spatialPath = '' }: Props) {
  const settings = useRef<HTMLDialogElement>(null);
  const settingsButton = useRef<HTMLButtonElement>(null);
  const opener = useRef<HTMLElement | null>(null);
  const openSettings = () => {
    const active = document.activeElement;
    opener.current = active && active !== document.body ? (active as HTMLElement) : null;
    if (!settings.current?.open) settings.current?.showModal();
  };
  useEffect(() => { if (setupRequest > 0) openSettings(); }, [setupRequest]);
  const playing = analysis.status.playing;
  const paused = analysis.status.paused;
  // A pause holds the last measurement, so the panels report the pause instead of "IDLE".
  const state = playing ? 'LIVE' : paused ? 'PAUSED' : 'IDLE';
  const outputLabel = analysis.outputName || 'Windows default';
  const selected = spatialModes.find(item => item.id === analysis.spatial.mode) ?? spatialModes[0];
  // The rail says what the selected track will actually do: with spatial sound
  // off, the Atmos position falls back to the app's own derived heights.
  const note = selected.id === 'atmos' && !analysis.spatialOutput.enabled
    ? 'Windows spatial is off for this output, so this track uses the app’s own 7.1.4 render instead of Dolby Atmos.'
    : selected.note;
  // The reverb reports the two numbers that shape it, and the limiter reports the only
  // number it has. A bypassed stage says so instead of printing a zero it never measured.
  const reverb = chain.chain.reverb;
  const decayPercent = Math.round(reverb.decay * 100);
  const dryPercent = Math.round(reverb.dry * 100);
  const wetPercent = Math.round(reverb.wet * 100);
  const limiter = chain.chain.limiter;
  const reduction = chain.meters.gainReductionDb;
  // Any app-rendered layout can be turned, so the page owns the placement for every
  // mode but stereo. Atmos is Windows' fixed 7.1.4 bed and has no placement controls.
  const shaped = analysis.spatial.mode !== 'stereo' && analysis.spatial.mode !== 'atmos';
  const atmos = analysis.spatial.mode === 'atmos';
  // The route describes the stream that is really open: the 7.1.4 bed only when
  // Windows spatial is available and this track did not fall back.
  const atmosOutput = atmos && analysis.spatialOutput.enabled && spatialPath !== 'derived';
  const format = atmosOutput ? '48 kHz · 32-bit float · 7.1.4 bed' : analysis.status.format;
  return <aside className="inspector instrument-stack" aria-label="Audio and scene inspector">
    <section className="glass-panel spatial-panel">
      <div className="panel-heading"><h2>Spatialization</h2><div className="heading-tools"><span className={`panel-meta ${playing ? 'live-meta' : ''}`}>{playing ? 'ACTIVE' : paused ? 'PAUSED' : 'READY'}</span><button ref={settingsButton} className="icon-button" aria-label="Open audio setup" title="Audio and scene settings" onClick={openSettings}><Settings2 size={14} /></button></div></div>
      <div className="spatial-body">
        <ModeSelector compact value={analysis.spatial.mode} onChange={mode => void analysis.setSpatial({ ...analysis.spatial, mode })} />
        <p className="panel-note">{note}</p>
      </div>
    </section>

    <section className="glass-panel spectrum-panel">
      <div className="panel-heading"><h2>Spectrum</h2><span className={`panel-meta ${playing ? 'live-meta' : ''}`}>{state}</span></div>
      <SpectrumLandscape status={analysis.status} surface={analysis.surface.current} />
      <p className="panel-note">FFT 2048 · Hann · 30 Hz – 16 kHz</p>
    </section>

    <section className="glass-panel dynamics-panel">
      <div className="panel-heading"><h2>Level</h2><span className="panel-meta">dBFS · not LUFS</span></div>
      <PeakMeter status={analysis.status} />
    </section>

    <section className="glass-panel routing-panel">
      <div className="panel-heading"><h2>Signal chain</h2><span className={`panel-meta ${playing ? 'live-meta' : ''}`}>{state}</span></div>
      <dl className={`route-list readout ${playing ? 'connected' : ''}`}>
        <div><dt>Source</dt><dd title={analysis.status.source === 'input'
          ? `Measuring ${analysis.capture.deviceName || 'the chosen input'}, read in shared mode`
          : track ? `Playing ${track} from this machine` : 'No track loaded. Files are decoded here, and the panels have nothing to measure until one plays.'}>{sourceTitle(analysis.status.source)}</dd></div>
        <div><dt>Level</dt><dd title={`Playback level ${Math.round(volume * 100)} percent`}>{formatGain(volume)}</dd></div>
        <div><dt>Output</dt><dd title={outputLabel}>{outputLabel}</dd></div>
      </dl>
      {/* One state line, always: the stream format, or the engine's own error in its place.
          The error belongs to this panel because the chain it names is what fails. */}
      {analysis.error
        ? <p className="panel-note route-error" role="alert" title={analysis.error}>
          <i className="error-lamp" aria-hidden="true" />
          <button type="button" className="error-details" onClick={openSettings}>Audio error · View details</button>
        </p>
        : <p className="panel-note route-status">{format.split(' · ').map((token, at) => <span key={token}>{at > 0 && <i aria-hidden="true">·</i>}{token}</span>)}</p>}
    </section>

    <section className="glass-panel reverb-panel">
      <div className="panel-heading"><h2>Reverb</h2><div className="heading-tools"><span className={`instrument-tag ${reverb.enabled ? 'live-tag' : ''}`}>{reverb.enabled ? 'IN PATH' : 'BYPASSED'}</span><button className="icon-button" aria-label="Open processors" title="Panner, reverb and limiter" onClick={onOpenRack}><SlidersHorizontal size={13} /></button></div></div>
      <dl className={`room-readout readout ${reverb.enabled ? 'engaged' : ''}`}>
        <div><dt>Network</dt><dd title={`${reverb.network} · ${reverbTime(reverb.decay)} · dry ${dryPercent}% · ${wetPercent}% wet`}>{reverb.network}</dd></div>
        <div><dt>Decay</dt><dd><span className="readout-scale" aria-hidden="true"><i style={{ width: `${decayPercent}%` }} /></span><span className="readout-number">{reverbTime(reverb.decay)}</span></dd></div>
        <div><dt>Dry</dt><dd><span className="readout-scale" aria-hidden="true"><i style={{ width: `${dryPercent}%` }} /></span><span className="readout-number">{dryPercent} %</span></dd></div>
        <div><dt>Wet</dt><dd><span className="readout-scale" aria-hidden="true"><i style={{ width: `${wetPercent}%` }} /></span><span className="readout-number">{wetPercent} %</span></dd></div>
      </dl>
      <p className="panel-note room-state"><span>Limiter</span> {limiter.enabled ? `${reduction.toFixed(1)} dB GR` : 'bypassed'}</p>
    </section>

    <dialog ref={settings} className="scene-settings glass-panel" aria-labelledby="scene-settings-title" onClose={() => ((opener.current?.isConnected ? opener.current : null) ?? settingsButton.current)?.focus()}>
      <div className="settings-head">
        <div><h2 id="scene-settings-title">Audio & scene</h2><p className="settings-sub">Playback, output and the scene behind it.</p></div>
        <button className="icon-button" aria-label="Close scene settings" onClick={() => settings.current?.close()}><X size={16} /></button>
      </div>
      <section className="settings-group" aria-labelledby="settings-playback">
        <h3 id="settings-playback">Playback</h3>
        {analysis.native ? <>
          <div className="settings-field">
            <div className="settings-field-head"><label htmlFor="playback-volume">Level</label><span className="settings-value">{formatGain(volume)}</span></div>
            <input id="playback-volume" className="audio-range" style={{ '--fill': `${Math.round(volume * 100)}%` } as CSSProperties} type="range" min="0" max="1" step="0.01" value={volume} onChange={event => onVolume(Number(event.target.value))} />
            <p className="settings-note">Applied before the processors, so a quieter setting only reaches the limiter when the file peaks that high.</p>
          </div>
          <div className="settings-field">
            <div className="settings-field-head"><span>Output</span><span className="settings-value" title={outputLabel}>{outputLabel}</span></div>
            <p className="settings-note">Chosen on the Devices page and remembered for the next launch. Changing it mid-track moves playback at the same position.</p>
          </div>
          {/* The field parameters are placed on the page they change, not behind this dialog. */}
          {shaped && <p className="settings-note settings-note-quiet">Width, crossfeed, height and rotation are on the Spatial field page, next to the speaker plot they move.</p>}
          {atmos && <p className="settings-note settings-note-quiet">{analysis.spatialOutput.enabled
            ? 'Windows spatial is on for this output. Windows renders the 7.1.4 bed with the format selected in Windows sound settings; a change applies to the next track.'
            : analysis.spatialOutput.reason || 'Spatial sound is off for this output; the app’s own 7.1.4 render handles this track.'}{spatialPath === 'derived' ? ' This track is using the app’s own 7.1.4 render instead.' : ''}</p>}
        </> : <p className="settings-note">Playback runs in the Windows app. This preview shows an idle scene.</p>}
      </section>
      <section className="settings-group" aria-labelledby="settings-sources">
        <h3 id="settings-sources">What the panels measure</h3>
        <dl className="settings-facts">
          <div><dt>Library</dt><dd>Played from disk on this machine. Nothing is uploaded.</dd></div>
          <div><dt>Input</dt><dd>The one device chosen on the Devices page, read in shared mode.</dd></div>
          <div><dt>Windows mix</dt><dd>Only while a capture runs, and never written to disk.</dd></div>
          <div><dt>Atmos</dt><dd>The stereo bed handed to Windows, not the platform's downstream render.</dd></div>
        </dl>
      </section>
      <section className="settings-group" aria-labelledby="settings-scene">
        <h3 id="settings-scene">Scene</h3>
        <div className="settings-field">
          <div className="settings-field-head"><span>Colour</span></div>
          <div className="theme-options" aria-label="Scene color">{(Object.keys(themes) as Theme[]).map(option => <button key={option} className={`theme-option ${theme === option ? 'selected' : ''}`} onClick={() => onTheme(option)} aria-pressed={theme === option}><i style={{ background: themes[option].primary }} />{option}</button>)}</div>
        </div>
        <div className="motion-row"><div><strong>Reduced motion</strong><span>A stiller field, slower readouts</span></div><button role="switch" aria-checked={reducedMotion} aria-label="Reduced motion" className={`switch ${reducedMotion ? 'on' : ''}`} onClick={onReducedMotion}><i /></button></div>
      </section>
      {analysis.error && <p className="audio-error error-message settings-error">{analysis.error}</p>}
    </dialog>
  </aside>;
}
