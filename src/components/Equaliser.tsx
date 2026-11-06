import { useEffect, useRef, useState } from 'react';
import { Knob } from './instrument/Knob';
import { ScrollArea } from './instrument/ScrollArea';
import { Segmented } from './instrument/Segmented';
import { clamp } from './instrument/scales';
import {
  eqKinds, eqKindUsesGain, eqLimits, formatDb, formatHz, isCorrection, isFit, presetNameLimit,
  type EqFilter, type EqKind, type EqPresetList, type EqResponse, type EqSettings,
} from '../model';

// The plot is drawn in its own pixels, not in a viewBox that gets scaled: text
// keeps its size and a drag lands where the pointer is, whatever the panel is.
const PAD_L = 42;
const PAD_R = 14;
const PAD_T = 10;
const PAD_B = 24;
const MIN_HZ = 20;
const MAX_HZ = 20_000;
// ±12 dB with 6 dB lines: the range a correction actually moves in. A ±24 dB
// window drew a 2 dB lift as a flat line, so a real patch read as no change at
// all. A band past the edge is pinned there and still reports its value in the
// band table.
const DB_SPAN = 12;
const LOW = Math.log10(MIN_HZ);
const HIGH = Math.log10(MAX_HZ);

const DB_TICKS = [12, 6, 0, -6, -12];
const HZ_TICKS = [20, 50, 100, 200, 500, 1_000, 2_000, 5_000, 10_000, 20_000];
/** Only every other decade carries a label, so the axis never prints over itself. */
const HZ_LABELS = new Set([20, 100, 1_000, 10_000, 20_000]);

const pad2 = (value: number) => String(value).padStart(2, '0');

interface Props {
  eq: EqSettings;
  curve: EqResponse;
  /** False in a browser tab, where the engine that owns the curve is absent. */
  native: boolean;
  onChange: (next: EqSettings) => void;
  /** The shipped patches and the saved copies, as the engine lists them. */
  presets: EqPresetList;
  /** A refused patch name, shown in place of the patch's own note. */
  patchError: string;
  onApplyPreset: (id: string) => void;
  onRenamePreset: (id: string, name: string) => Promise<boolean>;
  onDeletePreset: (id: string) => void;
}

/**
 * The equaliser as an instrument: a log-frequency plot with the band handles on
 * it, a numbered band list beside it, and one editor for the band in hand. The
 * curve is the Rust cascade's own response, so what the plot draws is what the
 * audio path runs.
 */
