import type { VoiceTrace } from './voice';

interface Props {
  /** The engine's own trace, oldest column first, as low/high pairs. */
  scope: number[];
  /** What one note of the patch looks like while the engine is stopped. */
  preview: VoiceTrace;
  running: boolean;
}

/** Frames to the middle of the plot: a trace is drawn to the same height at every width. */
const amplitude = (value: number) => 50 - Math.max(-1, Math.min(1, value)) * 46;
/** Columns across the plot, so both sources are drawn on the same axis. */
const columns = 100;

/**
 * One closed shape from a column's highest frame across and its lowest back: a
 * peak-held column drawn as a stroke would be a comb of separate bars, because
 * each one is only a pixel of a wider plot. Filled, the columns join into the
 * body of the waveform.
 */
function band(lows: number[], highs: number[]): string {
  const count = Math.max(lows.length, highs.length);
  if (!count) return '';
  const x = (index: number) => ((index / Math.max(1, count - 1)) * columns).toFixed(2);
  const top = highs.map((value, index) => `${index === 0 ? 'M' : 'L'}${x(index)},${amplitude(value).toFixed(2)}`).join('');
  const bottom = [...lows].reverse()
    .map((value, index, all) => `L${x(all.length - 1 - index)},${amplitude(value).toFixed(2)}`).join('');
  return `${top}${bottom}Z`;
}

const line = (values: number[]) => values
  .map((value, index) => `${index === 0 ? 'M' : 'L'}${((index / Math.max(1, values.length - 1)) * columns).toFixed(2)},${amplitude(value).toFixed(2)}`)
  .join('');

/**
 * The instrument's display. While the engine runs it draws the frames that were
 * written to the device; while it is stopped it draws the patch's own voice, one
 * A4 note built from the same oscillators, filter and level the engine uses. A
 * column keeps the lowest and highest frame it covered, so a waveform narrower
 * than a pixel still shows its full height instead of aliasing into a flat line.
 */
export function SynthScope({ scope, preview, running }: Props) {
  const live = running && scope.length >= 4;
  const lows: number[] = [];
  const highs: number[] = [];
  if (live) {
    for (let index = 0; index + 1 < scope.length; index += 2) {
      lows.push(scope[index]);
      highs.push(scope[index + 1]);
    }
  }

  return <div className={`synth-scope ${live ? 'live' : 'preview'}`}>
    <svg viewBox={`0 0 ${columns} 100`} preserveAspectRatio="none" role="img" aria-label={live ? 'Instrument output' : 'Patch preview, one A4 note'}>
      <g className="synth-scope-grid" aria-hidden="true">
        {[25, 50, 75].map(row => <line key={row} x1={0} x2={columns} y1={row} y2={row} />)}
        {[25, 50, 75].map(column => <line key={column} x1={column} x2={column} y1={0} y2={100} />)}
      </g>
      {live
        ? <path className="synth-scope-body live" d={band(lows, highs)} />
        : <>
          <path className="synth-scope-body" d={band(preview.low, preview.high)} />
          <path className="synth-scope-osc two" d={line(preview.osc2)} />
          <path className="synth-scope-osc one" d={line(preview.osc1)} />
        </>}
    </svg>
  </div>;
}
