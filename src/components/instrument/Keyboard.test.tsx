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
    expect(plain).not.toContain('panel-note piano-note');
    const typed = render({ computer: true });
    expect(typed).toContain('QWERTY plays from C3');
    expect(typed).toContain('Z and X move it an octave');
    // A is the base octave's C, and the hint is drawn on that key.
    expect(typed).toContain('class="piano-hint" aria-hidden="true">a<');
  });

  it('is disabled as one keyboard, not as a set of keys that still play', () => {
    const markup = render({ disabled: true, computer: true });
    const keys = markup.match(/<button[^>]*class="piano-key[^"]*"[^>]*>/g) ?? [];
    expect(keys.length).toBe(37);
    expect(keys.every(key => key.includes('disabled=""'))).toBe(true);
  });

  it('plays the home row chromatically, one letter per semitone', () => {
    const markup = render({ computer: true });
    const hints: Record<string, string> = {};
    for (const match of markup.matchAll(/<button[^>]*aria-label="([^"]+)"[^>]*>(.*?)<\/button>/g)) {
      const letter = match[2].match(/class="piano-hint" aria-hidden="true">([a-z])</);
      if (letter) hints[match[1]] = letter[1];
    }
    // A is C, W is C sharp, S is D: the letters climb one semitone at a time, so a
    // key never plays the note above or below the one its position promises.
    expect(hints).toEqual({
      'C3': 'a', 'C♯3': 'w', 'D3': 's', 'D♯3': 'e', 'E3': 'd',
      'F3': 'f', 'F♯3': 't', 'G3': 'g', 'G♯3': 'y', 'A3': 'h',
      'A♯3': 'u', 'B3': 'j', 'C4': 'k',
    });
  });

  it('moves the QWERTY row by a button as well as by Z and X', () => {
    const markup = render({ computer: true });
    expect(markup).toContain('aria-label="Move the QWERTY row up an octave"');
    expect(markup).toContain('aria-label="Move the QWERTY row down an octave"');
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
