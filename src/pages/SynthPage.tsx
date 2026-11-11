import { useEffect, useMemo, useState } from 'react';
import { ChevronDown, Circle, Save, Square, Trash2 } from 'lucide-react';
import {
  describeSynth, envelopeLine, formatHz, lfoTargets, lfoWaves, modCurves, modSources, modTargets, noteName,
  stageNames, synthFilterKinds, synthLimits, synthWaves,
  type ChainSettings, type StageId, type SynthEnvelope, type SynthFilter, type SynthFilterKind, type SynthLfo,
  type LfoTarget, type LfoWave, type ModCurve, type ModSource, type ModTarget, type SynthMod, type SynthOscillator,
  type SynthPatch, type SynthWave,
} from '../model';
import { Dialog } from '../components/instrument/Dialog';
import { Keyboard } from '../components/instrument/Keyboard';
import { Knob } from '../components/instrument/Knob';
import { ScrollArea } from '../components/instrument/ScrollArea';
import { Segmented } from '../components/instrument/Segmented';
import { SynthScope } from '../components/instrument/SynthScope';
import { WavetableEditor } from '../components/instrument/WavetableEditor';
import { envelopePlot, previewVoice } from '../components/instrument/voice';
import { activePatchName } from '../presets';
import type { useSynth } from '../useSynth';
import { Page } from './Page';

interface Props {
  synth: ReturnType<typeof useSynth>;
  /** A bounce is written into the library, so the page needs to know where it is. */
  libraryRoot: string;
  /** The rack the instrument plays through: its effects section, already running. */
  chain: ChainSettings;
  onOpenProcessors?: () => void;
  onOpenEq?: () => void;
}

/** The panels that have more controls than a faceplate can hold. */
type Panel = 'osc' | 'table' | 'shape' | 'env' | 'mod' | 'take' | 'book';

const percent = (value: number) => `${Math.round(value * 100)} %`;
const signed = (value: number, unit: string) => `${value > 0 ? '+' : value < 0 ? '−' : ''}${Math.abs(value)} ${unit}`;
const seconds = (value: number) => (value >= 1 ? `${value.toFixed(2)} s` : `${Math.round(value * 1000)} ms`);
const peakLabel = (db: number) => (db <= -119 ? '−∞' : `${db.toFixed(1)} dBFS`);
/** The rack's own names are long for a row of six plates; the plate keeps the short one. */
const shortName: Record<StageId, string> = {
  panner: 'Panner', reverb: 'Reverb', eq: 'Equaliser', compressor: 'Compressor', saturator: 'Saturator', limiter: 'Limiter',
};

// The cutoff knob is drawn on a logarithmic scale, because a linear sweep spends
// nine tenths of its travel above 2 kHz where nothing musical happens.
const CUT_LOW = Math.log10(synthLimits.cutoff.min);
const CUT_HIGH = Math.log10(synthLimits.cutoff.max);
const cutoffPosition = (hz: number) => (Math.log10(Math.max(synthLimits.cutoff.min, hz)) - CUT_LOW) / (CUT_HIGH - CUT_LOW);
const cutoffHz = (position: number) => 10 ** (CUT_LOW + Math.min(1, Math.max(0, position)) * (CUT_HIGH - CUT_LOW));

/** Where a tuning moves the keys, and how far the furthest one goes. */
function tuningReport(tuning: number[]) {
  let off = 0;
  let widest = 0;
  let key = 0;
  if (tuning.length !== 128) return { off, widest, key };
  tuning.forEach((offset, index) => {
    if (Math.abs(offset) > 0.05) off += 1;
    if (Math.abs(offset) > Math.abs(widest)) {
      widest = offset;
      key = index;
    }
  });
  return { off, widest, key };
}

/** Whether the patch is playing a tuning exactly as the picker lists it. */
const sameTuning = (a: number[], b: number[]) =>
  a.length > 0 && a.length === b.length && a.every((value, index) => Math.abs(value - b[index]) < 0.05);

/**
 * Which zone a MIDI channel belongs to. The masters sit at the ends of the strip and a
 * zone only exists while it has member channels, so an empty patch shows no zones at all.
 */
function mpeZone(channel: number, lower: number, upper: number): 'master' | 'member' | '' {
  if (channel === 1) return lower ? 'master' : '';
  if (channel === 16) return upper ? 'master' : '';
  if (lower && channel >= 2 && channel <= 1 + lower) return 'member';
  if (upper && channel >= 16 - upper && channel <= 15) return 'member';
  return '';
}

/**
 * An oscillator's panel: where it sits in pitch, how loud it is, how many copies it
 * is made of, and — when it reads a table — which table and where in it.
 */
