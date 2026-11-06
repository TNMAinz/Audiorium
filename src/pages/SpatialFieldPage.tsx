import { Volume2 } from 'lucide-react';
import { spatialModes, type Speaker } from '../model';
import type { useAnalysis } from '../useAnalysis';
import { ScrollArea } from '../components/instrument/ScrollArea';
import { Knob } from '../components/instrument/Knob';
import { Page } from './Page';

/** A signed angle, with the real minus sign every other readout in the app uses. */
const deg = (value: number) => `${value > 0 ? '+' : value < 0 ? '−' : ''}${Math.abs(Math.round(value))}°`;

const CENTER = { x: 200, y: 168 };
const FLOOR_DB = -60;
/** Played band level as a dBFS reading and a bar width against that floor. */
function band(value: number) {
  const db = value > 0 ? Math.max(FLOOR_DB, 20 * Math.log10(value)) : FLOOR_DB;
  return { width: `${Math.min(1, Math.max(0, (db - FLOOR_DB) / -FLOOR_DB)) * 100}%`, label: db <= FLOOR_DB ? '—' : `${db.toFixed(1).replace('-', '−')} dBFS` };
}
const SOURCE_COLORS: Record<string, string> = {
  Left: '#7cc0ff', Right: '#a995ff', Mid: '#ffc18c', Side: '#8ee7ff', Low: '#ff9aa4', High: '#7ef0c0',
};

/** Speaker trim as a signed dB change: the sign comes from the dB value, never from the linear gain. */
function trim(gain: number) {
  const db = Number((20 * Math.log10(Math.max(0.0001, Math.abs(gain)))).toFixed(1));
  return `${db > 0 ? '+' : db < 0 ? '−' : ''}${Math.abs(db).toFixed(1)} dB`;
}

function position(speaker: Speaker, maxGain: number) {
  const radians = (speaker.azimuth * Math.PI) / 180;
  const reach = speaker.elevation > 1 ? 66 : 74 + 58 * Math.min(1, Math.abs(speaker.gain) / maxGain);
  // Height speakers are drawn closer in with a stem; the rest move by their elevation
  // so a low channel sits below a mid one instead of printing on top of it.
  const lift = speaker.elevation > 1 ? 0 : -speaker.elevation * 0.8;
  return { x: CENTER.x + Math.sin(radians) * reach, y: CENTER.y - Math.cos(radians) * reach + lift };
}

