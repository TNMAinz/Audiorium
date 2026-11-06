import { useRef } from 'react';
import { spatialModes, type SpatialMode } from '../../model';

interface Props {
  value: SpatialMode;
  onChange: (mode: SpatialMode) => void;
  /** Two-column bank of short labels, for the instrument stack. */
  compact?: boolean;
  label?: string;
}

/**
 * The spatial mode as a hardware selector: one lit position, no dropdown.
 * Arrow keys walk the bank and move focus with the selection.
 */
export function ModeSelector({ value, onChange, compact = false, label = 'Spatial mode' }: Props) {
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);
  const index = Math.max(0, spatialModes.findIndex(mode => mode.id === value));
  const move = (delta: number) => {
    const next = (index + delta + spatialModes.length) % spatialModes.length;
    onChange(spatialModes[next].id);
    buttons.current[next]?.focus();
  };
  return <div
    className={`mode-selector ${compact ? 'compact' : ''}`}
    role="radiogroup"
    aria-label={label}
    onKeyDown={event => {
      if (event.key === 'ArrowRight' || event.key === 'ArrowDown') { event.preventDefault(); move(1); }
      if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') { event.preventDefault(); move(-1); }
    }}
  >
    {spatialModes.map((mode, position) => <button
      key={mode.id}
      ref={element => { buttons.current[position] = element; }}
      type="button"
      role="radio"
      aria-checked={mode.id === value}
      aria-label={mode.label}
      tabIndex={mode.id === value ? 0 : -1}
      className={mode.id === value ? 'selected' : ''}
      title={mode.label}
      onClick={() => onChange(mode.id)}
    >{compact ? mode.short : mode.label}</button>)}
  </div>;
}
