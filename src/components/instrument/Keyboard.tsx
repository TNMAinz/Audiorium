import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { noteName } from '../../model';

export interface KeyboardProps {
  /** MIDI note of the leftmost white key; always a C. */
  lowNote: number;
  /** Octaves drawn, 1 to 4. */
  octaves: number;
  /** The notes sounding right now, for the lit keys. */
  held: ReadonlySet<number>;
  onNoteOn: (note: number, velocity: number) => void;
  onNoteOff: (note: number) => void;
  /** When true the QWERTY row plays, so the instrument can be tried without MIDI. */
  computer?: boolean;
  disabled?: boolean;
}

/** The seven natural notes of an octave, in semitones from C. */
const naturals = [0, 2, 4, 5, 7, 9, 11];
/** The five sharps, keyed by the natural they sit above. */
const sharps: Record<number, number> = { 0: 1, 2: 3, 5: 6, 7: 8, 9: 10 };
/** The QWERTY row, in the shape of a keyboard: A is C, W is C sharp, S is D, and so
 * on up to K, which is the octave's C. The two rows are chromatic from end to end. */
const letters = ['a', 'w', 's', 'e', 'd', 'f', 't', 'g', 'y', 'h', 'u', 'j', 'k'];
const steps = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
const hints: Record<number, string> = Object.fromEntries(steps.map((step, index) => [step, letters[index]]));

/**
 * A playable keyboard. Every key is a real button, so it can be reached by the
 * tab key and read by assistive technology; the pointer and the QWERTY row both
 * send the same note on and note off the engine already knows.
 */