export function Equaliser({
  eq, curve, native, onChange, presets, patchError,
  onApplyPreset, onRenamePreset, onDeletePreset,
}: Props) {
  const [selected, setSelected] = useState(0);
  const drag = useRef<number | null>(null);
  const plot = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 640, h: 240 });
  // The patch being renamed, and the saved copy waiting for a second click.
  const [renaming, setRenaming] = useState<string | null>(null);
  const [draft, setDraft] = useState('');
  const [confirming, setConfirming] = useState<string | null>(null);
  const index = Math.min(selected, Math.max(0, eq.filters.length - 1));
  const active = eq.filters[index] as EqFilter | undefined;
  const bands = eq.filters.filter(filter => filter.enabled).length;
  // Three kinds of shipped patch, and the picker keeps them apart: AutoEq's
  // corrections, the ones this app fitted from a published measurement because
  // AutoEq has none, and the shapes written here with no measurement at all.
  const corrections = presets.presets.filter(preset => preset.builtin && isCorrection(preset) && !isFit(preset));
  const fits = presets.presets.filter(preset => preset.builtin && isFit(preset));
  const shapes = presets.presets.filter(preset => preset.builtin && !isCorrection(preset));
  const saved = presets.presets.filter(preset => !preset.builtin);
  const chosen = presets.presets.find(preset => preset.id === presets.active);
  const note = patchError || chosen?.note
    || 'These bands belong to no patch. Pick a correction to load published measurements, or a shape to start from; editing either saves a copy and leaves the original alone.';

  // A patch change closes whatever was open, so a name is never saved onto the
  // patch that was merely shown before it.
  useEffect(() => { setRenaming(null); setConfirming(null); }, [presets.active]);

  useEffect(() => {
    const element = plot.current;
    if (!element) return;
    const sync = () => {
      const rect = element.getBoundingClientRect();
      if (rect.width > 80 && rect.height > 80) setSize({ w: Math.round(rect.width), h: Math.round(rect.height) });
    };
    sync();
    const observer = new ResizeObserver(sync);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const W = size.w;
  const H = size.h;
  // A narrow plot keeps only the labels that cannot collide: decades, not halves.
  const labels: Set<number> = W < 560 ? new Set([20, 100, 1_000, 20_000]) : HZ_LABELS;
  const spanDb = 2 * DB_SPAN;
  const xFor = (hz: number) => PAD_L + ((Math.log10(clamp(hz, MIN_HZ, MAX_HZ)) - LOW) / (HIGH - LOW)) * (W - PAD_L - PAD_R);
  const yFor = (db: number) => PAD_T + ((DB_SPAN - clamp(db, -DB_SPAN, DB_SPAN)) / spanDb) * (H - PAD_T - PAD_B);
  const hzFor = (x: number) => 10 ** (LOW + ((x - PAD_L) / (W - PAD_L - PAD_R)) * (HIGH - LOW));
  const dbFor = (y: number) => DB_SPAN - ((y - PAD_T) / (H - PAD_T - PAD_B)) * spanDb;

  const patch = (at: number, changes: Partial<EqFilter>) => {
    if (at < 0 || at >= eq.filters.length) return;
    onChange({ ...eq, filters: eq.filters.map((filter, where) => (where === at ? { ...filter, ...changes } : filter)) });
  };

  const setFrequency = (value: number) => patch(index, { frequency: Math.round(clamp(value, MIN_HZ, MAX_HZ)) });
  const setGain = (value: number) => patch(index, { gainDb: clamp(Number(value.toFixed(1)), -eqLimits.gain.max, eqLimits.gain.max) });
  const setQ = (value: number) => patch(index, { q: clamp(Number(value.toFixed(2)), eqLimits.q.min, eqLimits.q.max) });
  const setPreamp = (value: number) => onChange({ ...eq, preampDb: clamp(Number(value.toFixed(1)), eqLimits.preamp.min, 0) });

  // The curve arrives from the engine; until it does, the plot says so with a
  // flat line rather than a shape that was guessed here.
  const drawn = curve.frequencies.length === curve.gains.length && curve.frequencies.length > 1;
  const line = drawn
    ? curve.frequencies.map((hz, at) => `${at ? 'L' : 'M'}${xFor(hz).toFixed(1)} ${yFor(curve.gains[at]).toFixed(1)}`).join(' ')
    : `M${xFor(MIN_HZ).toFixed(1)} ${yFor(0)} L${xFor(MAX_HZ).toFixed(1)} ${yFor(0)}`;
  const area = `${line} L${xFor(MAX_HZ).toFixed(1)} ${yFor(0)} L${xFor(MIN_HZ).toFixed(1)} ${yFor(0)} Z`;

  const local = (svg: SVGSVGElement, event: React.PointerEvent<SVGElement>) => {
    const point = svg.createSVGPoint();
    point.x = event.clientX;
    point.y = event.clientY;
    const matrix = svg.getScreenCTM();
    if (!matrix) return { x: 0, y: 0 };
    const mapped = point.matrixTransform(matrix.inverse());
    return { x: mapped.x, y: mapped.y };
  };

  const onNodeDown = (event: React.PointerEvent<SVGElement>, at: number) => {
    event.preventDefault();
    const svg = event.currentTarget.ownerSVGElement;
    if (!svg) return;
    svg.setPointerCapture(event.pointerId);
    drag.current = at;
    setSelected(at);
  };

  const onGraphMove = (event: React.PointerEvent<SVGSVGElement>) => {
    const at = drag.current;
    if (at === null) return;
    const { x, y } = local(event.currentTarget, event);
    const filter = eq.filters[at];
    if (!filter) return;
    const changes: Partial<EqFilter> = { frequency: Math.round(clamp(hzFor(x), MIN_HZ, MAX_HZ)) };
    if (eqKindUsesGain(filter.kind)) changes.gainDb = clamp(Number(dbFor(y).toFixed(1)), -eqLimits.gain.max, eqLimits.gain.max);
    onChange({ ...eq, filters: eq.filters.map((item, where) => (where === at ? { ...item, ...changes } : item)) });
  };

  const onGraphUp = (event: React.PointerEvent<SVGSVGElement>) => {
    if (drag.current === null) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
    drag.current = null;
  };

  const nudge = (event: React.KeyboardEvent<SVGGElement>, at: number) => {
    const filter = eq.filters[at];
    if (!filter) return;
    const fine = event.shiftKey;
    const gainOnly = !eqKindUsesGain(filter.kind);
    const keys: Record<string, () => void> = {
      ArrowLeft: () => patch(at, { frequency: Math.round(clamp(filter.frequency / (fine ? 1.005 : 1.06), MIN_HZ, MAX_HZ)) }),
      ArrowRight: () => patch(at, { frequency: Math.round(clamp(filter.frequency * (fine ? 1.005 : 1.06), MIN_HZ, MAX_HZ)) }),
      ArrowUp: () => patch(at, { gainDb: clamp(Number((filter.gainDb + (fine ? 0.1 : 0.5)).toFixed(1)), -eqLimits.gain.max, eqLimits.gain.max) }),
      ArrowDown: () => patch(at, { gainDb: clamp(Number((filter.gainDb - (fine ? 0.1 : 0.5)).toFixed(1)), -eqLimits.gain.max, eqLimits.gain.max) }),
    };
    const action = keys[event.key];
    if (!action) return;
    if (gainOnly && (event.key === 'ArrowUp' || event.key === 'ArrowDown')) return;
    event.preventDefault();
    action();
  };

  // Where every handle sits, so a label can see whether another one shares its point.
  const placed = eq.filters.map((filter, at) => ({
    at,
    x: xFor(filter.frequency),
    y: yFor(eqKindUsesGain(filter.kind) ? filter.gainDb : 0),
  }));

  const submitRename = (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const name = draft.trim();
    if (!renaming || !name) return;
    // The engine decides: a refused name keeps the field open with the reason.
    void onRenamePreset(renaming, name).then(stored => {
      if (stored) { setRenaming(null); setDraft(''); }
    });
  };

  // The same 0.05 dB threshold the engine uses to call a curve a boost: a high pass
  // is flat to within a rounding error, and that is not something to report.
  const headline = !native ? 'The engine runs in the Windows app'
    : !eq.enabled ? 'Bypassed: the path is untouched'
      : curve.headroomDb > 0.05 ? `Boosts reach ${formatDb(curve.headroomDb)} past full scale`
        : curve.maxBoostDb > 0.05 ? `${formatDb(curve.headroomDb)} of headroom at the peak`
          : 'No boost above the line';

  return <article className={`page-card wide-card eq-card ${eq.enabled ? 'engaged' : 'bypassed'}`}>
    <div className="panel-heading">
      <h2 id="eq-title" title="Equaliser">Equaliser</h2>
      <span className="panel-meta" title={`${bands} of ${eqLimits.bands} bands · preamp ${formatDb(eq.preampDb)} · peak ${formatDb(curve.maxBoostDb)}`}>
        {bands} of {eqLimits.bands} bands · preamp {formatDb(eq.preampDb)}
      </span>
      <button
        type="button"
        className={`eq-power ${eq.enabled ? 'on' : ''}`}
        aria-pressed={eq.enabled}
        aria-label="Equaliser in path"
        onClick={() => onChange({ ...eq, enabled: !eq.enabled })}
      >
        <i aria-hidden="true" /><span className="eq-power-label">{eq.enabled ? 'IN PATH' : 'BYPASSED'}</span>
      </button>
    </div>

    <div className="eq-body">
      {native && <div className="eq-patch">
        <span className="eq-group-label mono">PATCH</span>
        {renaming
          ? <form className="eq-rename" onSubmit={submitRename}>
            <input
              className="eq-rename-input mono"
              aria-label="Patch name"
              value={draft}
              maxLength={presetNameLimit}
              autoFocus
              spellCheck={false}
              onChange={event => setDraft(event.target.value)}
              onKeyDown={event => {
                if (event.key !== 'Escape') return;
                // Escape closes the name field, not the whole panel behind it.
                event.stopPropagation();
                setRenaming(null);
              }}
            />
            <button type="submit" className="page-button" disabled={!draft.trim()}>Save</button>
            <button type="button" className="page-button" onClick={() => setRenaming(null)}>Cancel</button>
          </form>
          : <>
            <select
              className="eq-patch-select mono"
              aria-label="Equaliser patch"
              value={presets.active ?? ''}
              title={chosen ? `${chosen.name} · ${chosen.note}` : 'Unsaved bands'}
              onChange={event => { if (event.target.value) onApplyPreset(event.target.value); }}
            >
              {!chosen && <option value="" disabled>Unsaved bands</option>}
              {corrections.length > 0 && <optgroup label={`Corrections · ${corrections.length} measured by oratory1990, computed by AutoEq`}>
                {corrections.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}
              </optgroup>}
              {fits.length > 0 && <optgroup label={`Fits · ${fits.length} fitted here to a published measurement`}>
                {fits.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}
              </optgroup>}
              {shapes.length > 0 && <optgroup label={`Shapes · ${shapes.length} starting points written here, no measurements`}>
                {shapes.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}
              </optgroup>}
              {saved.length > 0 && <optgroup label={`Saved on this machine · ${saved.length}`}>
                {saved.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}
              </optgroup>}
            </select>
            {chosen && !chosen.builtin && <>
              <button
                type="button"
                className="eq-patch-action"
                aria-label="Rename patch"
                title="Rename this saved patch"
                onClick={() => { setDraft(chosen.name); setRenaming(chosen.id); }}
              >Rename</button>
              <button
                type="button"
                className={`eq-patch-action danger ${confirming === chosen.id ? 'armed' : ''}`}
                aria-label={confirming === chosen.id ? 'Confirm delete patch' : 'Delete patch'}
                title={confirming === chosen.id
                  ? 'Click again to delete this saved patch. The bands stay as they are.'
                  : 'Delete this saved patch. The bands stay as they are.'}
                onBlur={() => setConfirming(null)}
                onClick={() => {
                  if (confirming === chosen.id) { onDeletePreset(chosen.id); setConfirming(null); }
                  else setConfirming(chosen.id);
                }}
              >{confirming === chosen.id ? 'Delete?' : 'Delete'}</button>
            </>}
            <span className={`eq-patch-note ${patchError ? 'warn' : ''}`} title={note}>{note}</span>
          </>}
        {/* Every patch states where its numbers came from. The row is always there,
            one line high, so choosing a correction cannot resize the instrument. */}
        <p className="eq-source" title={chosen?.source ? `${chosen.source.label} — ${chosen.source.url}` : undefined}>
          <span className="eq-source-tag mono">SOURCE</span>
          {chosen?.source
            ? <>
              <span className="eq-source-label">{chosen.source.label}</span>
              <span className="eq-source-terms">{chosen.source.terms}</span>
            </>
            : <span className="eq-source-terms">{chosen
              ? 'Written here: a starting point, no measurement behind these bands.'
              : 'No patch loaded: these bands were set by hand.'}</span>}
        </p>
      </div>}
      <div className="eq-plot">
        <div className="eq-canvas" ref={plot}>
        <svg
          width={W}
          height={H}
          viewBox={`0 0 ${W} ${H}`}
          role="group"
          aria-label={`Equaliser response: ${bands} ${bands === 1 ? 'band' : 'bands'}, largest boost ${formatDb(curve.maxBoostDb)}`}
          onPointerMove={onGraphMove}
          onPointerUp={onGraphUp}
          onPointerCancel={onGraphUp}
        >
          <defs>
            <linearGradient id="eq-fill" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0" stopColor="var(--accent)" stopOpacity="0.24" />
              <stop offset="0.5" stopColor="var(--accent)" stopOpacity="0.05" />
              <stop offset="1" stopColor="var(--accent)" stopOpacity="0.24" />
            </linearGradient>
          </defs>
          {HZ_TICKS.map(hz => <line
            key={hz}
            className={`eq-grid ${labels.has(hz) ? '' : 'faint'}`}
            x1={xFor(hz).toFixed(1)} x2={xFor(hz).toFixed(1)}
            y1={PAD_T} y2={H - PAD_B}
          />)}
          {DB_TICKS.map(db => <g key={db}>
            <line
              className={`eq-grid ${db === 0 ? 'zero' : ''}`}
              x1={PAD_L} x2={W - PAD_R}
              y1={yFor(db).toFixed(1)} y2={yFor(db).toFixed(1)}
            />
            <text className="eq-axis" x={PAD_L - 8} y={yFor(db) + 4} textAnchor="end">{db > 0 ? `+${db}` : db}</text>
          </g>)}
          {HZ_TICKS.filter(hz => labels.has(hz)).map(hz => <text
            key={hz}
            className="eq-axis"
            x={xFor(hz).toFixed(1)}
            y={H - 7}
            textAnchor={hz === MIN_HZ ? 'start' : hz === MAX_HZ ? 'end' : 'middle'}
          >{hz >= 1000 ? `${hz / 1000}k` : hz}</text>)}

          <path className="eq-area" d={area} fill="url(#eq-fill)" />
          <path className="eq-line" d={line} />

          {eq.filters.map((filter, at) => {
            const usesGain = eqKindUsesGain(filter.kind);
            const x = xFor(filter.frequency);
            const y = yFor(usesGain ? filter.gainDb : 0);
            // Two bands can share a point - a hand-built pair of high-pass
            // sections at one corner does -
            // so their numbers step aside instead of printing over each other. The
            // dots stay where the values are; only the labels move.
            const crowd = placed.filter(other => Math.abs(other.x - x) < 6 && Math.abs(other.y - y) < 6);
            const slot = crowd.findIndex(other => other.at === at);
            const labelShift = crowd.length > 1 ? (slot - (crowd.length - 1) / 2) * 18 : 0;
            return <g
              key={at}
              className={`eq-node ${at === index ? 'selected' : ''} ${filter.enabled ? '' : 'off'}`}
              role="button"
              tabIndex={0}
              aria-label={`Band ${at + 1}: ${eqKinds.find(kind => kind.id === filter.kind)?.label ?? filter.kind}, ${formatHz(filter.frequency)}${usesGain ? `, ${formatDb(filter.gainDb)}` : ''}, Q ${filter.q.toFixed(2)}`}
              aria-description="Drag to move this band in frequency and gain. Arrows nudge it; shift is fine."
              onPointerDown={event => onNodeDown(event, at)}
              onFocus={() => setSelected(at)}
              onKeyDown={event => nudge(event, at)}
            >
              <circle className="eq-node-hit" cx={x} cy={y} r={13} />
              <circle className="eq-node-dot" cx={x} cy={y} r={4.5} />
              <text className="eq-node-label" x={x + labelShift} y={y - 9} textAnchor="middle">{pad2(at + 1)}</text>
            </g>;
          })}
        </svg>
        </div>
        <p className={`eq-headline ${curve.headroomDb > 0.05 && eq.enabled ? 'warn' : ''}`} title={headline}>{headline}</p>
      </div>

      {eq.filters.length === 0
        ? <div className="eq-bands empty"><p>No bands yet.</p><span>Add one to shape the response, or bring the whole stage back to neutral.</span></div>
        : <ScrollArea className="eq-bands" role="list" ariaLabel="Equaliser bands">
          {eq.filters.map((filter, at) => {
            const usesGain = eqKindUsesGain(filter.kind);
            const kindLabel = eqKinds.find(kind => kind.id === filter.kind)?.label ?? filter.kind;
            const reading = `${formatHz(filter.frequency)} · ${usesGain ? formatDb(filter.gainDb) : 'no gain'} · Q ${filter.q.toFixed(2)}`;
            return <div key={at} className={`eq-row ${at === index ? 'selected' : ''} ${filter.enabled ? '' : 'off'}`} role="listitem">
              <button
                type="button"
                className="eq-row-main"
                aria-pressed={at === index}
                aria-label={`Band ${at + 1}, ${kindLabel}, ${reading}`}
                onClick={() => setSelected(at)}
              >
                <span className="eq-row-index mono">{pad2(at + 1)}</span>
                <span className="eq-row-kind" title={kindLabel}>{kindLabel}</span>
                <span className="eq-row-reading mono" title={reading}>{reading}</span>
              </button>
              <button
                type="button"
                className={`eq-row-toggle ${filter.enabled ? 'on' : ''}`}
                aria-pressed={filter.enabled}
                aria-label={`Band ${at + 1} in path`}
                title={filter.enabled ? 'Take this band out of the path' : 'Put this band back in the path'}
                onClick={() => patch(at, { enabled: !filter.enabled })}
              ><i aria-hidden="true" /></button>
              <button
                type="button"
                className="eq-row-remove"
                aria-label={`Remove band ${at + 1}`}
                title={`Remove band ${at + 1}`}
                onClick={() => {
                  onChange({ ...eq, filters: eq.filters.filter((_, where) => where !== at) });
                  setSelected(Math.max(0, at - 1));
                }}
              >×</button>
            </div>;
          })}
        </ScrollArea>}

      <div className="eq-edit">
        {active
          ? <div className="eq-group">
            <span className="eq-group-label mono">BAND {pad2(index + 1)}</span>
            <Segmented
              compact
              label="Band type"
              options={eqKinds.map(kind => ({ id: kind.id, label: kind.label }))}
              value={active.kind}
              onChange={id => patch(index, { kind: id as EqKind })}
            />
            <Knob
              label="Frequency"
              value={Math.log10(active.frequency)}
              min={LOW}
              max={HIGH}
              step={0.001}
              format={value => formatHz(10 ** value)}
              onChange={value => setFrequency(10 ** value)}
              ticks={[Math.log10(100), Math.log10(1_000), Math.log10(10_000)]}
              reset={Math.log10(1_000)}
              description="Centre frequency, on a logarithmic scale"
              announced={{ min: MIN_HZ, max: MAX_HZ, now: active.frequency }}
            />
            <Knob
              label="Gain"
              value={active.gainDb}
              min={-eqLimits.gain.max}
              max={eqLimits.gain.max}
              step={eqLimits.gain.step}
              bipolar
              format={value => formatDb(value)}
              onChange={setGain}
              reset={0}
              disabled={!eqKindUsesGain(active.kind)}
              description={eqKindUsesGain(active.kind) ? 'Boost or cut at the centre frequency' : 'This filter shape has no gain'}
            />
            <Knob
              label="Q"
              value={Math.log10(active.q)}
              min={Math.log10(eqLimits.q.min)}
              max={Math.log10(eqLimits.q.max)}
              step={0.001}
              format={value => (10 ** value).toFixed(2)}
              onChange={value => setQ(10 ** value)}
              reset={Math.log10(0.707)}
              description="Width of the band: higher Q is narrower"
              announced={{ min: eqLimits.q.min, max: eqLimits.q.max, now: active.q }}
            />
          </div>
          : <p className="eq-empty">Select a band to edit it.</p>}

        <div className="eq-group eq-stage-group">
          <span className="eq-group-label mono">STAGE</span>
          <Knob
            label="Preamp"
            value={eq.preampDb}
            min={eqLimits.preamp.min}
            max={0}
            step={eqLimits.preamp.step}
            format={value => formatDb(value)}
            onChange={setPreamp}
            reset={0}
            description="Attenuation ahead of the bands, to make room for a boost"
          />
          <div className="eq-actions">
            <button
              type="button"
              className="page-button"
              onClick={() => onChange({ ...eq, preampDb: -Math.min(Math.abs(eqLimits.preamp.min), Math.ceil(curve.maxBoostDb * 10) / 10) })}
              title="Set the preamp so the largest boost lands back at full scale"
            >Match headroom</button>
            <button
              type="button"
              className="page-button"
              disabled={eq.filters.length >= eqLimits.bands}
              onClick={() => {
                onChange({ ...eq, enabled: true, filters: [...eq.filters, { kind: 'peaking', frequency: 1_000, gainDb: 0, q: 1.4, enabled: true }] });
                setSelected(eq.filters.length);
              }}
            >Add band</button>
            <button type="button" className="page-button" onClick={() => { onChange({ ...eq, enabled: false, preampDb: 0, filters: [] }); setSelected(0); }}>Neutral</button>
          </div>
        </div>
      </div>
    </div>
  </article>;
}
