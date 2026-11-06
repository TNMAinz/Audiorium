import { useRef } from 'react';
import { clamp, padToValue, quantize, valueToPad } from './scales';

interface Props {
  azimuth: number;
  elevation: number;
  onChange: (next: { azimuth: number; elevation: number }) => void;
  /** Draw the early-reflection rings only when the panner has them on. */
  reflections: boolean;
  disabled?: boolean;
}

const WIDTH = 232;
const HEIGHT = 150;
const CX = WIDTH / 2;
const CY = 70;
const RADIUS = 62;

/**
 * Top view of the listener's head. Drag the source anywhere in front, to the
 * sides or behind; elevation is the vertical axis, so a source above the ears
 * sits toward the top edge. Arrow keys move by one degree, Shift by five.
 */
export function PositionPad({ azimuth, elevation, onChange, reflections, disabled = false }: Props) {
  const svg = useRef<SVGSVGElement>(null);
  const dragging = useRef(false);
  const point = valueToPad(azimuth, elevation, { radius: RADIUS });

  const move = (clientX: number, clientY: number) => {
    const box = svg.current?.getBoundingClientRect();
    if (!box) return;
    const scale = WIDTH / box.width;
    const x = (clientX - box.left) * scale - CX;
    const y = (clientY - box.top) * scale - CY;
    const next = padToValue(x, y, { radius: RADIUS });
    onChange({
      azimuth: quantize(next.azimuth, -180, 180, 1),
      elevation: quantize(next.elevation, -90, 90, 1),
    });
  };

  const key = (event: React.KeyboardEvent<SVGSVGElement>) => {
    if (disabled) return;
    const amount = event.shiftKey ? 5 : 1;
    const next = { azimuth, elevation };
    if (event.key === 'ArrowLeft') next.azimuth = clamp(azimuth - amount, -180, 180);
    else if (event.key === 'ArrowRight') next.azimuth = clamp(azimuth + amount, -180, 180);
    else if (event.key === 'ArrowUp') next.elevation = clamp(elevation + amount, -90, 90);
    else if (event.key === 'ArrowDown') next.elevation = clamp(elevation - amount, -90, 90);
    else if (event.key === 'Home') { next.azimuth = 0; next.elevation = 0; }
    else return;
    event.preventDefault();
    onChange(next);
  };

  return <div className={`position-pad ${disabled ? 'disabled' : ''}`}>
    <svg
      ref={svg}
      viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
      role="slider"
      tabIndex={disabled ? -1 : 0}
      aria-label="Source position"
      aria-valuemin={-180}
      aria-valuemax={180}
      aria-valuenow={Math.round(azimuth)}
      aria-valuetext={`azimuth ${Math.round(azimuth)} degrees, elevation ${Math.round(elevation)} degrees`}
      onPointerDown={event => {
        if (disabled) return;
        event.preventDefault();
        event.currentTarget.setPointerCapture(event.pointerId);
        dragging.current = true;
        move(event.clientX, event.clientY);
      }}
      onPointerMove={event => { if (dragging.current) move(event.clientX, event.clientY); }}
      onPointerUp={event => {
        dragging.current = false;
        if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
      }}
      onPointerCancel={() => { dragging.current = false; }}
      onKeyDown={key}
    >
      <ellipse className="pad-horizon" cx={CX} cy={CY} rx={RADIUS + 14} ry={RADIUS} />
      <ellipse className="pad-ear-line" cx={CX} cy={CY} rx={RADIUS + 14} ry="1" />
      <ellipse className="pad-ear-line" cx={CX} cy={CY} rx="1" ry={RADIUS} />
      {[0.33, 0.66, 1].map(ring => <ellipse key={ring} className="pad-ring" cx={CX} cy={CY} rx={(RADIUS + 14) * ring} ry={RADIUS * ring} />)}
      {reflections && [0.35, 0.6, 0.85].map(ring => <ellipse key={`r${ring}`} className="pad-reflection" cx={CX} cy={CY} rx={(RADIUS + 14) * ring} ry={RADIUS * ring} />)}
      <circle className="pad-head" cx={CX} cy={CY} r="9" />
      <path className="pad-nose" d={`M ${CX} ${CY - 9} l -3 -5 h 6 z`} />
      <text className="pad-front" x={CX} y="8" textAnchor="middle">front</text>
      {[-90, 0, 90].map(degree => <text key={degree} className="pad-degree" x={CX + (degree / 180) * (RADIUS + 14)} y={HEIGHT - 6} textAnchor="middle">{degree}°</text>)}
      {[-45, 45].map(degree => <text key={`e${degree}`} className="pad-degree" x="6" y={CY - (degree / 90) * RADIUS + 3}>{degree > 0 ? '+45°' : '−45°'}</text>)}
      <line className="pad-spoke" x1={CX} y1={CY} x2={CX + point.x} y2={CY + point.y} />
      <circle className="pad-source" cx={CX + point.x} cy={CY + point.y} r="5.5" />
    </svg>
    <div className="pad-readout mono">
      <span>az <b>{Math.round(azimuth)}°</b></span>
      <span>el <b>{Math.round(elevation)}°</b></span>
    </div>
  </div>;
}
