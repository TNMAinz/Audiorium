import { useEffect, useMemo, useRef, useState } from 'react';
import { sourceName } from '../model';
import type { AnalysisStatus } from '../useAnalysis';

const BAND_COUNT = 56;
const WIDTH = 320;
const HEIGHT = 96;
/** How many past frames the surface carries: at the 20 ms poll, about half a second. */
const LAYERS = 24;
/** How far each older frame sits above the live one, in viewBox units. */
const DEPTH = 1.8;
/** The floor the live ridge stands on, and the height it can reach. */
const BASE = HEIGHT - 8;
const RANGE = HEIGHT - 16;

/** Where a frame of a given age sits, and how far it can rise. */
const baseline = (depth: number) => BASE - depth * DEPTH;
/** A frame never rises past the top of the plot, so nothing is drawn over the heading. */
const amplitude = (depth: number) => Math.min(RANGE * (1 - depth * 0.006), baseline(depth) - 2);

const axis = ['20', '100', '1K', '10K', '20K'];
/** A crest below this much energy is silence, and a crest drawn there is a stray line. */
const CREST_GATE = 0.06;

type Point = readonly [number, number];

/** A smoothed path through the points, or nothing when there are too few to draw. */
function smooth(points: Point[]) {
  if (points.length < 2) return '';
  let path = `M${points[0][0].toFixed(1)} ${points[0][1].toFixed(1)}`;
  for (let i = 1; i < points.length; i++) {
    const [x0, y0] = points[i - 1];
    const [x1, y1] = points[i];
    const mid = (x0 + x1) / 2;
    path += ` C${mid.toFixed(1)} ${y0.toFixed(1)} ${mid.toFixed(1)} ${y1.toFixed(1)} ${x1.toFixed(1)} ${y1.toFixed(1)}`;
  }
  return path;
}

/**
 * One frame of the surface: the measured curve as a ridge over a baseline lifted by
 * its age, closed down to the floor so the fills stack into a landscape.
 */
function ridge(values: number[], depth: number) {
  const base = baseline(depth);
  const amp = amplitude(depth);
  // The live curve is drawn band by band; a faint frame behind it needs a quarter of
  // the points to read the same, and thirty of them at full resolution cost the frame.
  const step = depth === 0 ? 1 : 4;
  const sampled = values
    .map((value, index) => ({ value: Math.min(1, Math.max(0, value)), index }))
    .filter(({ index }) => index % step === 0 || index === values.length - 1);
  const points = sampled.map(({ value, index }) => [
    (index / Math.max(1, values.length - 1)) * WIDTH,
    base - value * amp,
  ] as const);
  const line = smooth(points);
  // A frame behind the front one traces only the parts of its ridge that stand above
  // silence. Its whole outline would draw a parallel line for every frame wherever the
  // spectrum is quiet, which reads as a hatch over the empty half of the plot.
  const runs: Point[][] = [];
  let run: Point[] = [];
  sampled.forEach(({ value }, index) => {
    if (value >= CREST_GATE) run.push(points[index]);
    else if (run.length) { runs.push(run); run = []; }
  });
  if (run.length > 1) runs.push(run);
  // The frame in front fills to the floor; the ones behind it only glow a short way
  // below their own baseline, so a quiet band does not hatch the panel with bands.
  const bottom = depth === 0 ? HEIGHT : Math.min(HEIGHT, base + 15);
  return {
    line,
    crest: runs.map(smooth).filter(Boolean).join(' '),
    filled: `${line} L${WIDTH} ${bottom} L0 ${bottom} Z`,
  };
}

/** The floor when nothing plays: one straight line, not a filled band. */
function floorLine() {
  return `M0 ${HEIGHT - 8} H${WIDTH}`;
}

/** How long the last landscape takes to fade once the output stops, in milliseconds. */
export const SURFACE_FADE_MS = 700;

/**
 * Whether the last picture stands as it is. A pause is a hold, not an ending: the player
 * is still loaded and resumes where it stopped, so both plots keep the last window and
 * carry on from it. A stop is neither, and only that gives the picture up.
 */
