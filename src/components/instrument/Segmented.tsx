interface Option { id: string; label: string }

interface Props {
  label: string;
  options: Option[];
  value: string;
  onChange: (id: string) => void;
  /** Compact variant for material chips. */
  compact?: boolean;
}

/** A hardware-style segmented switch: one lit choice, no dropdown. */
export function Segmented({ label, options, value, onChange, compact = false }: Props) {
  return <div className={`segmented ${compact ? 'compact' : ''}`} role="radiogroup" aria-label={label}>
    {options.map(option => <button
      key={option.id}
      type="button"
      role="radio"
      aria-checked={value === option.id}
      className={value === option.id ? 'selected' : ''}
      onClick={() => onChange(option.id)}
    >{option.label}</button>)}
  </div>;
}
