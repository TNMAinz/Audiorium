import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { Keyboard, type KeyboardProps } from './Keyboard';

const base: Omit<KeyboardProps, 'held'> = {
  lowNote: 48, octaves: 3, onNoteOn: () => {}, onNoteOff: () => {},
};
const render = (props: Partial<KeyboardProps> = {}) =>
  renderToStaticMarkup(<Keyboard {...base} held={new Set<number>()} {...props} />);
const count = (markup: string, tone: string) =>
  (markup.match(new RegExp(`class="piano-key ${tone}`, 'g')) ?? []).length;

describe('keyboard', () => {
  it('draws three whole octaves: a top C, and five sharps under it', () => {
    const markup = render();
    expect(count(markup, 'white')).toBe(22);
    expect(count(markup, 'black')).toBe(15);
    expect(markup).toContain('aria-label="C3"');
    expect(markup).toContain('aria-label="C6"');
    expect(markup).not.toContain('aria-label="C7"');
  });

  it('names every key and lights only the notes that are sounding', () => {
    const markup = render({ held: new Set([60]) });
    expect(markup).toContain('aria-label="A4"');
    expect(markup).toMatch(/aria-label="C4"[^>]*aria-pressed="true"/);
    expect(markup).toContain('class="piano-key white held');
    // One lit key out of the twenty-two whites.
    expect((markup.match(/aria-pressed="true"/g) ?? []).length).toBe(1);
  });

  it('labels every key with the note a drag across it would play', () => {
    // The pointer reads the note from the key under it, so a key without this
    // attribute is a key a drag cannot reach: silent on the way past.
    const notes = [...render().matchAll(/data-note="(\d+)"/g)]
      .map(match => Number(match[1]))
      .sort((a, b) => a - b);
    expect(notes.length).toBe(37);
    expect(new Set(notes).size).toBe(37);
    expect(notes[0]).toBe(48);
    expect(notes[notes.length - 1]).toBe(84);
  });

  it('offers the QWERTY row only when it is asked for', () => {
    const plain = render();
    expect(plain).toContain('aria-label="Keyboard"');
    expect(plain).not.toContain('QWERTY plays from');
    // Nothing is printed on a key that no letter plays.
    expect(plain).not.toContain('piano-hint');
    const typed = render({ computer: true });
    expect(typed).toContain('QWERTY plays from C3');
    expect(typed).toContain('- and = move it an octave');
    // Z is the base octave's C, and the hint is drawn on that key.
    expect(typed).toContain('class="piano-hint" aria-hidden="true">z<');
  });

  it('is disabled as one keyboard, not as a set of keys that still play', () => {
    const markup = render({ disabled: true, computer: true });
    const keys = markup.match(/<button[^>]*class="piano-key[^"]*"[^>]*>/g) ?? [];
    expect(keys.length).toBe(37);
    expect(keys.every(key => key.includes('disabled=""'))).toBe(true);
  });

  it('lays two octaves out like a piano: a white row, then its sharps above it', () => {
    const markup = render({ computer: true });
    const hints: Record<string, string> = {};
    for (const match of markup.matchAll(/<button[^>]*aria-label="([^"]+)"[^>]*>(.*?)<\/button>/g)) {
      const letter = match[2].match(/class="piano-hint" aria-hidden="true">([a-z0-9])</);
      if (letter) hints[match[1]] = letter[1];
    }
    // Z X C V B N M walk the first octave's white keys; S D G H J are its sharps,
    // each above and to the right of the white key it sharpens. Q W E R T Y U are the
    // second octave's whites with 2 3 5 6 7 above them, and I finishes on its C.
    expect(hints).toEqual({
      'C3': 'z', 'C♯3': 's', 'D3': 'x', 'D♯3': 'd', 'E3': 'c',
      'F3': 'v', 'F♯3': 'g', 'G3': 'b', 'G♯3': 'h', 'A3': 'n',
      'A♯3': 'j', 'B3': 'm',
      'C4': 'q', 'C♯4': '2', 'D4': 'w', 'D♯4': '3', 'E4': 'e',
      'F4': 'r', 'F♯4': '5', 'G4': 't', 'G♯4': '6', 'A4': 'y',
      'A♯4': '7', 'B4': 'u', 'C5': 'i',
    });
    // The sharps sit on the row above their whites and the whites stay in their own
    // rows: s d g h j over z x c v b n m, and 2 3 5 6 7 over q w e r t y u.
    for (const note of ['C♯3', 'D♯3', 'F♯3', 'G♯3', 'A♯3']) expect(['s', 'd', 'g', 'h', 'j']).toContain(hints[note]);
    for (const note of ['C3', 'D3', 'E3', 'F3', 'G3', 'A3', 'B3']) expect(['z', 'x', 'c', 'v', 'b', 'n', 'm']).toContain(hints[note]);
    for (const note of ['C♯4', 'D♯4', 'F♯4', 'G♯4', 'A♯4']) expect(['2', '3', '5', '6', '7']).toContain(hints[note]);
    for (const note of ['C4', 'D4', 'E4', 'F4', 'G4', 'A4', 'B4']) expect(['q', 'w', 'e', 'r', 't', 'y', 'u']).toContain(hints[note]);
  });

  it('draws exactly the notes the letters play, so no drawn key is silently silent', () => {
    const markup = render({ octaves: 2, computer: true });
    const notes = [...markup.matchAll(/data-note="(\d+)"/g)].map(match => Number(match[1]));
    expect(notes.length).toBe(25);
    expect(Math.min(...notes)).toBe(48);
    expect(Math.max(...notes)).toBe(72);
    // Every key on the bed wears the key that plays it, in both octaves.
    expect((markup.match(/class="piano-hint"/g) ?? []).length).toBe(25);
  });

  it('moves the bed an octave from a button, whether or not the letters are on', () => {
    for (const markup of [render(), render({ computer: true })]) {
      expect(markup).toContain('aria-label="Move the keyboard down an octave"');
      expect(markup).toContain('aria-label="Move the keyboard up an octave"');
      // The stepper names the octave the bed starts on.
      expect(markup).toContain('class="mono" aria-hidden="true">C3<');
    }
  });

  it('sizes a sharp as a share of a white key, not a share of the whole bed', () => {
    const one = render({ octaves: 1 });
    const three = render({ octaves: 3 });
    const width = (markup: string) => Number(markup.match(/class="piano-key black[^"]*" style="[^"]*width:([\d.]+)%/)?.[1]);
    // Eight whites in one octave, twenty-two in three: the sharp narrows with them.
    expect(width(one)).toBeCloseTo(7.75, 2);
    expect(width(three)).toBeCloseTo(2.82, 2);
  });
});