export function holdsLastFrame(playing: boolean, paused: boolean) {
  return playing || paused;
}

/** The opacity the held landscape keeps with `remaining` milliseconds of its fade left. */
export function surfaceFade(remaining: number) {
  return Math.min(1, Math.max(0, remaining / SURFACE_FADE_MS));
}

/**
 * The landscape the last output left behind. While it plays the plot owns its frames; a
 * pause keeps them exactly as they are, and a stop fades them out, so the surface settles
 * instead of being replaced by the floor on the next poll. The memory is kept in a ref so
 * the poll, which re-renders this component every 20 ms, does not restart the fade.
 */
function useFadingSurface(live: boolean, paused: boolean, frames: number[][]) {
  const last = useRef<number[][]>([]);
  const [fade, setFade] = useState(1);
  useEffect(() => { if (live) last.current = frames; });
  useEffect(() => {
    if (holdsLastFrame(live, paused) || !last.current.length) {
      setFade(1);
      return;
    }
    const ended = performance.now() + SURFACE_FADE_MS;
    const timer = window.setInterval(() => {
      const left = surfaceFade(ended - performance.now());
      setFade(left);
      if (left === 0) window.clearInterval(timer);
    }, 40);
    return () => window.clearInterval(timer);
  }, [live, paused]);
  return { frames: fade > 0 ? (live ? frames : last.current) : [], fade: holdsLastFrame(live, paused) ? 1 : fade };
}

export function SpectrumLandscape({ status, surface = [] }: { status: AnalysisStatus; surface?: number[][] }) {
  const live = status.playing && status.spectrum.length > 0;
  const paused = status.paused;
  // Oldest first, the frame the engine just published last, so the live curve is the
  // last path in the plot and the one on top.
  const frames = live
    ? [...surface, status.spectrum.slice(0, BAND_COUNT)].slice(-LAYERS)
    : [];
  const { frames: shown, fade: settle } = useFadingSurface(live, paused, frames);
  // One glow per age, anchored to that frame's own baseline: warm along the ridge,
  // blue just under it, gone below. Filling to the panel floor instead would stack
  // thirty-two fills into one slab. These depend on nothing that changes between
  // frames, so they are built once and React can skip them on every poll.
  const gradients = useMemo(() => <defs>
    {Array.from({ length: LAYERS }, (_, depth) => <linearGradient
      key={depth}
      id={`spectrum-ridge-${depth}`}
      gradientUnits="userSpaceOnUse"
      x1="0"
      y1={baseline(depth) - amplitude(depth)}
      x2="0"
      y2={baseline(depth) + (depth === 0 ? 26 : 15)}
    >
      {/* The front frame is lit along its crest. The ones behind it are dark at their
          own crest and glow under it: a lit crest on all thirty-two would draw thirty-two
          parallel lines wherever the spectrum is quiet. */}
      {depth === 0 ? <>
        <stop offset="0" stopColor="#ffb877" stopOpacity="0.7" />
        <stop offset="0.42" stopColor="var(--accent-2)" stopOpacity="0.34" />
        <stop offset="0.72" stopColor="var(--accent)" stopOpacity="0.22" />
      </> : <>
        <stop offset="0" stopColor="#ffb877" stopOpacity="0" />
        <stop offset="0.3" stopColor="var(--accent-2)" stopOpacity="0.26" />
        <stop offset="0.66" stopColor="var(--accent)" stopOpacity="0.15" />
      </>}
      <stop offset="1" stopColor="#1e3a6b" stopOpacity="0" />
    </linearGradient>)}
    <linearGradient id="spectrum-fade" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="0" y2={HEIGHT}>
      <stop offset="0" stopColor="#05070d" stopOpacity="0.7" />
      <stop offset="0.58" stopColor="#05070d" stopOpacity="0" />
    </linearGradient>
  </defs>, []);

  return <div className="spectrum">
    <svg viewBox={`0 0 ${WIDTH} ${HEIGHT}`} preserveAspectRatio="none" clipPath="inset(0)" role="img" aria-label={live ? `Real FFT spectrum of ${sourceName(status.source)}, 30 Hz to 16 kHz, with the last half second behind it` : paused ? 'Spectrum paused: the last half second is held where it was, ready to carry on' : 'Spectrum idle: nothing is being measured, so the display is flat'}>
      {gradients}
      {[0.25, 0.5, 0.75].map(fraction => <path key={fraction} d={`M0 ${(HEIGHT - 8) * fraction + 4} H${WIDTH}`} stroke="rgb(var(--accent-rgb) / .12)" strokeWidth="1" />)}
      {/* The surface: every frame the engine published in the last half second,
          oldest and dimmest at the back, the one it just measured last and brightest
          in front. The glow behind is what makes the ridges read as depth. */}
      {/* A paused landscape is held; a stopped one fades as a whole. Its own age still
          dims the frames behind the front one. */}
      <g opacity={settle}>
        {shown.map((frame, index) => {
          const depth = shown.length - 1 - index;
          const newest = depth === 0;
          const { line, crest, filled } = ridge(frame, depth);
          const age = Math.pow(0.945, depth);
          return newest
            ? <g key={depth}>
              <path d={filled} fill={`url(#spectrum-ridge-${depth})`} stroke="none" />
              <path d={line} fill="none" stroke="#eaf2ff" strokeWidth={1.4} strokeLinejoin="round" />
            </g>
            : <g key={depth} aria-hidden="true" opacity={0.8 * age}>
              <path d={filled} fill={`url(#spectrum-ridge-${depth})`} stroke="none" />
              {crest && <path d={crest} fill="none" stroke="var(--accent-light)" strokeWidth={0.6} strokeLinejoin="round" opacity={0.5} />}
            </g>;
        })}
      </g>
      {/* Older frames fall back into the dark, so the surface has depth instead of
          being a hatch of parallel lines. */}
      {shown.length > 0 && <rect x="0" y="0" width={WIDTH} height={HEIGHT} fill="url(#spectrum-fade)" aria-hidden="true" />}
      {/* A stopped instrument reads as a flat floor, not a landscape; a paused one keeps
          the picture it stopped on. */}
      {!live && !paused && <path d={floorLine()} fill="none" stroke="var(--accent)" strokeWidth={1} strokeLinejoin="round" opacity={0.4} />}
    </svg>
    <div className="spectrum-axis">{axis.map(mark => <span key={mark}>{mark}</span>)}</div>
  </div>;
}