export function Keyboard({
  lowNote, octaves, held, onNoteOn, onNoteOff, computer = false, disabled = false,
}: KeyboardProps) {
  // The QWERTY row can be moved an octave at a time without changing the patch.
  const [base, setBase] = useState(lowNote);
  useEffect(() => { setBase(lowNote); }, [lowNote]);
  const typing = useRef(new Set<number>());
  // The QWERTY row moves an octave at a time, by Z and X or by the two buttons
  // under the keys. It never leaves the MIDI range.
  const shift = (steps: number) => setBase(current => Math.min(108, Math.max(0, current + steps)));

  const { whites, blacks, blackWidth } = useMemo(() => {
    const whites: { note: number; left: number }[] = [];
    const blacks: { note: number; left: number }[] = [];
    const count = octaves * 7 + 1;
    const width = 100 / count;
    for (let octave = 0; octave < octaves; octave += 1) {
      naturals.forEach(offset => {
        whites.push({ note: lowNote + octave * 12 + offset, left: whites.length * width });
        if (sharps[offset] !== undefined) {
          // A black key straddles the line its natural ends on.
          blacks.push({ note: lowNote + octave * 12 + sharps[offset], left: whites.length * width });
        }
      });
    }
    // A three-octave keyboard ends on a top C, not on a B: the octave is whole.
    whites.push({ note: lowNote + octaves * 12, left: whites.length * width });
    // A sharp is a share of a white key, never a fixed share of the whole bed: the
    // same keyboard then looks the same whether one octave is drawn or four.
    return { whites, blacks, blackWidth: width * 0.62 };
  }, [lowNote, octaves]);

  useEffect(() => {
    if (!computer || disabled) return;
    const releaseAll = () => {
      typing.current.forEach(note => onNoteOff(note));
      typing.current.clear();
    };
    const inField = (target: EventTarget | null) =>
      target instanceof HTMLElement
      && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable);
    const down = (event: KeyboardEvent) => {
      if (event.repeat || event.ctrlKey || event.metaKey || event.altKey || inField(event.target)) return;
      const key = event.key.toLowerCase();
      if (key === 'z' || key === 'x') {
        shift(key === 'x' ? 12 : -12);
        return;
      }
      const at = letters.indexOf(key);
      if (at < 0) return;
      const note = Math.min(127, base + steps[at]);
      if (typing.current.has(note)) return;
      typing.current.add(note);
      onNoteOn(note, 0.85);
    };
    const up = (event: KeyboardEvent) => {
      const at = letters.indexOf(event.key.toLowerCase());
      if (at < 0) return;
      const note = base + steps[at];
      if (!typing.current.delete(note)) return;
      onNoteOff(note);
    };
    window.addEventListener('keydown', down);
    window.addEventListener('keyup', up);
    // A window that loses focus never sends the key up, so the note is released
    // here instead of ringing on until the next click.
    window.addEventListener('blur', releaseAll);
    return () => {
      window.removeEventListener('keydown', down);
      window.removeEventListener('keyup', up);
      window.removeEventListener('blur', releaseAll);
      releaseAll();
    };
  }, [computer, disabled, base, onNoteOn, onNoteOff]);

  // A pointer on the keys is a finger on a real keyboard: pressing one starts it,
  // and dragging moves that note to whatever key is under the cursor, releasing the
  // one it left. Each pointer is tracked on its own, so a touch screen can hold one
  // note and slide another without either letting go of the first.
  const pointers = useRef(new Map<number, number>());

  /** The note under a point, or null when the point is on no key at all. */
  const noteAt = useCallback((clientX: number, clientY: number) => {
    if (disabled) return null;
    const under = document.elementFromPoint(clientX, clientY);
    const key = under instanceof Element ? under.closest<HTMLElement>('.piano-key') : null;
    const note = key?.dataset.note;
    return note === undefined ? null : Number(note);
  }, [disabled]);

  /** Sounds `note` for one pointer, releasing what that same pointer held before. */
  const slide = useCallback((pointerId: number, note: number | null) => {
    const current = pointers.current.get(pointerId);
    if (current === note) return;
    if (current !== undefined) onNoteOff(current);
    if (note === null) pointers.current.delete(pointerId);
    else {
      pointers.current.set(pointerId, note);
      onNoteOn(note, 0.85);
    }
  }, [onNoteOn, onNoteOff]);

  useEffect(() => {
    const move = (event: PointerEvent) => {
      if (!pointers.current.has(event.pointerId)) return;
      slide(event.pointerId, noteAt(event.clientX, event.clientY));
    };
    // A release outside the keys, or a cancelled pointer, still ends the note the
    // drag started: nothing is left ringing because the cursor travelled off the bed.
    const end = (event: PointerEvent) => {
      if (!pointers.current.has(event.pointerId)) return;
      slide(event.pointerId, null);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', end);
    window.addEventListener('pointercancel', end);
    return () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', end);
      window.removeEventListener('pointercancel', end);
      pointers.current.forEach(note => onNoteOff(note));
      pointers.current.clear();
    };
  }, [noteAt, slide, onNoteOff]);

  const press = (event: React.PointerEvent<HTMLDivElement>) => {
    if (disabled || event.button !== 0) return;
    // A drag across the keys is playing, not selecting text or dragging a key image.
    event.preventDefault();
    slide(event.pointerId, noteAt(event.clientX, event.clientY));
  };

  const key = (note: number, tone: 'white' | 'black', left: number) => {
    const on = held.has(note);
    const hint = computer ? hints[note - base] : undefined;
    return <button
      key={note}
      type="button"
      className={`piano-key ${tone} ${on ? 'held' : ''}`}
      style={tone === 'black' ? { left: `${left}%`, width: `${blackWidth}%` } : undefined}
      aria-label={noteName(note)}
      aria-pressed={on}
      data-note={note}
      disabled={disabled}
    >
      {hint && <span className="piano-hint" aria-hidden="true">{hint}</span>}
      {(tone === 'white' && note % 12 === 0) && <span className="piano-name" aria-hidden="true">{noteName(note)}</span>}
    </button>;
  };

  return <div className="piano" role="group" aria-label="Keyboard">
    <div className="piano-keys" onPointerDown={press}>
      {whites.map(white => key(white.note, 'white', white.left))}
      {blacks.map(black => key(black.note, 'black', black.left))}
    </div>
    {computer && <div className="piano-tools">
      <p className="panel-note piano-note">QWERTY plays from {noteName(base)}. Z and X move it an octave.</p>
      {/* The row can also be moved with the pointer, for anyone not on the letter keys. */}
      <div className="piano-octave">
        <button type="button" aria-label="Move the QWERTY row down an octave" onClick={() => shift(-12)}>−</button>
        <span className="mono" aria-hidden="true">{noteName(base)}</span>
        <button type="button" aria-label="Move the QWERTY row up an octave" onClick={() => shift(12)}>+</button>
      </div>
    </div>}
  </div>;
}
