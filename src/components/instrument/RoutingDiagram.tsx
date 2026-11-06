import { reverbStructures } from '../../model';

interface Props {
  value: string;
  onChange: (structure: string) => void;
}

const width = 54;
const height = 30;

/** The three routings drawn as what they are: parallel sums, a series chain, a woven field. */
function Diagram({ structure }: { structure: string }) {
  if (structure === 'parallel') {
    return <svg viewBox={`0 0 ${width} ${height}`} aria-hidden="true">
      {[8, 15, 22].map(y => <path key={y} className="route-line" d={`M 6 15 L 16 ${y} L 38 ${y} L 48 15`} />)}
      <circle className="route-node" cx="6" cy="15" r="2" />
      <circle className="route-node" cx="48" cy="15" r="2" />
    </svg>;
  }
  if (structure === 'series') {
    return <svg viewBox={`0 0 ${width} ${height}`} aria-hidden="true">
      <path className="route-line" d="M 4 15 L 50 15" />
      {[12, 27, 42].map(x => <rect key={x} className="route-box" x={x - 5} y="9" width="10" height="12" rx="2" />)}
    </svg>;
  }
  return <svg viewBox={`0 0 ${width} ${height}`} aria-hidden="true">
    <path className="route-line" d="M 5 6 C 22 6, 22 24, 49 24" />
    <path className="route-line" d="M 5 15 L 49 15" />
    <path className="route-line" d="M 5 24 C 22 24, 22 6, 49 6" />
    <circle className="route-node" cx="5" cy="15" r="2" />
    <circle className="route-node" cx="49" cy="15" r="2" />
  </svg>;
}

export function RoutingDiagram({ value, onChange }: Props) {
  const current = value.toLowerCase();
  return <div className="routing" role="radiogroup" aria-label="Reverb routing">
    {reverbStructures.map(structure => <button
      key={structure}
      type="button"
      role="radio"
      aria-checked={current === structure}
      aria-label={`${structure} routing`}
      className={`routing-option ${current === structure ? 'selected' : ''}`}
      onClick={() => onChange(structure)}
    >
      <Diagram structure={structure} />
      <span>{structure}</span>
    </button>)}
  </div>;
}