const FLOOR_DB = -60;

function toDecibels(value: number) {
  return value > 0 ? Math.max(FLOOR_DB, 20 * Math.log10(value)) : FLOOR_DB;
}

function position(decibels: number) {
  return Math.min(1, Math.max(0, (decibels - FLOOR_DB) / -FLOOR_DB));
}

export function PeakMeter({ status }: { status: AnalysisStatus }) {
  const shown = holdsLastFrame(status.playing, status.paused);
  const peakDb = shown ? toDecibels(status.peak) : FLOOR_DB;
  const rmsDb = shown ? toDecibels(status.rms) : FLOOR_DB;
  const display = (value: number) => (shown && value > FLOOR_DB ? value.toFixed(1).replace('-', '−') : '—');
  return <div className="peak-meter">
    <div className="peak-readout">
      <strong>{display(toDecibels(status.rms))}</strong>
      <span>dBFS RMS</span>
      <small>peak {display(peakDb)}</small>
    </div>
    <div className={`peak-meter-body ${shown ? 'live' : ''}`} aria-hidden="true">
      <div className="peak-track"><div className="peak-fill" style={{ width: `${position(peakDb) * 100}%` }} /><div className="peak-rms" style={{ left: `${position(rmsDb) * 100}%` }} /></div>
      <div className="peak-ticks">{[-6, -12, -18, -24, -30, -36, -42, -48].map(db => <span key={db}>{db}</span>)}</div>
    </div>
  </div>;
}

