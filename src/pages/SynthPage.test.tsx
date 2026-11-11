import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { defaultChain, defaultMpe, defaultSynth, emptyPackCatalogue, idleSynthStatus } from '../model';
import type { useSynth } from '../useSynth';
import { SynthPage } from './SynthPage';

type Synth = ReturnType<typeof useSynth>;

/** Everything the page reads, with the engine's own defaults unless a test says otherwise. */
function stub(over: Partial<Synth> = {}): Synth {
  return {
    native: true,
    patch: defaultSynth,
    patches: {
      patch: defaultSynth,
      active: 'builtin:init',
      presets: [
        { id: 'builtin:init', name: 'Init saw', builtin: true },
        { id: 'builtin:pad', name: 'Warm pad', builtin: true },
      ],    tables: ['Basic', 'Formant', 'Bell', 'Vocal'],
    mpe: defaultMpe,
    },
    catalogue: emptyPackCatalogue,
    status: idleSynthStatus,
    held: new Set<number>(),
    error: '',
    busy: false,
    bounced: '',
    devices: [],
    deviceError: '',
    apply: () => {},
    edit: () => {},
    noteOn: () => {},
    noteOff: () => {},
    allNotesOff: () => {},
    bend: () => {},
    start: async () => {},
    stop: async () => {},
    record: async () => {},
    bounce: async () => {},
    applyPreset: () => {},
    savePreset: async () => true,
    deletePreset: async () => {},
    refreshDevices: async () => {},
    openDevice: async () => {},
    closeDevice: async () => {},
    setMpe: async () => {},
    tableShape: async () => [],
    scales: [
      { name: 'Equal', cents: new Array(128).fill(0) },
      { name: 'Just major', cents: new Array(128).fill(0) },
    ],
    chooseTable: async () => null,
    chooseScale: async () => null,
    ...over,
  };
}

const page = (over: Partial<Synth> = {}, libraryRoot = 'D:/Music') =>
  renderToStaticMarkup(<SynthPage synth={stub(over)} libraryRoot={libraryRoot} chain={defaultChain} />);

