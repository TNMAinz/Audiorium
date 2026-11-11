import { useCallback, useRef, useState } from 'react';
import { arcPath, clamp, dragToValue, knobAngle, knobFraction, polar, quantize, stepValue, KNOB_START, KNOB_SWEEP } from './scales';

interface Props {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  /** Shown under the knob, in monospace. */
  format: (value: number) => string;
  onChange: (value: number) => void;
  /** Reached by double-click, like a hardware detent. */
  reset?: number;
  /** Tick marks drawn on the scale, in value units. */
  ticks?: number[];
  /** Draw the value arc from the centre (for bipolar controls). */
  bipolar?: boolean;
  size?: number;
  disabled?: boolean;
  /** Longer text for assistive technology, if the label needs it. */
  description?: string;
  /** Shown under the knob when the accessible name has to be longer than the face allows. */
  caption?: string;
  /**
   * A control that runs on a transformed scale (a logarithm, say) still has to
   * announce the values it means. These are what assistive tech reads; the
   * control keeps working in the units it was given.
   */
  announced?: { min: number; max: number; now: number };
}

/**
 * A rotary control with a real scale. Drag up or down (hold Shift for fine),
 * arrow keys step, Page Up/Down jump ten steps, double-click returns to the
 * detent, and the SVG arc always shows where the value sits.
 */
export function Knob({
  label, value, min, max, step, format, onChange, reset, ticks = [], bipolar = false,
  size = 52, disabled = false, description, announced, caption,
}: Props) {
  const drag = useRef<{ startValue: number; startY: number } | null>(null);
  const [dragging, setDragging] = useState(false);
  const centre = size / 2;
  const radius = centre - 7;
  const angle = knobAngle(value, min, max);
  const originAngle = bipolar ? knobAngle(clamp(0, min, max), min, max) : KNOB_START;
  const pointer = polar(centre, centre, radius - 1, angle);
  const pointerBase = polar(centre, centre, radius * 0.42, angle);

  const commit = useCallback((next: number) => {
    const snapped = quantize(next, min, max, step);
    if (snapped !== value) onChange(snapped);
  }, [max, min, onChange, step, value]);

  const onPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (disabled) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { startValue: value, startY: event.clientY };
    setDragging(true);
  };
  const onPointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const state = drag.current;
    if (!state || disabled) return;
    commit(dragToValue({
      start: state.startValue, dy: event.clientY - state.startY, min, max, step,
      fine: event.shiftKey,
    }));
  };
  const endDrag = (event: React.PointerEvent<HTMLDivElement>) => {
    if (!drag.current) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
    drag.current = null;
    setDragging(false);
  };
  const onKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (disabled) return;
    const fine = event.shiftKey ? 0.2 : 1;
    const map: Record<string, () => void> = {
      ArrowUp: () => commit(stepValue(value, 1, step * fine, min, max)),
      ArrowRight: () => commit(stepValue(value, 1, step * fine, min, max)),
      ArrowDown: () => commit(stepValue(value, -1, step * fine, min, max)),
      ArrowLeft: () => commit(stepValue(value, -1, step * fine, min, max)),
      PageUp: () => commit(stepValue(value, 1, step, min, max, 10)),
      PageDown: () => commit(stepValue(value, -1, step, min, max, 10)),
      Home: () => commit(min),
      End: () => commit(max),
    };
    const action = map[event.key];
    if (!action) return;
    event.preventDefault();
    action();
  };

  return <div className={`knob ${dragging ? 'dragging' : ''} ${disabled ? 'disabled' : ''}`}>
    <div
      className="knob-dial"
      role="slider"
      tabIndex={disabled ? -1 : 0}
      aria-label={label}
      aria-valuemin={announced?.min ?? min}
      aria-valuemax={announced?.max ?? max}
      aria-valuenow={announced?.now ?? value}
      aria-valuetext={format(value)}
      aria-description={description}
      aria-disabled={disabled || undefined}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
      onDoubleClick={() => { if (!disabled) commit(reset ?? min); }}
      onKeyDown={onKeyDown}
      title={`${label}: ${format(value)}${description ? ` — ${description}` : ''}`}
    >
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} aria-hidden="true">
        <path className="knob-track" d={arcPath(centre, centre, radius, KNOB_START, KNOB_START + KNOB_SWEEP)} />
        {ticks.map(tick => {
          const tickAngle = knobAngle(tick, min, max);
          const outer = polar(centre, centre, radius - 2.5, tickAngle);
          const inner = polar(centre, centre, radius - 6, tickAngle);
          return <line key={tick} className="knob-tick" x1={inner.x} y1={inner.y} x2={outer.x} y2={outer.y} />;
        })}
        {Math.abs(angle - originAngle) > 0.4 && <path
          className={`knob-value ${bipolar && value < 0 ? 'negative' : ''}`}
          d={arcPath(centre, centre, radius, Math.min(originAngle, angle), Math.max(originAngle, angle))}
        />}
        <line className="knob-pointer" x1={pointerBase.x} y1={pointerBase.y} x2={pointer.x} y2={pointer.y} />
        <circle className="knob-cap" cx={centre} cy={centre} r={radius * 0.3} />
      </svg>
    </div>
    <span className="knob-label">{caption ?? label}</span>
    <span className="knob-readout mono">{format(value)}</span>
    <span className="sr-only">{`${label} at ${Math.round(knobFraction(value, min, max) * 100)} percent of range`}</span>
  </div>;
}