export function SpatialFieldPage({ audio, layout, spatialPath = '' }: { audio: ReturnType<typeof useAnalysis>; layout: Speaker[]; spatialPath?: string }) {
  const mode = spatialModes.find(item => item.id === audio.spatial.mode) ?? spatialModes[0];
  const maxGain = Math.max(0.001, ...layout.map(speaker => Math.abs(speaker.gain)));
  const sources = [...new Set(layout.map(speaker => speaker.source))];
  const heights = layout.filter(speaker => speaker.elevation > 1).length;
  const atmos = mode.id === 'atmos';
  const spatialState = audio.spatialOutput.enabled ? 'Windows spatial on' : 'Windows spatial off';
  const state = `${mode.label} · ${layout.length} virtual speaker${layout.length === 1 ? '' : 's'}${heights ? ` · ${heights} height channel${heights === 1 ? '' : 's'}` : ''}${atmos ? ` · ${spatialState}` : ''} · ${audio.status.playing ? 'playing' : 'idle'}`;
  // Width and height only exist in the manual layout, and crossfeed only in the two headphone
  // modes. Atmos is a fixed 7.1.4 static bed rendered by Windows, so it has no
  // placement controls of its own; every other layout can be turned.
  const manual = mode.id === 'custom';
  const crossfeed = manual || mode.id === 'binaural';
  const shaped = (layout.length > 0 || manual) && !atmos;
  const pair = layout.filter(speaker => speaker.source === 'Left' || speaker.source === 'Right').map(speaker => speaker.azimuth).sort((a, b) => a - b);
  const overhead = layout.filter(speaker => speaker.elevation > 1);
  // The degrees come from the renderer's own layout, not from a second calculation of the same angle.
  const spec = atmos
    ? `7.1.4 bed · ${layout.length} static objects · ${audio.spatialOutput.enabled ? 'Windows spatial on' : 'app-rendered 7.1.4 fallback'}`
    : pair.length
      ? [
        `pair ${pair.map(deg).join(' / ')}`,
        ...(overhead.length ? [`overhead ${Math.round(overhead[0].elevation)}° up`] : []),
        ...(crossfeed ? [`crossfeed ${Math.round(audio.spatial.crossfeed * 100)} %`] : []),
        `rotation ${deg(audio.spatial.rotation)}`,
      ].join(' · ')
      : 'No pair: this layout is a pass-through.';

  return <Page
    title="Spatial field"
    state={state}
  >
    <div className="page-grid field-grid">
      <div className="field-column">
      <article className="page-card field-card">
        <div className="panel-heading"><h2 title={mode.label}>{mode.label}</h2><span className="panel-meta">{layout.length} SPEAKER{layout.length === 1 ? '' : 'S'}{heights ? ` · ${heights} HEIGHT CHANNEL${heights === 1 ? '' : 'S'}` : ''}</span></div>
        <svg className="field-svg" viewBox="0 0 400 356" role="img" aria-label={layout.length === 0 ? 'No virtual speakers: stereo pass-through' : `${layout.length} virtual speakers for ${mode.label}`}>
          {[64, 108, 152].map(radius => <circle key={radius} cx={CENTER.x} cy={CENTER.y} r={radius} className="field-ring" />)}
          <path d={`M${CENTER.x} ${CENTER.y - 16} L${CENTER.x + 9} ${CENTER.y + 6} L${CENTER.x} ${CENTER.y + 1} L${CENTER.x - 9} ${CENTER.y + 6} Z`} className="field-head" />
          <text className="field-axis" x={CENTER.x} y={CENTER.y - 160}>FRONT</text>
          <text className="field-axis" x={CENTER.x - 186} y={CENTER.y}>L</text>
          <text className="field-axis" x={CENTER.x + 180} y={CENTER.y}>R</text>
          <text className="field-axis" x={CENTER.x - 8} y={CENTER.y + 176}>REAR</text>
          {layout.map((speaker, index) => {
            const point = position(speaker, maxGain);
            const color = SOURCE_COLORS[speaker.source] ?? 'var(--accent)';
            return <g key={index}>
              <line x1={CENTER.x} y1={CENTER.y} x2={point.x} y2={point.y} stroke={color} strokeWidth={0.8} opacity={0.28} />
              {speaker.elevation > 1 && <line x1={point.x} y1={point.y} x2={point.x} y2={point.y - speaker.elevation * 0.5} stroke={color} strokeWidth={1.4} opacity={0.5} strokeDasharray="2 3" />}
              <circle cx={point.x} cy={point.y} r={3 + 4 * Math.min(1, Math.abs(speaker.gain) / maxGain)} fill={color} opacity={speaker.gain < 0 ? 0.55 : 0.95} />
              <text className="field-label" x={point.x} y={point.y - 11} textAnchor="middle" fill={color}>{speaker.source}</text>
            </g>;
          })}
          <text className="field-axis" x={CENTER.x} y={CENTER.y + 26} textAnchor="middle">LISTENER</text>
        </svg>
        <p className="panel-note">{layout.length === 0 ? `No virtual speakers. Pass-through: left stays left and right stays right.` : mode.note}</p>
      </article>
      {atmos && <article className="page-card layout-card">
        <div className="panel-heading"><h2>Windows spatial</h2><span className={`panel-meta ${audio.spatialOutput.enabled ? 'live-meta' : ''}`}>{audio.spatialOutput.enabled ? 'ON' : 'OFF'}</span></div>
        <p className="panel-copy">{spatialPath === 'derived' && 'This track fell back to the app’s own 7.1.4 render. '}{audio.spatialOutput.enabled
          ? 'Windows renders this 7.1.4 bed with the spatial format selected for the output — Dolby Atmos for Headphones or Home Theater when that is the format. Width, crossfeed, height and rotation do not apply to a fixed static bed.'
          : audio.spatialOutput.reason || 'Spatial sound is off for this output, so this track uses the app’s own 7.1.4 render.'}{!audio.spatialOutput.enabled && ' A virtual mixer has no spatial format of its own: choose the headphones or receiver on the Devices page, then set Dolby Atmos for that output in Windows.'}</p>
        <p className="layout-spec mono" title={spec}>{spec}</p>
        <p className="panel-note layout-note">The bed is derived from the stereo mix; no Atmos bitstream is decoded here. A format or mode change applies to the next track you start.</p>
        <button className="page-button" onClick={() => void audio.refreshSpatialOutput()}>Check the output again</button>
      </article>}
      {shaped && <article className="page-card layout-card">
        <div className="panel-heading"><h2>Field placement</h2><span className={`panel-meta ${audio.status.playing ? 'live-meta' : ''}`}>{audio.status.playing ? 'LIVE' : 'READY'}</span></div>
        <div className="knob-row placement">
          {manual && <Knob
            label="Width" value={audio.spatial.width} min={0} max={1} step={0.01} ticks={[0, 0.25, 0.5, 0.75, 1]} reset={0.65}
            format={value => `${Math.round(value * 100)} %`}
            description="How far apart the two front speakers sit. The pair above is the resulting angle."
            onChange={value => void audio.setSpatial({ ...audio.spatial, width: value })}
          />}
          {crossfeed && <Knob
            label="Crossfeed" value={audio.spatial.crossfeed} min={0} max={1} step={0.01} ticks={[0, 0.25, 0.5, 0.75, 1]} reset={0.35}
            format={value => `${Math.round(value * 100)} %`}
            description="How much of each channel reaches the opposite ear. Zero keeps the pair fully separated."
            onChange={value => void audio.setSpatial({ ...audio.spatial, crossfeed: value })}
          />}
          {manual && <Knob
            label="Height" value={audio.spatial.height} min={0} max={1} step={0.01} ticks={[0, 0.25, 0.5, 0.75, 1]} reset={0.4}
            format={value => `${Math.round(value * 100)} %`}
            description="How much of the mix is lifted into the overhead pair. Below 1 % the pair is not rendered at all."
            onChange={value => void audio.setSpatial({ ...audio.spatial, height: value })}
          />}
          <Knob
            label="Rotation" value={audio.spatial.rotation} min={-60} max={60} step={1} ticks={[-60, -30, 0, 30, 60]} reset={0} bipolar
            format={value => deg(value)}
            description="Turns the whole field around the listener. Negative turns left, positive turns right."
            onChange={value => void audio.setSpatial({ ...audio.spatial, rotation: value })}
          />
        </div>
        <p className="layout-spec mono" title={spec}>{spec}</p>
        <p className="panel-note layout-note">Levels are constant-power normalised, so changing the field does not change the output level. Nothing here decodes discrete or object audio.</p>
      </article>}
      </div>

      <div className="page-stack">
        <article className="page-card">
          <div className="panel-heading"><h2>Field summary</h2><span className={`panel-meta ${audio.status.playing ? 'live-meta' : ''}`}>{audio.status.playing ? 'ACTIVE' : 'READY'}</span></div>
          <dl className="stat-list">
            <div><dt>Mode</dt><dd title={mode.label}>{mode.label}</dd></div>
            <div><dt>Speakers</dt><dd title={`${layout.length} virtual speakers`}>{layout.length}</dd></div>
            <div><dt>Height channels</dt><dd title={`${heights} speakers above the plane`}>{heights}</dd></div>
            <div><dt>Output</dt><dd title={audio.outputName || 'Windows default'}>{audio.outputName || 'Windows default'}</dd></div>
            {atmos && <div><dt>Spatial output</dt><dd title={audio.spatialOutput.reason || 'Windows spatial sound is on for this output'}>{spatialPath === 'derived' ? 'App 7.1.4 render · this track' : audio.spatialOutput.enabled ? 'Windows spatial · on' : 'Windows spatial · off'}</dd></div>}
          </dl>
          {layout.length > 0 && <ScrollArea className="speaker-table" role="table" ariaLabel="Virtual speaker trim">
            <div className="speaker-head" role="row"><span>Source</span><span>Az</span><span>El</span><span>Trim</span></div>
            {layout.map((speaker, index) => <div key={index} className="speaker-row" role="row">
              <span><i style={{ background: SOURCE_COLORS[speaker.source] ?? 'var(--accent)' }} />{speaker.source}</span>
              <span>{deg(speaker.azimuth)}</span>
              <span>{`${Math.round(speaker.elevation)}°`}</span>
              <span>{trim(speaker.gain)}</span>
            </div>)}
          </ScrollArea>}
        </article>
        <article className="page-card derived-card">
          <div className="panel-heading"><h2>Derived sources</h2><Volume2 size={14} /></div>
          <div className="legend">
            {sources.length === 0 && <span className="panel-note">Pass-through: no channels are derived.</span>}
            {sources.map(source => <span key={source} className="legend-chip"><i style={{ background: SOURCE_COLORS[source] ?? 'var(--accent)' }} />{source}</span>)}
          </div>
          <div className="band-list" role="table" aria-label="Derived band levels">
            {([['Low', audio.status.low], ['Mid', audio.status.mid], ['High', audio.status.high]] as const).map(([label, value]) => {
              const reading = band(audio.status.playing ? value : 0);
              return <div key={label} className="band-row" role="row">
                <span>{label}</span>
                <div className="band-track"><div className="band-fill" style={{ width: audio.status.playing ? reading.width : '0%' }} /></div>
                <strong>{audio.status.playing ? reading.label : '—'}</strong>
              </div>;
            })}
          </div>
          <p className="panel-note">Mid, side, low and high bands are matrix-derived from the stereo mix this app is playing; nothing is decoded from discrete or object audio.</p>
        </article>
        <article className="page-card mode-card">
          <div className="panel-heading"><h2>Modes</h2><span className="panel-meta">WHAT EACH ROUTING DOES</span></div>
          <ScrollArea className="mode-list" role="radiogroup" ariaLabel="Spatial mode">
            {spatialModes.map(item => {
              const active = item.id === audio.spatial.mode;
              return <button
                key={item.id}
                className={`mode-row ${active ? 'active' : ''}`}
                role="radio"
                aria-checked={active}
                aria-label={`${item.label}: ${item.note}`}
                onClick={() => void audio.setSpatial({ ...audio.spatial, mode: item.id })}
              >
                <span className="mode-short mono">{item.short}</span>
                <strong>{item.label}</strong>
                <span className="mode-note">{item.note}</span>
              </button>;
            })}
          </ScrollArea>
        </article>
      </div>
    </div>
  </Page>;
}