describe('synth page', () => {
  it('keeps the whole instrument reachable, with the detailed panels in dialogs', () => {
    const markup = page();
    // The faceplate carries the four ways into the panels, each naming its setting.
    for (const tab of ['Oscillators', 'Filter &amp; LFO', 'Envelopes', 'Modulation', 'Take &amp; MIDI']) expect(markup).toContain(tab);
    // Five tabs and the patch chip in the header all open a dialog.
    expect((markup.match(/aria-haspopup="dialog"/g) ?? []).length).toBe(6);
    // No panel is lost behind a dialog: all of them are on the page, just closed.
    expect(markup).toContain('Oscillator 1');
    expect(markup).toContain('Oscillator 2');
    expect(markup).toContain('Filter');
    expect(markup).toContain('LFO');
    expect(markup).toContain('Amplitude envelope');
    expect(markup).toContain('Filter envelope');
    expect(markup).toContain('Patch book');
    expect(markup).toContain('Init saw');
    expect(markup).toContain('Warm pad');
    // Every dialog is a dialog: the faceplate is not one. Six panels hold the
    // instrument's controls, and the wavetable editor has a seventh of its own.
    expect((markup.match(/<dialog/g) ?? []).length).toBe(7);
    // The keyboard is drawn on the page over the two octaves the QWERTY row plays,
    // from C3 to the C above it: nothing on the bed is out of reach of the letters.
    expect(markup).toContain('aria-label="Keyboard"');
    expect(markup).toContain('aria-label="C3"');
    expect(markup).toContain('aria-label="C5"');
    expect(markup).not.toContain('aria-label="C6"');
  });

  it('draws each envelope as one unbroken path from zero and back', () => {
    const markup = page();
    const paths = [...markup.matchAll(/class="synth-graph-line" d="([^"]+)"/g)].map(match => match[1]);
    expect(paths.length).toBe(2);
    for (const d of paths) {
      // A malformed join shows up as two numbers run together, which is not a path.
      expect(d).toMatch(/^M-?[\d.]+,-?[\d.]+(?:L-?[\d.]+,-?[\d.]+)+$/);
      expect(d.startsWith('M0,')).toBe(true);
      expect(d.endsWith('L100,92.00')).toBe(true);
    }
  });

  it('draws the patch itself while the engine is stopped', () => {
    const markup = page();
    expect(markup).toContain('preview · A4');
    expect(markup).toContain('Patch preview, one A4 note');
    expect(markup).toContain('class="synth-scope-osc one"');
    expect(markup).toContain('osc 1');
  });

  it('draws the output the engine wrote while it runs', () => {
    const scope = [0.1, 0.2, -0.3, 0.4, 0.05, -0.5, 0, 0.7];
    const markup = page({ status: { ...idleSynthStatus, running: true, voices: 3, device: 'Speakers', scope } });
    expect(markup).toContain('Instrument output');
    expect(markup).not.toContain('Patch preview, one A4 note');
    expect(markup).toContain('class="synth-scope-body live"');
    // One closed shape, not a stroke per column: a peak-held column drawn on its own
    // is a bar, and ninety-six bars read as a comb instead of a waveform.
    const body = /class="synth-scope-body live" d="([^"]+)"/.exec(markup)?.[1] ?? '';
    expect(body.startsWith('M')).toBe(true);
    expect(body.endsWith('Z')).toBe(true);
    expect((body.match(/M/g) ?? []).length).toBe(1);
  });

  it('names the rack the voices play through', () => {
    const markup = page();
    expect(markup).toContain('Effects');
    expect(markup).toContain('Binaural panner');
    expect(markup).toContain('Limiter');
    expect(markup).toContain('all bypassed');
    expect(markup).toContain('The rack is the instrument');
  });

  it('names what the voice is doing in the header', () => {
    const markup = page();
    expect(markup).toContain('Saw + Saw');
    expect(markup).toContain('LP 3.20 kHz');
    expect(markup).toContain('instrument idle');
  });

  it('reports the voices the engine says are sounding', () => {
    const markup = page({ status: { ...idleSynthStatus, running: true, voices: 3, device: 'Speakers' } });
    expect(markup).toContain('Running');
    expect(markup).toContain('3 voices');
    expect(markup).toContain('Speakers');
  });

  it('refuses a bounce it has nowhere to write, and says why', () => {
    const markup = page({}, '');
    expect(markup).toContain('Choose a library folder on the Library page');
    expect(markup).toMatch(/disabled=""[^>]*>Bounce to WAV/);
  });

  it('carries the engine\'s refusal instead of hiding it', () => {
    const markup = page({ error: 'Cutoff is outside 20 to 20000' });
    expect(markup).toContain('Cutoff is outside 20 to 20000');
  });

  it('gives the matrix curves and the faceplate four macros to route', () => {
    const markup = page();
    // Each macro is drawn twice: once on the faceplate and once in the matrix
    // panel, so a route's source has a control in front of it.
    for (const name of ['Macro 1', 'Macro 2', 'Macro 3', 'Macro 4']) {
      const labels = markup.match(new RegExp(`class="knob-label">${name}<`, 'g')) ?? [];
      expect(labels.length, `${name} is drawn twice`).toBe(2);
    }
    // The new filter models are in the picker, and a table can be brought in
    // without leaving the panel.
    expect(markup).toContain('>Notch<');
    expect(markup).toContain('>Peak<');
    expect(markup).toContain('aria-label="Drive"');
    expect(markup).toContain('Import table…');
  });

  it('writes a curve on every route it draws', () => {
    const markup = page({
      patch: {
        ...defaultSynth,
        mods: [{ source: 'env2', target: 'cutoff', amount: 0.6, curve: 'log' }],
      },
    });
    expect(markup).toMatch(/<select[^>]*aria-label="Route 1 curve"/);
    // The curve list is the engine's four shapes, not a free-text amount.
    for (const curve of ['>Linear<', '>Exp<', '>Log<', '>S<']) expect(markup).toContain(curve);
  });

  it('names the tuning and how far it moves the keys', () => {
    const tuning = new Array(128).fill(0);
    // E4 is key 64, and just intonation puts it 13.7 cents flat of equal.
    tuning[64] = -13.7;
    const markup = page({ patch: { ...defaultSynth, tuning } });
    expect(markup).toContain('1 key off equal · widest −13.7 ¢ at E4');
    expect(markup).toContain('aria-label="Tuning"');
    expect(markup).toContain('Import Scala…');
    expect(markup).toContain('Equal temperament');
  });
});
