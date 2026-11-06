import { reflectionTaps } from './scales';

interface Props {
  length: number;
  level: number;
  active: boolean;
}

const width = 150;
const height = 26;
const span = 26;

/** The four early taps the panner actually renders, placed by their real delays. */
export function TapStrip({ length, level, active }: Props) {
  const taps = reflectionTaps(length, level);
  return <div className={`tap-strip ${active ? '' : 'idle'}`}>
    <svg viewBox={`0 0 ${width} ${height}`} role="img" aria-label={`Early reflections at ${taps.map(tap => `${tap.ms} milliseconds`).join(', ')}`}>
      <line className="tap-axis" x1="0" y1={height - 1} x2={width} y2={height - 1} />
      {taps.map((tap, index) => {
        const x = 3 + (tap.ms / span) * (width - 6);
        const top = height - 1 - tap.level * (height - 5);
        return <g key={index}>
          <line className="tap-stem" x1={x} y1={height - 1} x2={x} y2={top} />
          <circle className="tap-head" cx={x} cy={top} r="2" />
        </g>;
      })}
    </svg>
    <span className="tap-times mono" aria-hidden="true" title={`${taps.map(tap => tap.ms).join(' · ')} ms`}>{taps.map(tap => tap.ms.toFixed(1)).join(' · ')} ms</span>
    <span className="panel-note">Four discrete taps · {active ? `${Math.round(level * 100)}% level` : 'reflections off'}</span>
  </div>;
}