/** Where a value from −1 to +1 sits on a scale drawn left to right. */
const signed = (value: number) => Math.min(1, Math.max(0, (value + 1) / 2));
/** The real minus sign, as every other readout in the app uses. */
const signedNumber = (value: number) => `${value > 0 ? '+' : value < 0 ? '−' : ''}${Math.abs(value).toFixed(2)}`;

/** How long the last trace takes to leave the scope once the output stops, in milliseconds. */
export const TRACE_FADE_MS = 1000;

/**
 * The share of the trace each poll removes while the output is stopped: light at first,
 * heavier as the pause ages, so a pause reads as the field settling instead of the
 * measurement blanking on the first idle frame.
 */
export function traceFade(age: number) {
  return 0.04 + 0.1 * Math.min(1, Math.max(0, age / TRACE_FADE_MS));
}

const GONIO_SIZE = 200;
const GONIO_CENTER = GONIO_SIZE / 2;
const GONIO_RADIUS = 92;
/** A half-scale ring, so the cloud has a size reference of its own. */
const GONIO_HALF = GONIO_RADIUS / 2;
/** Where the ±45° diagonals meet the ring: hard left and hard right. */
const GONIO_DIAGONAL = GONIO_RADIUS / Math.SQRT2;

/**
 * One sample on the goniometer: mid runs up the vertical axis, side runs across it with a
 * left-heavy pair on the left, so the picture agrees with the L / R diagonals and the
 * balance readout below it.
 *
 * A stereo frame with neither channel past the window's peak lives in a diamond in
 * mid/side — `|mid| + |side| ≤ peak` — so a plain `mid × scale, side × scale` plot keeps
 * every sample inside it: hard-panned material stops at 71 % of the ring and only mono ever
 * touches it. The diamond is stretched out to the ring instead, keeping each sample's
 * direction and reading its level as a fraction of full scale in that direction. The
 * window's loudest sample then lands on the ring whatever its pan, and the whole circle is
 * reachable.
 */
export function goniometerPoint(mid: number, side: number, scale: number) {
  const distance = Math.hypot(mid, side);
  if (distance === 0) return { x: GONIO_CENTER, y: GONIO_CENTER };
  const reach = scale * (Math.abs(mid) + Math.abs(side));
  return {
    x: GONIO_CENTER - (side / distance) * reach,
    y: GONIO_CENTER - (mid / distance) * reach,
  };
}

/** The scale is drawn once and left alone: only the cloud under it changes. */
const GONIOMETER_SCALE = <svg className="goniometer-grid" viewBox={`0 0 ${GONIO_SIZE} ${GONIO_SIZE}`} aria-hidden="true">
  <circle className="goniometer-ring" cx={GONIO_CENTER} cy={GONIO_CENTER} r={GONIO_RADIUS} />
  <circle className="goniometer-ring faint" cx={GONIO_CENTER} cy={GONIO_CENTER} r={GONIO_HALF} />
  <line className="goniometer-axis" x1={GONIO_CENTER} y1={GONIO_CENTER - GONIO_RADIUS} x2={GONIO_CENTER} y2={GONIO_CENTER + GONIO_RADIUS} />
  <line className="goniometer-axis" x1={GONIO_CENTER - GONIO_RADIUS} y1={GONIO_CENTER} x2={GONIO_CENTER + GONIO_RADIUS} y2={GONIO_CENTER} />
  <line className="goniometer-diagonal" x1={GONIO_CENTER - GONIO_DIAGONAL} y1={GONIO_CENTER - GONIO_DIAGONAL} x2={GONIO_CENTER + GONIO_DIAGONAL} y2={GONIO_CENTER + GONIO_DIAGONAL} />
  <line className="goniometer-diagonal" x1={GONIO_CENTER - GONIO_DIAGONAL} y1={GONIO_CENTER + GONIO_DIAGONAL} x2={GONIO_CENTER + GONIO_DIAGONAL} y2={GONIO_CENTER - GONIO_DIAGONAL} />
  <text className="goniometer-label" x={GONIO_CENTER} y={11} textAnchor="middle">M</text>
  <text className="goniometer-label" x={GONIO_CENTER - GONIO_DIAGONAL - 8} y={GONIO_CENTER - GONIO_DIAGONAL + 3} textAnchor="middle">L</text>
  <text className="goniometer-label" x={GONIO_CENTER + GONIO_DIAGONAL + 8} y={GONIO_CENTER - GONIO_DIAGONAL + 3} textAnchor="middle">R</text>
