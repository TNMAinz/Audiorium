import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { noteName } from '../../model';
import { slideNote } from './slide';

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
/**
 * The QWERTY keys, laid out the way a piano is. Two letter rows are the white keys of
 * two octaves — `z x c v b n m` and `q w e r t y u` — and the row above each one holds
 * that octave's sharps, sitting above and to the right of the white key they belong to:
 * `s` is C sharp above `z`, `d` is D sharp above `x`. `i` is the second octave's C, so
 * the row reaches two whole octaves and the keyboard draws nothing the letters cannot
 * play. This is the layout every tracker and DAW's computer keyboard uses, because the
 * hands already know it: the left hand walks the low octave, the right hand the high one.
 */
const bindings: Record<string, number> = {
  z: 0, s: 1, x: 2, d: 3, c: 4, v: 5, g: 6, b: 7, h: 8, n: 9, j: 10, m: 11,
  q: 12, '2': 13, w: 14, '3': 15, e: 16, r: 17, '5': 18, t: 19, '6': 20, y: 21, '7': 22, u: 23, i: 24,
};
/** What is printed on each key, one key per note, so no key wears two hints. */
const hints: Record<number, string> = {};
Object.entries(bindings).forEach(([key, step]) => { hints[step] = key; });
/** The two keys that move the bed instead of playing it: they are the only keys in
 * the rows around the notes that are not notes themselves. */
const octaveKeys: Record<string, number> = { '-': -12, '=': 12 };

/**
 * A playable keyboard. Every key is a real button, so it can be reached by the
 * tab key and read by assistive technology; the pointer and the QWERTY row both
 * send the same note on and note off the engine already knows.
 */
export function Keyboard({
  lowNote, octaves, held, onNoteOn, onNoteOff, computer = false, disabled = false,
}: KeyboardProps) {
  // The bed can be moved an octave at a time without changing the patch.
  const [base, setBase] = useState(lowNote);
  useEffect(() => { setBase(lowNote); }, [lowNote]);
  const typing = useRef(new Set<number>());
  // The bed never leaves the MIDI range, and it is the same bed the letters play on,
  // so the last whole octave it can start on is the highest that still fits.
  const span = octaves * 12;
  const highest = Math.max(0, Math.floor((127 - span) / 12) * 12);
  const shift = (by: number) => {
    // A letter let go after the bed moved would name a different note, so the notes
    // the letters are holding are released before the bed slides out from under them.
    typing.current.forEach(note => onNoteOff(note));
    typing.current.clear();
    setBase(current => Math.min(highest, Math.max(0, current + by)));
  };
  // The bed follows the octave the letters start on, whether or not QWERTY is on: a
  // key that is drawn is a key the mouse can play, and one that is labelled is a key
  // the letters can play, so the two never disagree about what is on show.
  const bed = Math.min(base, highest);

  const { whites, blacks, blackWidth } = useMemo(() => {
    const whites: { note: number; left: number }[] = [];
    const blacks: { note: number; left: number }[] = [];
    const count = octaves * 7 + 1;
    const width = 100 / count;
    for (let octave = 0; octave < octaves; octave += 1) {
      naturals.forEach(offset => {
        whites.push({ note: bed + octave * 12 + offset, left: whites.length * width });
        if (sharps[offset] !== undefined) {
          // A black key straddles the line its natural ends on.
          blacks.push({ note: bed + octave * 12 + sharps[offset], left: whites.length * width });
        }
      });
    }
    // The bed ends on a top C, not on a B: every octave it draws is whole.
    whites.push({ note: bed + octaves * 12, left: whites.length * width });
    // A sharp is a share of a white key, never a fixed share of the whole bed: the
    // same keyboard then looks the same whether one octave is drawn or four.
    return { whites, blacks, blackWidth: width * 0.62 };
  }, [bed, octaves]);

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
      const octave = octaveKeys[key];
      if (octave !== undefined) {
        shift(octave);
        return;
      }
      const step = bindings[key];
      if (step === undefined) return;
      const note = Math.min(127, bed + step);
      if (typing.current.has(note)) return;
      typing.current.add(note);
      onNoteOn(note, 0.85);
    };
    const up = (event: KeyboardEvent) => {
      const step = bindings[event.key.toLowerCase()];
      if (step === undefined) return;
      const note = Math.min(127, bed + step);
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
  }, [computer, disabled, bed, onNoteOn, onNoteOff]);

  // A pointer on the keys is a finger on a real keyboard: pressing one starts it,
  // and dragging moves that note to whatever key is under the cursor, releasing the
  // one it left. A pointer that slides off the keys keeps its place on the bed until
  // it is let go — its entry goes null rather than away — so a drag that travels over
  // the gap between two keys, off the edge of the bed or onto the panel below comes
  // back playing instead of going silent for the rest of the hold. Each pointer is
  // tracked on its own, so a touch screen can hold one note and slide another without
  // either letting go of the first.
  const pointers = useRef(new Map<number, number | null>());

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
    const change = slideNote(pointers.current.get(pointerId), note);
    if (!change) return;
    if (change.off !== null) onNoteOff(change.off);
    pointers.current.set(pointerId, change.on);
    if (change.on !== null) onNoteOn(change.on, 0.85);
  }, [onNoteOn, onNoteOff]);

  useEffect(() => {
    const move = (event: PointerEvent) => {
      if (!pointers.current.has(event.pointerId)) return;
      slide(event.pointerId, noteAt(event.clientX, event.clientY));
    };
    // A release outside the keys, or a cancelled pointer, still ends the note the
    // drag started: nothing is left ringing because the cursor travelled off the bed.
    // This is the only place a pointer leaves the map, because the release is the only
    // thing that ends a drag.
    const end = (event: PointerEvent) => {
      if (!pointers.current.has(event.pointerId)) return;
      slide(event.pointerId, null);
      pointers.current.delete(event.pointerId);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', end);
    window.addEventListener('pointercancel', end);
    return () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', end);
      window.removeEventListener('pointercancel', end);
      pointers.current.forEach(note => { if (note !== null) onNoteOff(note); });
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
    const hint = computer ? hints[note - bed] : undefined;
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
    <div className="piano-tools">
      <p className="panel-note piano-note">{computer
        ? `QWERTY plays from ${noteName(bed)}. - and = move it an octave.`
        : `The arrows move the keyboard an octave; every key is played by the mouse.`}</p>
      {/* The bed moves with the pointer too, and it moves whether or not the letters
          are on, so the keys on show are never stuck on one octave. */}
      <div className="piano-octave">
        <button type="button" aria-label="Move the keyboard down an octave" onClick={() => shift(-12)}>−</button>
        <span className="mono" aria-hidden="true">{noteName(bed)}</span>
        <button type="button" aria-label="Move the keyboard up an octave" onClick={() => shift(12)}>+</button>
      </div>
    </div>
  </div>;
}
