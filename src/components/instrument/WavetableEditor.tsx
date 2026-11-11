import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Redo2, Undo2 } from 'lucide-react';

const SIZE = 2048;
const HARMONICS = 128;
type Harmonic = { sine: number; cosine: number };

export function spectrum(samples: number[]): Harmonic[] {
  if (!samples.length) return [];
  return Array.from({ length: HARMONICS }, (_, index) => {
    const step = 2 * Math.PI * (index + 1) / samples.length;
    const sinStep = Math.sin(step);
    const cosStep = Math.cos(step);
    let sinPhase = 0;
    let cosPhase = 1;
    let sine = 0;
    let cosine = 0;
    for (const sample of samples) {
      sine += sample * sinPhase;
      cosine += sample * cosPhase;
      const nextSin = sinPhase * cosStep + cosPhase * sinStep;
      cosPhase = cosPhase * cosStep - sinPhase * sinStep;
      sinPhase = nextSin;
    }
    return { sine: 2 * sine / samples.length, cosine: 2 * cosine / samples.length };
  });
}

export function waveform(bins: Harmonic[]): number[] {
  const samples = new Array<number>(SIZE).fill(0);
  bins.forEach((bin, index) => {
    const step = 2 * Math.PI * (index + 1) / SIZE;
    const sinStep = Math.sin(step);
    const cosStep = Math.cos(step);
    let sine = 0;
    let cosine = 1;
    for (let i = 0; i < SIZE; i++) {
      samples[i] += bin.sine * sine + bin.cosine * cosine;
      const nextSin = sine * cosStep + cosine * sinStep;
      cosine = cosine * cosStep - sine * sinStep;
      sine = nextSin;
    }
  });
  const peak = Math.max(1, ...samples.map(Math.abs));
  return samples.map(value => value / peak);
}

export function warpSpectrum(bins: Harmonic[], tilt: number, stretch: number): Harmonic[] {
  return Array.from({ length: HARMONICS }, (_, i) => {
    const source = (i + 1) / stretch - 1;
    const left = Math.floor(source);
    const frac = source - left;
    const a = bins[left] ?? { sine: 0, cosine: 0 };
    const b = bins[left + 1] ?? { sine: 0, cosine: 0 };
    const gain = Math.pow(i + 1, tilt);
    return { sine: (a.sine * (1 - frac) + b.sine * frac) * gain, cosine: (a.cosine * (1 - frac) + b.cosine * frac) * gain };
  });
}

/**
 * What the editor is holding: the table's frames, which frame is open, and the
 * states either side of it. Every edit keeps the state it replaced, so a drawn
 * stroke, a warped spectrum or a deleted frame can be stepped back and forward.
 */
type Doc = {
  frames: number[][];
  past: number[][][];
  future: number[][][];
  selected: number;
};

/** Every history keeps the last fifty edits: enough to undo a session's work. */
const DEPTH = 50;

/** An edit, with the state it replaced pushed onto the undo stack. */
function edited(doc: Doc, next: Partial<Doc>): Doc {
  return {
    ...doc,
    ...next,
    past: [...doc.past.slice(1 - DEPTH), doc.frames],
    future: [],
  };
}

/** One step through the history, back or forward. */
function stepped(doc: Doc, back: boolean): Doc {
  const from = back ? doc.past : doc.future;
  if (!from.length) return doc;
  const frames = from[from.length - 1];
  return {
    frames,
    past: back ? doc.past.slice(0, -1) : [...doc.past.slice(1 - DEPTH), doc.frames],
    future: back ? [...doc.future, doc.frames] : doc.future.slice(0, -1),
    selected: Math.min(doc.selected, Math.max(0, frames.length - 1)),
  };
}

const empty: Doc = { frames: [], past: [], future: [], selected: 0 };