</svg>;

/**
 * One window's trace, drawn on top of what the scope already holds. The window's kept
 * samples are consecutive again, so they are joined: the Lissajous path is what a
 * goniometer shows, and a cloud of unrelated dots is only dust. Additive, so a dense
 * passage burns brighter than a sparse one instead of reading as one blob.
 */
function drawTrace(context: CanvasRenderingContext2D, status: AnalysisStatus, width: number, height: number, ratio: number) {
  // The ring is the window's own loudest sample: a quiet passage draws small rather than
  // being stretched to fill the circle.
  const size = Math.min(width, height);
  const left = (width - size) / 2;
  const top = (height - size) / 2;
  const unit = size / GONIO_SIZE;
  const scale = GONIO_RADIUS / Math.max(0.02, status.peak);
  context.globalCompositeOperation = 'lighter';
  context.strokeStyle = 'rgba(196, 178, 255, 0.14)';
  context.lineWidth = Math.max(0.8, 0.9 * ratio);
  context.lineJoin = 'round';
  context.lineCap = 'round';
  context.beginPath();
  let previous: { x: number; y: number } | null = null;
  for (let index = 0; index + 1 < status.vectorscope.length; index += 2) {
    const point = goniometerPoint(status.vectorscope[index], status.vectorscope[index + 1], scale);
    const across = point.x - GONIO_CENTER;
    const up = point.y - GONIO_CENTER;
    const reach = Math.hypot(across, up);
    const fit = reach > GONIO_RADIUS ? GONIO_RADIUS / reach : 1;
    const x = left + (GONIO_CENTER + across * fit) * unit;
    const y = top + (GONIO_CENTER + up * fit) * unit;
    if (previous) {
      context.moveTo(previous.x, previous.y);
      context.lineTo(x, y);
    }
    previous = { x, y };
  }
  context.stroke();
}

/**
 * What the two output channels have in common, and whether the pair is level with itself.
 * The goniometer plots mid vertically against side horizontally from the window's own
 * samples, so a mono file draws a vertical line, an inverted pair a horizontal one, and a
 * wide mix a cloud. The correlation and the balance are the same window, as numbers.
 */