function Oscillator({ label, osc, tables, onChange, onImport, canImport, busy }: {
  label: string; osc: SynthOscillator; tables: string[]; onChange: (patch: Partial<SynthOscillator>) => void;
  onImport: () => void; canImport: boolean; busy: boolean;
}) {
  return <section className="synth-section" aria-label={label}>
    <div className="synth-section-head">
      <h3>{label}</h3>
      <Segmented
        compact
        label={`${label} waveform`}
        value={osc.wave}
        options={synthWaves.map(wave => ({ id: wave.id, label: wave.label }))}
        onChange={id => onChange({ wave: id as SynthWave })}
      />
    </div>
    <div className="synth-knobs">
      <Knob size={52} label="Octave" value={osc.octave} min={synthLimits.octave.min} max={synthLimits.octave.max} step={1} reset={0} format={value => signed(value, 'oct')} onChange={value => onChange({ octave: value })} />
      <Knob size={52} label="Semitone" value={osc.semitone} min={synthLimits.semitone.min} max={synthLimits.semitone.max} step={1} reset={0} format={value => signed(value, 'st')} onChange={value => onChange({ semitone: value })} />
      <Knob size={52} label="Fine" value={osc.cents} min={synthLimits.cents.min} max={synthLimits.cents.max} step={synthLimits.cents.step} reset={0} bipolar format={value => signed(value, '¢')} onChange={value => onChange({ cents: value })} />
      <Knob size={52} label="Level" value={osc.level} min={synthLimits.oscLevel.min} max={synthLimits.oscLevel.max} step={synthLimits.oscLevel.step} reset={0.8} format={percent} onChange={value => onChange({ level: value })} />
    </div>
    <div className="synth-knobs four">
      <Knob size={52} label="Unison" value={osc.unison} min={synthLimits.unison.min} max={synthLimits.unison.max} step={synthLimits.unison.step} reset={1} format={value => (value === 1 ? 'off' : `${value}×`)} description="Detuned copies summed into this oscillator" onChange={value => onChange({ unison: Math.round(value) })} />
      <Knob size={52} label="Detune" value={osc.detune} min={synthLimits.detune.min} max={synthLimits.detune.max} step={synthLimits.detune.step} reset={0} format={value => `${Math.round(value)} ¢`} description="How far apart the copies are spread, in cents from edge to edge" onChange={value => onChange({ detune: value })} />
      <Knob size={52} label="Spread" value={osc.spread} min={synthLimits.spread.min} max={synthLimits.spread.max} step={synthLimits.spread.step} reset={0} format={percent} description="How far across the image the copies sit" onChange={value => onChange({ spread: value })} />
      <Knob size={52} label={osc.wave === 'table' ? 'Morph' : 'Table'} value={osc.wave === 'table' ? osc.morph : osc.table} min={0} max={osc.wave === 'table' ? synthLimits.morph.max : Math.max(0, tables.length - 1)} step={osc.wave === 'table' ? synthLimits.morph.step : 1} reset={0} format={value => (osc.wave === 'table' ? percent(value) : tables[value] ?? `Table ${value + 1}`)} description={osc.wave === 'table' ? 'Where the reading sits across the table frames' : 'Which table a table oscillator reads'} onChange={value => onChange(osc.wave === 'table' ? { morph: value } : { table: Math.round(value) })} />
    </div>
    {osc.wave === 'table' ? <p className="panel-note">Reading {tables[osc.table] ?? `Table ${osc.table + 1}`} at {percent(osc.morph)}. A table is a stack of frames: the morph walks from the first to the last.</p> : null}
    <div className="synth-field">
      <span className="synth-figures mono">{tables.length} {tables.length === 1 ? 'table' : 'tables'}</span>
      <button
        type="button"
        className="page-button ghost"
        disabled={!canImport || busy}
        title={canImport
          ? 'Bring a wavetable file in: one cycle, or up to eight cycles one after another'
          : 'A table is imported by the Windows app, which reads the file itself'}
        onClick={onImport}
      >Import table…</button>
    </div>
  </section>;
}

/** A curve of the envelope's own recurrence, with the stages marked where they end. */
function EnvelopeGraph({ envelope, label }: { envelope: SynthEnvelope; label: string }) {
  const plot = useMemo(() => envelopePlot(envelope), [envelope]);
  const x = (share: number) => (share * 100).toFixed(2);
  const y = (level: number) => (92 - Math.min(1, Math.max(0, level)) * 84).toFixed(2);
  // The curve leaves zero, follows the envelope, and comes back down to zero.
  const line = `M0,${y(0)}${plot.points.map(point => `L${x(point.x)},${y(point.level)}`).join('')}L100,${y(0)}`;
  return <svg className="synth-envelope-graph" viewBox="0 0 100 100" preserveAspectRatio="none" role="img" aria-label={`${label} shape`}>
    {[25, 50, 75].map(row => <line key={row} className="synth-graph-grid" x1={0} x2={100} y1={row} y2={row} />)}
    {[plot.attackEnd, plot.decayEnd, plot.releaseStart].map((at, index) => at > 0 && at < 1
      ? <line key={index} className="synth-graph-mark" x1={x(at)} x2={x(at)} y1={8} y2={92} />
      : null)}
    <path className="synth-graph-line" d={line} />
  </svg>;
}

function EnvelopeEditor({ label, envelope, onChange }: { label: string; envelope: SynthEnvelope; onChange: (patch: Partial<SynthEnvelope>) => void }) {
  return <section className="synth-section" aria-label={label}>
    <div className="synth-section-head">
      <h3>{label}</h3>
      <span className="synth-figures mono">{envelopeLine(envelope)}</span>
    </div>
    <EnvelopeGraph envelope={envelope} label={label} />
    <div className="synth-knobs">
      <Knob size={52} label="Attack" value={envelope.attack} min={synthLimits.attack.min} max={synthLimits.attack.max} step={synthLimits.attack.step} reset={0.005} format={seconds} onChange={value => onChange({ attack: value })} />
      <Knob size={52} label="Decay" value={envelope.decay} min={synthLimits.decay.min} max={synthLimits.decay.max} step={synthLimits.decay.step} reset={0.25} format={seconds} onChange={value => onChange({ decay: value })} />
      <Knob size={52} label="Sustain" value={envelope.sustain} min={synthLimits.sustain.min} max={synthLimits.sustain.max} step={synthLimits.sustain.step} reset={0.7} format={percent} onChange={value => onChange({ sustain: value })} />
      <Knob size={52} label="Release" value={envelope.release} min={synthLimits.release.min} max={synthLimits.release.max} step={synthLimits.release.step} reset={0.35} format={seconds} onChange={value => onChange({ release: value })} />
    </div>
  </section>;
}