export function WavetableEditor({ table, onSaved }: { table: number; onSaved: () => void }) {
  const [doc, setDoc] = useState<Doc>(empty);
  const [harmonic, setHarmonic] = useState(1);
  const [sampleIndex, setSampleIndex] = useState(0);
  const [tilt, setTilt] = useState(0);
  const [stretch, setStretch] = useState(1);
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);
  const dragging = useRef<{ pointerId: number; index: number; value: number } | null>(null);
  const { frames, selected } = doc;
  const frame = frames[selected] ?? [];
  const bins = useMemo(() => frame.length ? spectrum(frame) : [], [frame]);
  const bin = bins[harmonic - 1] ?? { sine: 0, cosine: 0 };
  const magnitude = Math.hypot(bin.sine, bin.cosine);
  const phase = Math.atan2(bin.cosine, bin.sine) * 180 / Math.PI;

  useEffect(() => {
    let active = true;
    setDoc(empty);
    setError('');
    invoke<number[][]>('synth_editable_table', { table }).then(data => {
      if (active) setDoc({ frames: data, past: [], future: [], selected: 0 });
    }).catch(reason => { if (active) setError(String(reason)); });
    return () => { active = false; };
  }, [table]);

  const replace = (samples: number[]) => setDoc(current => edited(current, {
    frames: current.frames.map((item, i) => i === current.selected ? samples : item),
  }));
  const changeBin = (amplitude: number, degrees: number) => {
    amplitude = Math.max(0, Math.min(2, amplitude));
    degrees = Math.max(-180, Math.min(180, degrees));
    const angle = degrees * Math.PI / 180;
    const next = bins.map((item, i) => i === harmonic - 1
      ? { sine: amplitude * Math.cos(angle), cosine: amplitude * Math.sin(angle) } : item);
    replace(waveform(next));
  };
  const applyWarp = () => {
    if (tilt !== 0 || stretch !== 1) replace(waveform(warpSpectrum(bins, tilt, stretch)));
  };
  /**
   * A stroke is one edit, not one per pointer move: the state before the pen went
   * down is written to the history as the stroke starts, and the moves that follow
   * only paint. Otherwise a single drag would bury every earlier edit.
   */
  const draw = (event: React.PointerEvent<SVGSVGElement>, record: boolean) => {
    const previous = dragging.current;
    if (saving || !previous || previous.pointerId !== event.pointerId || !frame.length) return;
    const rect = event.currentTarget.getBoundingClientRect();
    const index = Math.min(SIZE - 1, Math.max(0, Math.round((event.clientX - rect.left) / rect.width * (SIZE - 1))));
    const value = Math.min(1, Math.max(-1, 1 - 2 * (event.clientY - rect.top) / rect.height));
    setDoc(current => {
      const brushed = current.frames.map((item, i) => {
        if (i !== current.selected) return item;
        const next = item.slice();
        const distance = Math.abs(index - previous.index);
        const direction = Math.sign(index - previous.index);
        for (let step = 0; step <= distance; step++) {
          const at = previous.index + direction * step;
          const sample = previous.value + (value - previous.value) * (distance ? step / distance : 1);
          for (let offset = -4; offset <= 4; offset++) next[(at + offset + SIZE) % SIZE] = sample;
        }
        return next;
      });
      return record ? edited(current, { frames: brushed }) : { ...current, frames: brushed };
    });
    dragging.current = { pointerId: event.pointerId, index, value };
  };
  const save = async () => {
    setSaving(true);
    setError('');
    try {
      await invoke('synth_save_table', { table, frames });
      const saved = await invoke<number[][]>('synth_editable_table', { table });
      setDoc(current => ({ ...current, frames: saved, selected: Math.min(current.selected, Math.max(0, saved.length - 1)) }));
      onSaved();
    } catch (reason) { setError(String(reason)); }
    finally { setSaving(false); }
  };
  const undo = useCallback(() => setDoc(current => stepped(current, true)), []);
  const redo = useCallback(() => setDoc(current => stepped(current, false)), []);

  // Ctrl+Z and Ctrl+Shift+Z step the history from the keyboard, which is where the
  // hand already is after a stroke. A text field keeps its own undo.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey) || event.altKey) return;
      const key = event.key.toLowerCase();
      if (key !== 'z' && key !== 'y') return;
      const target = event.target;
      if (target instanceof HTMLElement
        && (target.tagName === 'TEXTAREA' || (target instanceof HTMLInputElement && target.type !== 'number'))) return;
      event.preventDefault();
      if (key === 'y' || event.shiftKey) redo();
      else undo();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [undo, redo]);

  const points = frame.filter((_, i) => i % 4 === 0)
    .map((sample, i) => `${(i / 511 * 100).toFixed(2)},${(50 - sample * 48).toFixed(2)}`);
  const line = `M${points.join('L')}`;
  const area = `M0,50L${points.join('L')}L100,50Z`;
  // The tallest partial is the scale of the strip, so a quiet table still shows its
  // spectrum; the square root keeps the high partials visible beside the first.
  const loudest = Math.max(0.0001, ...bins.map(item => Math.hypot(item.sine, item.cosine)));
  const pickHarmonic = (event: React.PointerEvent<HTMLDivElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    const index = Math.floor((event.clientX - rect.left) / rect.width * HARMONICS) + 1;
    setHarmonic(Math.min(HARMONICS, Math.max(1, index)));
  };

  return <>
    {error && <p className="audio-error" role="alert">{error}</p>}
    {!frame.length ? !error && <p className="panel-note" role="status">Loading table…</p> : <fieldset className="synth-editor" disabled={saving}>
      <div className="synth-toolbar">
        <label className="synth-entry narrow">
          <span>Frame</span>
          <select aria-label="Wavetable frame" value={selected} onChange={event => setDoc(current => ({ ...current, selected: Number(event.target.value) }))}>
            {frames.map((_, index) => <option key={index} value={index}>Frame {index + 1}</option>)}
          </select>
        </label>
        <span className="synth-figures mono">frame {selected + 1} of {frames.length}</span>
        <span className="synth-toolbar-gap" />
        <button type="button" className="icon-button" aria-label="Undo the last edit" title="Undo (Ctrl+Z)" disabled={!doc.past.length} onClick={undo}><Undo2 size={14} aria-hidden="true" /></button>
        <button type="button" className="icon-button" aria-label="Redo the last edit" title="Redo (Ctrl+Shift+Z)" disabled={!doc.future.length} onClick={redo}><Redo2 size={14} aria-hidden="true" /></button>
        <button type="button" className="page-button ghost" disabled={frames.length >= 8} onClick={() => setDoc(current => edited(current, {
          frames: [...current.frames, (current.frames[current.selected] ?? []).slice()],
          selected: current.frames.length,
        }))}>Duplicate frame</button>
        <button type="button" className="page-button ghost" disabled={frames.length <= 1} onClick={() => setDoc(current => edited(current, {
          frames: current.frames.filter((_, i) => i !== current.selected),
          selected: Math.max(0, current.selected - 1),
        }))}>Delete frame</button>
        <button type="button" className="page-button" disabled={saving} onClick={() => void save()}>{saving ? 'Saving…' : 'Save table'}</button>
      </div>
      <svg className="synth-envelope-graph synth-editor-graph" viewBox="0 0 100 100" preserveAspectRatio="none" role="img" aria-label="Editable waveform; drag to draw" style={{ touchAction: 'none' }}
        onPointerDown={event => {
          if (saving || event.button !== 0) return;
          const rect = event.currentTarget.getBoundingClientRect();
          const index = Math.min(SIZE - 1, Math.max(0, Math.round((event.clientX - rect.left) / rect.width * (SIZE - 1))));
          const value = Math.min(1, Math.max(-1, 1 - 2 * (event.clientY - rect.top) / rect.height));
          dragging.current = { pointerId: event.pointerId, index, value };
          event.currentTarget.setPointerCapture(event.pointerId);
          draw(event, true);
        }}
        onPointerMove={event => draw(event, false)} onPointerUp={() => { dragging.current = null; }} onPointerCancel={() => { dragging.current = null; }}>
        {[25, 50, 75].map(row => <line key={`row-${row}`} className="synth-graph-grid" x1={0} x2={100} y1={row} y2={row} />)}
        {[25, 50, 75].map(column => <line key={`column-${column}`} className="synth-graph-grid" x1={column} x2={column} y1={0} y2={100} />)}
        <path className="synth-graph-area" d={area} />
        <path className="synth-graph-line" d={line} />
      </svg>
      <div className="synth-spectrum">
        <div className="synth-spectrum-head">
          <span className="synth-figures mono">partials 1–128</span>
          <span className="synth-figures mono">partial {harmonic} · {magnitude.toFixed(2)}</span>
        </div>
        <div className="synth-bars" aria-hidden="true" onPointerDown={pickHarmonic}>
          {bins.map((item, index) => <i
            key={index}
            className={index === harmonic - 1 ? 'on' : ''}
            style={{ height: `${Math.round(Math.max(2, Math.sqrt(Math.hypot(item.sine, item.cosine) / loudest) * 100))}%` }}
          />)}
        </div>
      </div>
      <p className="panel-note">Draw on the waveform, or shape a sample, a harmonic or the whole spectrum below. The strip is the spectrum the drawing converts to: click a partial to edit it. Saving keeps the first 128 harmonics and normalises each frame.</p>
      <div className="synth-edit-grid">
        <div className="synth-group">
          <h4>Sample</h4>
          <label className="synth-entry row"><span>Index</span><input aria-label="Waveform sample number" type="number" min={0} max={SIZE - 1} value={sampleIndex} onChange={event => setSampleIndex(Math.min(SIZE - 1, Math.max(0, Math.floor(Number(event.target.value) || 0))))} /></label>
          <label className="synth-entry row"><span>Value</span><input aria-label="Waveform sample value" type="number" min={-1} max={1} step={0.01} value={Number((frame[sampleIndex] ?? 0).toFixed(3))} onChange={event => {
            const value = Math.min(1, Math.max(-1, Number(event.target.value)));
            const next = frame.slice();
            next[sampleIndex] = value;
            replace(next);
          }} /></label>
        </div>
        <div className="synth-group">
          <h4>Harmonic</h4>
          <label className="synth-entry row"><span>Number</span><input aria-label="Harmonic number" type="number" min={1} max={HARMONICS} value={harmonic} onChange={event => setHarmonic(Math.min(HARMONICS, Math.max(1, Math.floor(Number(event.target.value) || 1))))} /></label>
          <label className="synth-entry row"><span>Magnitude</span><input aria-label="Harmonic magnitude" type="number" min={0} max={2} step={0.01} value={Number(magnitude.toFixed(3))} onChange={event => changeBin(Number(event.target.value), phase)} /></label>
          <label className="synth-entry row"><span>Phase °</span><input aria-label="Harmonic phase" type="number" min={-180} max={180} step={1} value={Math.round(phase)} onChange={event => changeBin(magnitude, Number(event.target.value))} /></label>
        </div>
        <div className="synth-group">
          <h4>Spectral warp</h4>
          <label className="synth-entry row"><span>Tilt</span><input aria-label="Spectral tilt" type="number" min={-1} max={1} step={0.1} value={tilt} onChange={event => setTilt(Math.min(1, Math.max(-1, Number(event.target.value))))} /></label>
          <label className="synth-entry row"><span>Stretch</span><input aria-label="Harmonic stretch" type="number" min={0.5} max={2} step={0.1} value={stretch} onChange={event => setStretch(Math.min(2, Math.max(0.5, Number(event.target.value))))} /></label>
          <button type="button" className="page-button ghost" onClick={applyWarp}>Apply spectral warp</button>
        </div>
      </div>
    </fieldset>}
  </>;
}
