import { AudioWaveform, Headphones, Library, Settings2, SlidersHorizontal, Sparkles } from 'lucide-react';
import { describeEq, formatGain, spatialModes } from '../model';
import { ModeSelector } from '../components/instrument/ModeSelector';
import { ScrollArea } from '../components/instrument/ScrollArea';
import type { useAnalysis } from '../useAnalysis';
import type { useChain } from '../useChain';
import { Page } from './Page';

interface Props {
  audio: ReturnType<typeof useAnalysis>;
  chain: ReturnType<typeof useChain>;
  /** The loaded track, or empty when nothing is playing. */
  track: string;
  volume: number;
  onVolume: (volume: number) => void;
  onOpenRack: () => void;
  /** The bands are edited on their own page; this page only reports them. */
  onOpenEq: () => void;
}

export function SignalChainPage({ audio, chain, track, volume, onVolume, onOpenRack, onOpenEq }: Props) {
  const status = audio.status;
  const mode = spatialModes.find(item => item.id === audio.spatial.mode) ?? spatialModes[0];
  const playing = status.playing;
  const eq = chain.chain.eq;
  const eqRunning = eq.enabled && eq.filters.some(filter => filter.enabled);
  const stages = [
    chain.chain.panner.enabled ? `Panner ${Math.round(chain.chain.panner.azimuth)}° / ${Math.round(chain.chain.panner.elevation)}°` : null,
    chain.chain.reverb.enabled ? `Reverb ${Math.round(chain.chain.reverb.wet * 100)}% wet` : null,
    eqRunning ? `Equaliser · ${eq.filters.filter(filter => filter.enabled).length} band${eq.filters.filter(filter => filter.enabled).length === 1 ? '' : 's'}` : null,
    chain.chain.limiter.enabled ? `Limiter ${chain.chain.limiter.thresholdDb.toFixed(1)} dB` : null,
  ].filter(Boolean) as string[];
  // The path reads in processing order: the decoded file, the level, the field, the processors, the output device.
  const modules = [
    { name: 'Panner', on: chain.chain.panner.enabled, value: `${Math.round(chain.chain.panner.azimuth)}° / ${Math.round(chain.chain.panner.elevation)}°` },
    { name: 'Reverb', on: chain.chain.reverb.enabled, value: `${chain.chain.reverb.network} · ${Math.round(chain.chain.reverb.wet * 100)}% wet` },
    { name: 'Equaliser', on: eqRunning, value: describeEq(eq) },
    { name: 'Limiter', on: chain.chain.limiter.enabled, value: `${chain.chain.limiter.thresholdDb.toFixed(1)} dB · ${chain.chain.limiter.release} release` },
  ];
  return <Page
    title="Signal chain"
    state={`${status.format} · ${playing ? 'playing' : 'idle'} · ${stages.length ? stages.join(' · ') : 'all processors bypassed'}`}
    actions={<div className="page-actions-row">
      <button className="page-button" onClick={onOpenEq}><AudioWaveform size={13} />Equaliser</button>
      <button className="page-button" onClick={onOpenRack}><SlidersHorizontal size={13} />Processor rack</button>
      <span className={`source-badge ${playing ? 'live' : ''}`}><i />{playing ? 'Playing' : 'Idle'}</span>
    </div>}
  >
    <div className="page-grid chain-grid">
      <article className="page-card wide-card">
        <div className="panel-heading"><h2>Path</h2><span className="panel-meta">{status.format}</span></div>
        <div className={`chain ${playing ? 'connected' : ''}`}>
          <div className="chain-stage">
            <Library size={18} />
            <strong title={track || 'No track loaded'}>{track || 'Library playback'}</strong>
            <span>Decoded on this machine</span>
          </div>
          <b aria-hidden="true">→</b>
          <div className="chain-stage">
            <Settings2 size={18} />
            <strong>{formatGain(volume)}</strong>
            <span>Playback level</span>
          </div>
          <b aria-hidden="true">→</b>
          <div className="chain-stage">
            <Sparkles size={18} />
            <strong>{mode.label}</strong>
            <span>Spatial renderer</span>
          </div>
          <b aria-hidden="true">→</b>
          <div className="chain-stage">
            <SlidersHorizontal size={18} />
            <strong title={stages.join(' · ') || 'All bypassed'}>{stages.length ? `${stages.length} active` : 'All bypassed'}</strong>
            <span>Processors</span>
          </div>
          <b aria-hidden="true">→</b>
          <div className="chain-stage">
            <Headphones size={18} />
            <strong title={audio.outputName || 'Windows default'}>{audio.outputName || 'Windows default'}</strong>
            <span>Output device</span>
          </div>
        </div>
        <ScrollArea className="module-table" role="table" ariaLabel="Processors in the path">
          <div className="module-head-row" role="row"><span>Processor</span><span>State</span><span>Value</span></div>
          {modules.map(module => <div key={module.name} className="module-row" role="row">
            <span>{module.name}</span>
            <span className={module.on ? 'on' : ''}>{module.on ? 'IN PATH' : 'BYPASSED'}</span>
            <span title={module.value}>{module.value}</span>
          </div>)}
        </ScrollArea>
        <div className="fact-row">
          <span>48 kHz</span><span>32-bit float</span><span>Stereo</span><span>{stages.length ? stages.join(' · ') : 'Bypassed'}</span>
        </div>
      </article>

      <div className="page-stack">
        <article className="page-card">
          <div className="panel-heading"><h2>Playback level</h2><span className="panel-meta">{formatGain(volume)}</span></div>
          <input className="audio-range" type="range" min="0" max="1" step="0.01" value={volume} aria-label="Playback level" onChange={event => onVolume(Number(event.target.value))} />
          <p className="panel-note">Applied before the processors, so the limiter works on what you actually send out.</p>
        </article>
        <article className="page-card">
          <div className="panel-heading"><h2>Spatial stage</h2></div>
          <ModeSelector value={audio.spatial.mode} onChange={mode => void audio.setSpatial({ ...audio.spatial, mode })} />
          <p className="panel-note">{mode.note}</p>
        </article>
        <article className="page-card warning-card">
          <div className="panel-heading"><h2>Guards</h2></div>
          <ul className="guard-list">
            <li>Files are decoded here; the Windows mix is never tapped. Capture, when it runs, reads one chosen input.</li>
            <li>A stage that is bypassed is a pass-through, not a quieter version.</li>
            <li>The decoder ring is bounded; a stalled disk drops frames rather than growing.</li>
          </ul>
        </article>
      </div>
    </div>
  </Page>;
}