export function SynthPage({ synth, libraryRoot, chain, onOpenProcessors, onOpenEq }: Props) {
  const { patch, status, patches } = synth;
  const [panel, setPanel] = useState<Panel | null>(null);
  // The QWERTY row is on by default: this is an instrument page, and the letters
  // are the first keyboard most people have. A text field takes them back.
  const [computer, setComputer] = useState(true);
  const [name, setName] = useState('');
  const [saving, setSaving] = useState(false);
  const [editingTable, setEditingTable] = useState<number | null>(null);
  const [tableRevision, setTableRevision] = useState(0);
  const { refreshDevices } = synth;

  // A port list is what the machine has now, so it is read when the page opens
  // rather than remembered from the last launch.
  useEffect(() => { void refreshDevices(); }, [refreshDevices]);

  // A table oscillator's shape belongs to the engine that will read it, so the
  // page asks for the frame it is about to play rather than inventing one. It is
  // asked again only when the table or the morph moves.
  const [cycles, setCycles] = useState<{ one?: number[]; two?: number[] }>({});
  useEffect(() => {
    let live = true;
    void (async () => {
      const one = patch.osc1.wave === 'table'
        ? await synth.tableShape(patch.osc1.table, patch.osc1.morph)
        : undefined;
      const two = patch.osc2.wave === 'table'
        ? await synth.tableShape(patch.osc2.table, patch.osc2.morph)
        : undefined;
      if (live) setCycles({ one, two });
    })();
    return () => { live = false; };
  }, [synth.tableShape, patch.osc1.wave, patch.osc1.table, patch.osc1.morph, patch.osc2.wave, patch.osc2.table, patch.osc2.morph, tableRevision]);

  // The display is redrawn when the patch changes, never per poll: one preview is
  // a note's worth of frames, and the same patch always draws the same note.
  const preview = useMemo(() => previewVoice(patch, 69, undefined, cycles), [patch, cycles]);
  const run = status.running;

  const setOsc1 = (next: Partial<SynthOscillator>) => synth.edit({ osc1: { ...patch.osc1, ...next } });
  const setOsc2 = (next: Partial<SynthOscillator>) => synth.edit({ osc2: { ...patch.osc2, ...next } });
  const setFilter = (next: Partial<SynthFilter>) => synth.edit({ filter: { ...patch.filter, ...next } });
  const setLfo = (next: Partial<SynthLfo>) => synth.edit({ lfo: { ...patch.lfo, ...next } });
  const setLfo2 = (next: Partial<SynthLfo>) => synth.edit({ lfo2: { ...patch.lfo2, ...next } });
  const setMods = (mods: SynthMod[]) => synth.edit({ mods });
  const setAmp = (next: Partial<SynthEnvelope>) => synth.edit({ amp: { ...patch.amp, ...next } });
  const setFilterEnv = (next: Partial<SynthEnvelope>) => synth.edit({ filterEnv: { ...patch.filterEnv, ...next } });

  /**
   * A table is brought in by the engine, not the page: the Rust side cuts the file
   * into cycles, keeps their harmonics and answers with the index the table was
   * given. The oscillator that asked is pointed at it, so the click is heard.
   */
  const importTable = async (which: 1 | 2) => {
    const index = await synth.chooseTable();
    if (index === null) return;
    if (which === 1) setOsc1({ wave: 'table', table: index });
    else setOsc2({ wave: 'table', table: index });
  };

  /** A Scala file becomes the patch's tuning, cents per key and nothing else. */
  const importScale = async () => {
    const scale = await synth.chooseScale();
    if (scale) synth.edit({ tuning: scale.cents.slice(0, 128) });
  };

  // A pack is the engine's own shelf too, so the header names a pack patch as well
  // as a shipped or saved one.
  const activeName = patches.active
    ? activePatchName(synth.catalogue, patches.presets, patches.active) || 'Unsaved'
    : 'Unsaved patch';
  const stored = patches.presets.filter(preset => !preset.builtin);
  const activeSaved = patches.active && stored.some(preset => preset.id === patches.active);
  const enabled = chain.order.filter(stage => chain[stage].enabled).length;
  const enabledEffects = chain.effects.filter(effect => effect.enabled).length;
  const wave = (id: SynthWave) => synthWaves.find(entry => entry.id === id)?.label ?? id;
    const filterName = (kind: SynthFilterKind) => synthFilterKinds.find(entry => entry.id === kind)?.label ?? kind;
  const tuning = tuningReport(patch.tuning);
  const tuningLine = tuning.off
    ? `${tuning.off} ${tuning.off === 1 ? 'key' : 'keys'} off equal · widest ${signed(Number(tuning.widest.toFixed(1)), '¢')} at ${noteName(tuning.key)}`
    : 'Equal temperament';
  const chosenTuning = synth.scales.find(scale => sameTuning(scale.cents, patch.tuning))?.name ?? '';

  return <Page
    title="Synth"
    state={describeSynth(patch, status)}
    actions={<div className="page-actions-row">
      <button
        type="button"
        className="synth-patch-chip"
        aria-haspopup="dialog"
        title={`Patch book · ${activeName}`}
        onClick={() => setPanel('book')}
      ><span>{activeName}</span><ChevronDown size={13} aria-hidden="true" /></button>
      <button
        type="button"
        role="switch"
        aria-checked={run}
        className={`toggle-line ${run ? 'on' : ''}`}
        title={run ? 'Stop the instrument output' : 'Start the instrument output'}
        onClick={() => void (run ? synth.stop() : synth.start())}
      ><i aria-hidden="true" />{run ? 'Running' : 'Stopped'}</button>
      <span className={`source-badge ${run ? 'live' : ''}`}><i />{run ? `${status.voices} ${status.voices === 1 ? 'voice' : 'voices'}` : 'Silent'}</span>
    </div>}
  >
    {!synth.native && <p className="audio-warning">The instrument runs in the Windows app. These controls drive the real engine; nothing here is simulated.</p>}
    {synth.error && <p className="audio-error error-message">{synth.error}</p>}

    <div className="synth-page">
      <article className="page-card synth-display">
        <div className="panel-heading">
          <h2>Voice field</h2>
          <span className="panel-meta mono">{run ? `output · ${status.device || 'device'}` : 'preview · A4'}</span>
        </div>
        <div className="synth-tabs">
          <button type="button" className="synth-tab" aria-haspopup="dialog" onClick={() => setPanel('osc')}>
            <span>Oscillators</span><em>{wave(patch.osc1.wave)} + {wave(patch.osc2.wave)}</em>
          </button>
          <button type="button" className="synth-tab" aria-haspopup="dialog" onClick={() => setPanel('shape')}>
            <span>Filter &amp; LFO</span><em>{filterName(patch.filter.kind)} {formatHz(patch.filter.cutoff)}</em>
          </button>
          <button type="button" className="synth-tab" aria-haspopup="dialog" onClick={() => setPanel('env')}>
            <span>Envelopes</span><em>A {seconds(patch.amp.attack)} · D {seconds(patch.amp.decay)}</em>
          </button>
          <button type="button" className="synth-tab" aria-haspopup="dialog" onClick={() => setPanel('mod')}>
            <span>Modulation</span><em>{patch.mods.length ? `${patch.mods.length} ${patch.mods.length === 1 ? 'route' : 'routes'}` : 'no routes'}</em>
          </button>
          <button type="button" className="synth-tab" aria-haspopup="dialog" onClick={() => setPanel('take')}>
            <span>Take &amp; MIDI</span><em>{status.recording ? `${status.recordedSeconds.toFixed(1)} s captured` : status.midi || 'no input'}</em>
          </button>
        </div>
        <div className="synth-screen">
          <SynthScope scope={status.scope} preview={preview} running={run} />
          {/* The key to the traces sits on the glass, not under it: the display keeps
              its whole height at every window size. */}
          <div className="synth-screen-tags mono" aria-hidden="true">
            <span><i className="swatch one" />osc 1</span>
            <span><i className="swatch two" />osc 2</span>
            <span><i className="swatch body" />voice</span>
            <span className="synth-screen-peak">{peakLabel(status.peakDb)}</span>
          </div>
        </div>
        {/* One row, seven controls: the three the hand reaches for while playing and
            the four macros the matrix reads. Env amount, the LFO pair and glide live in
            their own dialogs, so the faceplate never grows a second row. */}
        <div className="synth-macros">
          <Knob size={52} label="Cutoff" value={cutoffPosition(patch.filter.cutoff)} min={0} max={1} step={0.001} reset={cutoffPosition(3200)} format={value => formatHz(cutoffHz(value))} announced={{ min: synthLimits.cutoff.min, max: synthLimits.cutoff.max, now: Math.round(patch.filter.cutoff) }} onChange={value => setFilter({ cutoff: Math.round(cutoffHz(value)) })} />
          <Knob size={52} label="Resonance" value={patch.filter.resonance} min={synthLimits.resonance.min} max={synthLimits.resonance.max} step={synthLimits.resonance.step} reset={0.15} format={percent} onChange={value => setFilter({ resonance: value })} />
          <Knob size={52} label="Output" value={patch.level} min={synthLimits.level.min} max={synthLimits.level.max} step={synthLimits.level.step} reset={0.7} format={percent} description="The level the voices leave at, before the rack" onChange={value => synth.edit({ level: value })} />
          {patch.macros.map((value, index) => <Knob
            key={`macro-${index}`}
            size={52}
            label={`Macro ${index + 1}`}
            value={value}
            min={synthLimits.macro.min}
            max={synthLimits.macro.max}
            step={synthLimits.macro.step}
            reset={0}
            format={percent}
            description={`Performance macro ${index + 1} — route it in the matrix`}
            onChange={next => synth.edit({
              macros: patch.macros.map((entry, at) => (at === index ? next : entry)) as SynthPatch['macros'],
            })}
          />)}
        </div>
      </article>

      <article className="page-card synth-fx">
        <div className="panel-heading">
          <h2>Effects</h2>
          <span className="panel-meta mono">{enabled || enabledEffects ? `${enabled} of 6 stages · ${enabledEffects} additional FX` : 'all bypassed'}</span>
        </div>
        <div className="synth-fx-row">
          {chain.order.map((stage, index) => <button
            key={stage}
            type="button"
            className={`synth-fx-chip ${chain[stage].enabled ? 'on' : ''}`}
            title={`${stageNames[stage]} — ${chain[stage].enabled ? 'in the path' : 'bypassed'}`}
            onClick={() => (stage === 'eq' ? onOpenEq?.() : onOpenProcessors?.())}
          >
            <span className="mono synth-fx-index">{String(index + 1).padStart(2, '0')}</span>
            <i className="synth-fx-lamp" aria-hidden="true" />
            <span className="synth-fx-name">{shortName[stage]}</span>
          </button>)}
        </div>
        <p className="panel-note">The rack is the instrument's effects section: its twelve additional effects run on the instrument and its bounce.</p>
      </article>

      <article className="page-card synth-keys">
        <div className="panel-heading">
          <h2>Keyboard</h2>
          <div className="synth-keyboard-tools">
            <button
              type="button"
              role="switch"
              aria-checked={computer}
              className={`toggle-line ${computer ? 'on' : ''}`}
              onClick={() => setComputer(!computer)}
            ><i aria-hidden="true" />QWERTY</button>
            <button type="button" className="page-button ghost" onClick={synth.allNotesOff}>All notes off</button>
          </div>
        </div>
        <Keyboard
          lowNote={48}
          octaves={2}
          held={synth.held}
          computer={computer}
          onNoteOn={synth.noteOn}
          onNoteOff={synth.noteOff}
        />
      </article>
    </div>

    <Dialog
      title="Oscillators"
      meta={`${wave(patch.osc1.wave)} + ${wave(patch.osc2.wave)} · ${signed(patch.osc2.octave, 'oct')} · sub ${percent(patch.sub)}`}
      open={panel === 'osc'}
      onClose={() => setPanel(null)}
    >
      <div className="synth-panel-grid">
        <Oscillator label="Oscillator 1" osc={patch.osc1} tables={patches.tables} onChange={setOsc1} onImport={() => void importTable(1)} canImport={synth.native} busy={synth.busy} />
        <Oscillator label="Oscillator 2" osc={patch.osc2} tables={patches.tables} onChange={setOsc2} onImport={() => void importTable(2)} canImport={synth.native} busy={synth.busy} />
      </div>
      <section className="synth-section" aria-label="Mix">
        <div className="synth-section-head"><h3>Mix</h3><span className="synth-figures mono">sub · noise · glide</span></div>
        <div className="synth-knobs four">
          <Knob size={52} label="Sub" value={patch.sub} min={synthLimits.sub.min} max={synthLimits.sub.max} step={synthLimits.sub.step} reset={0} format={percent} description="Square an octave under oscillator one" onChange={value => synth.edit({ sub: value })} />
          <Knob size={52} label="Noise" value={patch.noise} min={synthLimits.noise.min} max={synthLimits.noise.max} step={synthLimits.noise.step} reset={0} format={percent} onChange={value => synth.edit({ noise: value })} />
          <Knob size={52} label="Glide" value={patch.glideMs} min={synthLimits.glide.min} max={synthLimits.glide.max} step={synthLimits.glide.step} reset={0} format={value => (value === 0 ? 'off' : `${Math.round(value)} ms`)} description="How long a new note takes to reach its pitch from the last one" onChange={value => synth.edit({ glideMs: value })} />
          <Knob size={52} label="Output" value={patch.level} min={synthLimits.level.min} max={synthLimits.level.max} step={synthLimits.level.step} reset={0.7} format={percent} description="The level the voices leave at, before the rack" onChange={value => synth.edit({ level: value })} />
        </div>
      </section>
      {synth.native && <section className="synth-section" aria-label="Wavetable editor">
        <div className="synth-section-head">
          <h3>Wavetable editor</h3>
          <span className="synth-figures mono">{patches.tables.length > 4 ? `${patches.tables.length - 4} imported` : 'no imported tables'}</span>
        </div>
        <div className="synth-field">
          <button
            type="button"
            className="page-button"
            disabled={patches.tables.length <= 4}
            title={patches.tables.length > 4
              ? 'Draw a table’s frames and warp its spectrum, with undo for every edit'
              : 'Import a table from an oscillator first'}
            onClick={() => {
              // The editor opens on something: the table left in hand, or the first imported one.
              setEditingTable(current => current ?? (patches.tables.length > 4 ? 4 : null));
              setPanel('table');
            }}
          >Open the editor</button>
        </div>
        <p className="panel-note">{patches.tables.length > 4
          ? 'The editor has a panel of its own, so the waveform, its partials and the three ways of shaping them get the room they need.'
          : 'Import a table from an oscillator to shape it here. A table is read as its first 128 harmonics, so it plays clean at every pitch, and the file is kept between launches.'}</p>
      </section>}
    </Dialog>

    <Dialog
      title="Wavetable editor"
      meta={editingTable !== null && patches.tables[editingTable]
        ? `${patches.tables[editingTable]} · ${patches.tables.length - 4} imported ${patches.tables.length - 4 === 1 ? 'table' : 'tables'}`
        : 'no table open'}
      open={panel === 'table'}
      onClose={() => setPanel(null)}
    >
      <section className="synth-section" aria-label="Table">
        <div className="synth-section-head">
          <h3>Table</h3>
          <select
            className="synth-select"
            aria-label="Select table to edit"
            value={editingTable ?? ''}
            onChange={event => setEditingTable(event.target.value === '' ? null : Number(event.target.value))}
          >
            <option value="">{patches.tables.length > 4 ? 'Choose a table' : 'No imported tables'}</option>
            {patches.tables.slice(4).map((name, index) => <option key={index + 4} value={index + 4}>{name}</option>)}
          </select>
        </div>
        {editingTable !== null
          ? <WavetableEditor key={editingTable} table={editingTable} onSaved={() => setTableRevision(value => value + 1)} />
          : <p className="panel-note">Choose an imported table to draw its frames or warp its spectrum. An edit keeps the table’s index, so every patch that points at it follows.</p>}
      </section>
    </Dialog>

    <Dialog
      title="Filter and movement"
      meta={`${filterName(patch.filter.kind)} ${formatHz(patch.filter.cutoff)} · resonance ${percent(patch.filter.resonance)} · LFO ${patch.lfo.rate.toFixed(2)} Hz`}
      open={panel === 'shape'}
      onClose={() => setPanel(null)}
    >
      <div className="synth-panel-grid">
        <section className="synth-section" aria-label="Filter">
          <div className="synth-section-head">
            <h3>Filter</h3>
            <select
              className="synth-select"
              aria-label="Filter type"
              value={patch.filter.kind}
              onChange={event => setFilter({ kind: event.target.value as SynthFilterKind })}
            >
              {synthFilterKinds.map(kind => <option key={kind.id} value={kind.id}>{kind.label}</option>)}
            </select>
          </div>
          <div className="synth-knobs dense">
            <Knob size={44} label="Cutoff" value={cutoffPosition(patch.filter.cutoff)} min={0} max={1} step={0.001} reset={cutoffPosition(3200)} format={value => formatHz(cutoffHz(value))} announced={{ min: synthLimits.cutoff.min, max: synthLimits.cutoff.max, now: Math.round(patch.filter.cutoff) }} onChange={value => setFilter({ cutoff: Math.round(cutoffHz(value)) })} />
            <Knob size={44} label="Resonance" value={patch.filter.resonance} min={synthLimits.resonance.min} max={synthLimits.resonance.max} step={synthLimits.resonance.step} reset={0.15} format={percent} onChange={value => setFilter({ resonance: value })} />
            <Knob size={44} label="Env amount" value={patch.filter.envAmount} min={synthLimits.envAmount.min} max={synthLimits.envAmount.max} step={synthLimits.envAmount.step} reset={0.35} bipolar format={value => signed(Number(value.toFixed(2)), 'oct')} description="Octaves the filter envelope moves the cutoff" onChange={value => setFilter({ envAmount: value })} />
            <Knob size={44} label="Key track" value={patch.filter.keyTrack} min={synthLimits.keyTrack.min} max={synthLimits.keyTrack.max} step={synthLimits.keyTrack.step} reset={0.3} format={percent} description="How much the cutoff follows the key: one is an octave per octave" onChange={value => setFilter({ keyTrack: value })} />
            <Knob size={44} label="Drive" value={patch.filter.drive} min={synthLimits.drive.min} max={synthLimits.drive.max} step={synthLimits.drive.step} reset={0} format={percent} description="How hard the filter is pushed: the ladder hears it as saturation, the linear models are compensated so it is character rather than level" onChange={value => setFilter({ drive: value })} />
          </div>
          <p className="panel-note">{patch.filter.kind === 'ladder'
            ? 'Four one-poles in a ladder, saturated at the input and run at twice the rate: the peak an analog filter makes when it is pushed. Drive is what pushes it.'
            : patch.filter.kind === 'comb'
              ? 'A tuned delay fed back on itself: the cutoff is the delay\u2019s pitch, so the filter is heard as a note rather than a slope.'
              : patch.filter.kind === 'notch'
                ? 'The band the state variable rejects: one frequency taken out of the note, which is a hole in the mix rather than a slope.'
                : patch.filter.kind === 'peak'
                  ? 'The band the state variable lifts: one frequency pushed forward, which is a formant when the resonance is high.'
                  : 'A topology-preserving state-variable filter. The envelope sweeps the cutoff; either LFO can move it, the pitch or the level.'}</p>
        </section>
        <section className="synth-section" aria-label="Low frequency oscillator">
          <div className="synth-section-head">
            <h3>LFO</h3>
            <Segmented
              compact
              label="LFO shape"
              value={patch.lfo.wave}
              options={lfoWaves.map(entry => ({ id: entry.id, label: entry.label }))}
              onChange={id => setLfo({ wave: id as LfoWave })}
            />
          </div>
          <div className="synth-knobs four">
            <Knob size={52} label="Rate" value={patch.lfo.rate} min={synthLimits.rate.min} max={synthLimits.rate.max} step={synthLimits.rate.step} reset={5} format={value => `${value.toFixed(2)} Hz`} onChange={value => setLfo({ rate: value })} />
            <Knob size={52} label="Depth" value={patch.lfo.depth} min={synthLimits.depth.min} max={synthLimits.depth.max} step={synthLimits.depth.step} reset={0} format={percent} onChange={value => setLfo({ depth: value })} />
          </div>
          <div className="synth-field">
            <span className="synth-figures">Destination</span>
            <Segmented
              label="LFO destination"
              value={patch.lfo.target}
              options={lfoTargets.map(target => ({ id: target.id, label: target.label }))}
              onChange={id => setLfo({ target: id as LfoTarget })}
            />
          </div>
          <p className="panel-note">One LFO, shared by every voice: a per-voice LFO would drift out of phase and the movement would be lost.</p>
        </section>
      </div>
    </Dialog>

    <Dialog
      title="Envelopes"
      meta={`amp ${envelopeLine(patch.amp)}`}
      open={panel === 'env'}
      onClose={() => setPanel(null)}
    >
      <div className="synth-panel-grid">
        <EnvelopeEditor label="Amplitude envelope" envelope={patch.amp} onChange={setAmp} />
        <EnvelopeEditor label="Filter envelope" envelope={patch.filterEnv} onChange={setFilterEnv} />
      </div>
      <p className="panel-note">Both envelopes are drawn from their own recurrence: a linear attack, and an exponential decay and release, which is what a tail does.</p>
    </Dialog>

    <Dialog
      title="Modulation"
      meta={`${patch.mods.length} ${patch.mods.length === 1 ? 'route' : 'routes'} · LFO 2 ${patch.lfo2.rate.toFixed(2)} Hz`}
      open={panel === 'mod'}
      onClose={() => setPanel(null)}
    >
      <section className="synth-section" aria-label="Modulation routes">
        <div className="synth-section-head">
          <h3>Matrix</h3>
          <button
            type="button"
            className="page-button ghost"
            disabled={patch.mods.length >= 8}
            onClick={() => setMods([...patch.mods, { source: 'lfo1', target: 'cutoff', amount: 0.3, curve: 'linear' }])}
          >Add route</button>
        </div>
        {patch.mods.length ? <div className="synth-mod-list">
          {patch.mods.map((route, index) => <div className="synth-mod-route" key={`${index}-${route.source}-${route.target}`}>
            <select
              className="synth-select"
              aria-label={`Route ${index + 1} source`}
              value={route.source}
              onChange={event => setMods(patch.mods.map((entry, at) => (at === index ? { ...entry, source: event.target.value as ModSource } : entry)))}
            >
              {modSources.map(source => <option key={source.id} value={source.id}>{source.label}</option>)}
            </select>
            <span className="synth-mod-arrow mono" aria-hidden="true">→</span>
            <select
              className="synth-select"
              aria-label={`Route ${index + 1} destination`}
              value={route.target}
              onChange={event => setMods(patch.mods.map((entry, at) => (at === index ? { ...entry, target: event.target.value as ModTarget } : entry)))}
            >
              {modTargets.map(target => <option key={target.id} value={target.id}>{target.label}</option>)}
            </select>
            <Knob
              size={44}
              label="Amount"
              value={route.amount}
              min={synthLimits.modAmount.min}
              max={synthLimits.modAmount.max}
              step={synthLimits.modAmount.step}
              reset={0}
              bipolar
              format={value => signed(Number(value.toFixed(2)), '×')}
              description="Pitch and cutoff are in octaves, everything else a share of the parameter"
              onChange={value => setMods(patch.mods.map((entry, at) => (at === index ? { ...entry, amount: value } : entry)))}
            />
            <select
              className="synth-select synth-mod-curve"
              aria-label={`Route ${index + 1} curve`}
              value={route.curve}
              onChange={event => setMods(patch.mods.map((entry, at) => (at === index ? { ...entry, curve: event.target.value as ModCurve } : entry)))}
            >
              {modCurves.map(curve => <option key={curve.id} value={curve.id}>{curve.label}</option>)}
            </select>
            <button
              type="button"
              className="icon-button"
              aria-label={`Remove route ${index + 1}`}
              onClick={() => setMods(patch.mods.filter((_, at) => at !== index))}
            ><Trash2 size={13} /></button>
          </div>)}
        </div> : <p className="panel-note">No routes. A route sends an LFO, an envelope, velocity, the key, the wheel, aftertouch or a macro to one of the patch’s own parameters.</p>}
      </section>

      <section className="synth-section" aria-label="Performance macros">
        <div className="synth-section-head">
          <h3>Macros</h3>
          <span className="synth-figures mono">matrix sources</span>
        </div>
        <div className="synth-knobs four">
          {patch.macros.map((value, index) => <Knob
            key={`panel-macro-${index}`}
            size={52}
            label={`Macro ${index + 1}`}
            value={value}
            min={synthLimits.macro.min}
            max={synthLimits.macro.max}
            step={synthLimits.macro.step}
            reset={0}
            format={percent}
            description={`Performance macro ${index + 1} — pick it as a source and one knob moves several destinations`}
            onChange={next => synth.edit({
              macros: patch.macros.map((entry, at) => (at === index ? next : entry)) as SynthPatch['macros'],
            })}
          />)}
        </div>
        <p className="panel-note">Four knobs the matrix can read. A macro holds a value, not a wire: route it once and every destination it feeds moves together, which is how one gesture becomes a performance.</p>
      </section>

      <div className="synth-panel-grid">
        <section className="synth-section" aria-label="Second LFO">
          <div className="synth-section-head">
            <h3>LFO 2</h3>
            <Segmented
              compact
              label="Second LFO shape"
              value={patch.lfo2.wave}
              options={lfoWaves.map(entry => ({ id: entry.id, label: entry.label }))}
              onChange={id => setLfo2({ wave: id as LfoWave })}
            />
          </div>
          <div className="synth-knobs four">
            <Knob size={52} label="Rate" value={patch.lfo2.rate} min={synthLimits.rate.min} max={synthLimits.rate.max} step={synthLimits.rate.step} reset={0.8} format={value => `${value.toFixed(2)} Hz`} onChange={value => setLfo2({ rate: value })} />
            <Knob size={52} label="Depth" value={patch.lfo2.depth} min={synthLimits.depth.min} max={synthLimits.depth.max} step={synthLimits.depth.step} reset={0} format={percent} onChange={value => setLfo2({ depth: value })} />
          </div>
          <div className="synth-field">
            <span className="synth-figures">Destination</span>
            <Segmented
              label="Second LFO destination"
              value={patch.lfo2.target}
              options={lfoTargets.map(target => ({ id: target.id, label: target.label }))}
              onChange={id => setLfo2({ target: id as LfoTarget })}
            />
          </div>
          <p className="panel-note">Both LFOs run on every voice at once. A route is summed with the other routes to the same destination, so two small movements make one large one.</p>
        </section>

        <section className="synth-section" aria-label="What a route does">
          <div className="synth-section-head"><h3>Units</h3><span className="synth-figures mono">source × amount</span></div>
          <dl className="synth-facts">
            <div><dt>Pitch</dt><dd>Twelve semitones at full amount</dd></div>
            <div><dt>Cutoff</dt><dd>Four octaves at full amount</dd></div>
            <div><dt>Resonance</dt><dd>One whole step of the knob</dd></div>
            <div><dt>Levels</dt><dd>A share of the oscillator, sub or noise</dd></div>
            <div><dt>Pan</dt><dd>Edge to edge of the image</dd></div>
          </dl>
          <p className="panel-note">Velocity and key are per voice; the wheel scales both LFOs as well as being a source of its own.</p>
        </section>
      </div>
    </Dialog>

    <Dialog
      title="Take and MIDI"
      meta={run ? `output running · ${status.voices} / 16 voices` : 'output stopped'}
      open={panel === 'take'}
      onClose={() => setPanel(null)}
    >
      <div className="synth-panel-grid">
        <section className="synth-section" aria-label="Transport">
          <div className="synth-section-head"><h3>Transport</h3><span className="synth-figures mono">{peakLabel(status.peakDb)}</span></div>
          <div className="synth-run-row">
            <button type="button" className="page-button" onClick={() => void synth.start()} disabled={synth.busy || run}><Circle size={13} aria-hidden="true" />Start output</button>
            <button type="button" className="page-button" onClick={() => void synth.stop()} disabled={!run}><Square size={13} aria-hidden="true" />Stop</button>
          </div>
          <div className="synth-run-row">
            <button
              type="button"
              role="switch"
              aria-checked={status.recording}
              className={`toggle-line ${status.recording ? 'on' : ''}`}
              onClick={() => void synth.record(!status.recording)}
              disabled={!run}
            ><i aria-hidden="true" />Record</button>
            <span className="synth-figures mono">{status.recording ? `${status.recordedSeconds.toFixed(1)} s captured` : 'capture is off'}</span>
          </div>
          <div className="synth-levels" aria-label="Instrument output">
            <span className="rack-label">Peak</span>
            <div className="synth-peak" title={peakLabel(status.peakDb)}>
              <i style={{ width: `${Math.max(0, Math.min(100, ((status.peakDb + 60) / 60) * 100))}%` }} />
            </div>
            <span className="mono">{peakLabel(status.peakDb)}</span>
          </div>
          <button
            type="button"
            className="page-button"
            disabled={synth.busy || !libraryRoot}
            title={libraryRoot ? 'Render the take through the rack into the library' : 'Choose a library folder first'}
            onClick={() => void synth.bounce(libraryRoot)}
          >Bounce to WAV</button>
          <p className="panel-note">{libraryRoot
            ? `A bounce is written to ${libraryRoot} · Synth, through the rack and at 48 kHz.`
            : 'Choose a library folder on the Library page: a bounce is written into the library.'}</p>
          {synth.bounced && <p className="panel-note synth-bounced" title={synth.bounced}>Written: {synth.bounced}</p>}
        </section>

        <section className="synth-section" aria-label="MIDI input">
          <div className="synth-section-head">
            <h3>MIDI input</h3>
            <button type="button" className="page-button ghost" onClick={() => void synth.refreshDevices()}>Rescan</button>
          </div>
          <select
            className="synth-midi"
            aria-label="MIDI input"
            value={status.midi ? String(synth.devices.indexOf(status.midi)) : ''}
            onChange={event => {
              if (event.target.value === '') void synth.closeDevice();
              else void synth.openDevice(Number(event.target.value));
            }}
          >
            <option value="">No input</option>
            {synth.devices.map((device, index) => <option key={`${device}-${index}`} value={String(index)}>{device}</option>)}
          </select>
          <p className="panel-note" title={synth.deviceError || status.midi}>
            {synth.deviceError ? `Port error: ${synth.deviceError}` : status.midi ? `Playing from ${status.midi}.` : `${synth.devices.length} ${synth.devices.length === 1 ? 'port' : 'ports'} found. Notes, velocity, pitch bend and the mod wheel are read here.`}
          </p>
          <p className="panel-note">A take keeps the notes it heard and the patch it heard them with; a bounce renders them through the rack offline, so it is the same every time.</p>
        </section>

        <section className="synth-section" aria-label="MPE settings">
          <div className="synth-section-head">
            <h3>MPE zones</h3>
            <span className="synth-figures mono">{(patches.mpe.lowerMembers || patches.mpe.upperMembers)
              ? `${patches.mpe.lowerMembers} lower · ${patches.mpe.upperMembers} upper`
              : 'channel expression'}</span>
          </div>
          <div className="synth-entries">
            <label className="synth-entry"><span>Lower members</span><input aria-label="Lower MPE member channels" type="number" min={0} max={patches.mpe.upperMembers ? 14 - patches.mpe.upperMembers : 15} value={patches.mpe.lowerMembers} onChange={event => {
              const limit = patches.mpe.upperMembers ? 14 - patches.mpe.upperMembers : 15;
              void synth.setMpe({ ...patches.mpe, lowerMembers: Math.max(0, Math.min(limit, Math.floor(Number(event.target.value) || 0))) });
            }} /></label>
            <label className="synth-entry"><span>Upper members</span><input aria-label="Upper MPE member channels" type="number" min={0} max={patches.mpe.lowerMembers ? 14 - patches.mpe.lowerMembers : 15} value={patches.mpe.upperMembers} onChange={event => {
              const limit = patches.mpe.lowerMembers ? 14 - patches.mpe.lowerMembers : 15;
              void synth.setMpe({ ...patches.mpe, upperMembers: Math.max(0, Math.min(limit, Math.floor(Number(event.target.value) || 0))) });
            }} /></label>
            <label className="synth-entry"><span>Member bend ± st</span><input aria-label="MPE member bend range" type="number" min={0} max={48} value={patches.mpe.memberBend} onChange={event => void synth.setMpe({ ...patches.mpe, memberBend: Math.max(0, Math.min(48, Math.floor(Number(event.target.value) || 0))) })} /></label>
            <label className="synth-entry"><span>Master bend ± st</span><input aria-label="MPE master bend range" type="number" min={0} max={48} value={patches.mpe.masterBend} onChange={event => void synth.setMpe({ ...patches.mpe, masterBend: Math.max(0, Math.min(48, Math.floor(Number(event.target.value) || 0))) })} /></label>
          </div>
          <div className="mpe-map" aria-hidden="true">
            {Array.from({ length: 16 }, (_, index) => {
              const channel = index + 1;
              const zone = mpeZone(channel, patches.mpe.lowerMembers, patches.mpe.upperMembers);
              return <span key={channel} className={zone} title={`Channel ${channel}${zone ? ` · ${zone}` : ' · unused'}`}>{channel}</span>;
            })}
          </div>
          <p className="panel-note">Zero disables a zone. Lower uses channel 1 as master; upper uses channel 16. Route Pressure and MPE timbre in the modulation matrix. Changing zones releases held notes.</p>
        </section>

        <section className="synth-section" aria-label="Tuning">
          <div className="synth-section-head">
            <h3>Tuning</h3>
            <span className="synth-figures mono" title={tuningLine}>{tuningLine}</span>
          </div>
          <div className="synth-field">
            <select
              className="synth-select"
              aria-label="Tuning"
              value={chosenTuning}
              onChange={event => {
                const scale = synth.scales.find(entry => entry.name === event.target.value);
                if (!scale) return;
                // A tuning of zeroes is equal temperament, and the empty list is how
                // the engine is told so.
                synth.edit({ tuning: scale.cents.some(offset => Math.abs(offset) > 0.05) ? scale.cents.slice(0, 128) : [] });
              }}
            >
              <option value="">{patch.tuning.length ? 'Custom tuning' : 'Equal temperament'}</option>
              {synth.scales.map(scale => <option key={scale.name} value={scale.name}>{scale.name}</option>)}
            </select>
            {/* The picker already names the tuning in hand, so the way back to equal
                temperament is only offered while the patch is off it. */}
            {tuning.off > 0 && <button type="button" className="page-button ghost" onClick={() => synth.edit({ tuning: [] })}>Equal temperament</button>}
            <button
              type="button"
              className="page-button ghost"
              disabled={!synth.native}
              title={synth.native ? 'Read a Scala .scl file into this patch' : 'A tuning is imported by the Windows app, which reads the file itself'}
              onClick={() => void importScale()}
            >Import Scala…</button>
          </div>
          <p className="panel-note">A tuning is cents of offset from equal temperament on each key, added before the glide: it is the pitch of the key, not an effect after it. {tuning.off
            ? `Here it moves ${tuning.off} ${tuning.off === 1 ? 'key' : 'keys'} by up to ${Math.abs(tuning.widest).toFixed(1)} ¢.`
            : 'Here every key sits on the piano grid, so the patch is in equal temperament.'}</p>
        </section>
      </div>
    </Dialog>

    <Dialog
      title="Patch book"
      meta={`${patches.presets.length} ${patches.presets.length === 1 ? 'patch' : 'patches'} · ${activeName}`}
      open={panel === 'book'}
      onClose={() => setPanel(null)}
    >
      <div className="synth-book-grid">
        <ScrollArea className="synth-patches" role="listbox" ariaLabel="Patches">
          {patches.presets.map(preset => <button
            key={preset.id}
            type="button"
            role="option"
            aria-selected={patches.active === preset.id}
            className={`synth-patch ${patches.active === preset.id ? 'selected' : ''}`}
            onClick={() => synth.applyPreset(preset.id)}
          >
            <span>{preset.name}</span>
            <em className="mono">{preset.builtin ? 'factory' : 'saved'}</em>
          </button>)}
          {!patches.presets.length && <p className="panel-note">Patches are read from the engine when it is running.</p>}
        </ScrollArea>
        <div className="synth-book-side">
          <form
            className="synth-save"
            onSubmit={event => {
              event.preventDefault();
              if (!name.trim() || saving) return;
              setSaving(true);
              void synth.savePreset(name.trim()).then(created => {
                setSaving(false);
                if (created) setName('');
              });
            }}
          >
            <input
              className="synth-name"
              type="text"
              value={name}
              maxLength={40}
              placeholder="Name this patch"
              aria-label="Patch name"
              onChange={event => setName(event.target.value)}
            />
            <button type="submit" className="page-button" disabled={!name.trim() || saving}><Save size={13} aria-hidden="true" />Save</button>
            {activeSaved && <button
              type="button"
              className="page-button ghost"
              title="Delete this saved patch"
              aria-label={`Delete ${activeName}`}
              onClick={() => void synth.deletePreset(patches.active ?? '')}
            ><Trash2 size={13} aria-hidden="true" /></button>}
          </form>
          <p className="panel-note">Factory patches ship with the app. A saved patch is this user's, and editing one leaves it behind rather than rewriting it.</p>
        </div>
      </div>
    </Dialog>
  </Page>;
}