export function StereoField({ status }: { status: AnalysisStatus }) {
  const live = status.playing;
  const paused = status.paused;
  // A pause holds the last window on the scope, so the readings it produced stay up while
  // the player is paused and only a stop reports no output.
  const held = holdsLastFrame(live, paused);
  const correlation = held ? status.correlation : 0;
  const balance = held ? status.balance : 0;
  const verdict = !held
    ? 'NO OUTPUT'
    : correlation > 0.85
      ? 'MONO-COMPATIBLE'
      : correlation >= 0.2
        ? 'IN PHASE'
        : correlation > -0.2
          ? 'WIDE'
          : 'OUT OF PHASE';
  const side = Math.round(Math.abs(balance) * 100);
  const balanceWord = !held ? '—' : side < 2 ? 'CENTRE' : `${side} % ${balance > 0 ? 'RIGHT' : 'LEFT'}`;
  const points = status.vectorscope.length / 2;
  const cloud = useRef<HTMLCanvasElement>(null);
  /** When the output stopped, and whether the trace has finished leaving the scope. */
  const stoppedAt = useRef(0);
  const spent = useRef(true);

  // A phosphor scope, not a picture per poll. The canvas fades what it drew last time and
  // adds this window's points on top, so the field builds a trace instead of replacing 240
  // dots fifty times a second. Drawing here also keeps those points out of the React tree:
  // the poll re-renders the readouts, not the cloud.
  useEffect(() => {
    const canvas = cloud.current;
    const context = canvas?.getContext('2d');
    if (!canvas || !context) return;
    const box = canvas.getBoundingClientRect();
    const ratio = Math.min(2, window.devicePixelRatio || 1);
    const width = Math.max(1, Math.round(box.width * ratio));
    const height = Math.max(1, Math.round(box.height * ratio));
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
      // Resizing clears the canvas, so whatever it held is gone: a pause redraws its
      // held window, and a stop has nothing left to drain.
      spent.current = true;
    }
    if (!live) {
      // A pause is a hold: the canvas keeps exactly what the last window drew and paints
      // nothing, so the field freezes where the music left it. A scope mounted during the
      // pause has nothing on it yet, so it draws the held window once instead of showing
      // an empty field beside a frozen landscape.
      if (paused) {
        if (spent.current && status.vectorscope.length) {
          drawTrace(context, status, width, height, ratio);
          spent.current = false;
        }
        return;
      }
      // A stop is a decay, not a cut. The canvas keeps what the last window drew and wipes
      // a little more of it on each poll, so the field ebbs out over about a second; once
      // it has drained the poll stops painting an empty canvas.
      if (spent.current) return;
      if (!stoppedAt.current) stoppedAt.current = performance.now();
      const age = performance.now() - stoppedAt.current;
      context.globalCompositeOperation = 'destination-out';
      context.fillStyle = `rgba(0, 0, 0, ${traceFade(age)})`;
      context.fillRect(0, 0, width, height);
      if (age > TRACE_FADE_MS) {
        context.clearRect(0, 0, width, height);
        spent.current = true;
      }
      return;
    }
    stoppedAt.current = 0;
    spent.current = false;
    // destination-out fades the old points and leaves the card's panel showing through.
    context.globalCompositeOperation = 'destination-out';
    context.fillStyle = 'rgba(0, 0, 0, 0.18)';
    context.fillRect(0, 0, width, height);
    drawTrace(context, status, width, height, ratio);
  }, [status, live, paused]);

  return <div className="stereo-field">
    <div className="goniometer-wrap">
      <div
        className="goniometer"
        role="img"
        aria-label={live
          ? `Goniometer of ${sourceName(status.source)}: mid vertically, side horizontally, ${points} points from the window just measured`
          : paused
            ? `Goniometer paused: mid vertically, side horizontally, ${points} points held from the window that was measured`
            : 'Goniometer idle: nothing is being measured, so the field is empty'}
      >
        {GONIOMETER_SCALE}
        <canvas ref={cloud} className="goniometer-cloud" aria-hidden="true" />
      </div>
    </div>
    <div className="phase-readout">
      <strong>{held ? signedNumber(correlation) : '—'}</strong>
      <span>correlation</span>
      <em className={verdict === 'OUT OF PHASE' ? 'warn' : ''}>{verdict}</em>
    </div>
    <div
      className="phase-scale"
      role="meter"
      aria-label="Stereo correlation of the measured signal"
      aria-valuemin={-1}
      aria-valuemax={1}
      aria-valuenow={Number(correlation.toFixed(2))}
      aria-valuetext={held ? `correlation ${signedNumber(correlation)}, ${verdict.toLowerCase()}` : 'no signal'}
    >
      <div className="phase-centre" />
      <div className="phase-needle" style={{ left: `${signed(correlation) * 100}%` }} />
    </div>
    <div className="phase-ticks" aria-hidden="true"><span>−1</span><span>0</span><span>+1</span></div>
    <div className="balance-row">
      <span aria-hidden="true">L</span>
      <div
        className="balance-track"
        role="meter"
        aria-label="Stereo balance of the measured signal"
        aria-valuemin={-1}
        aria-valuemax={1}
        aria-valuenow={Number(balance.toFixed(2))}
        aria-valuetext={held ? `balance ${balanceWord.toLowerCase()}` : 'no signal'}
      >
        <div className="balance-centre" />
        <div className="balance-mark" style={{ left: `${signed(balance) * 100}%` }} />
      </div>
      <span aria-hidden="true">R</span>
      <strong>{balanceWord}</strong>
    </div>
    <p className="panel-note">Ring = this window's loudest sample, in its own direction. +1 folds to mono; a needle left of centre cancels.</p>
  </div>;
}
