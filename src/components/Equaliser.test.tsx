import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { Equaliser } from './Equaliser';
import type { EqFilter, EqPresetList, EqResponse, EqSettings } from '../model';

const band = (changes: Partial<EqFilter> = {}): EqFilter => ({
  kind: 'peaking', frequency: 1_200, gainDb: 4.5, q: 1.4, enabled: true, ...changes,
});

const settings = (changes: Partial<EqSettings> = {}): EqSettings => ({
  enabled: true, preampDb: -4.5, filters: [band()], ...changes,
});

/** A 1 kHz +4.5 dB peak, the curve the engine would return for that band. */
const curve: EqResponse = {
  frequencies: [20, 100, 1_000, 10_000, 20_000],
  gains: [0.1, 0.6, 4.0, 0.2, 0.05],
  maxBoostDb: 4.5,
  headroomDb: 0,
};

const shelf: EqFilter = { kind: 'lowshelf', frequency: 105, gainDb: 4, q: 0.7, enabled: true };

/** The shipped list, with the shelf patch active. */
const shipped: EqPresetList = {
  presets: [
    { id: 'builtin:flat', name: 'Flat', builtin: true, note: 'No bands and no preamp: a straight wire.', preampDb: 0, filters: [] },
    {
      id: 'builtin:bass-shelf', name: 'Bass shelf', builtin: true,
      note: 'Low shelf at 105 Hz, +4 dB. Broad lift; asks for 4 dB of headroom.',
      preampDb: 0, filters: [shelf],
    },
  ],
  active: 'builtin:bass-shelf',
};

/** The same list after one edit: a saved copy, which is what is running now. */
const forked: EqPresetList = {
  presets: [...shipped.presets, {
    id: 'custom:1', name: 'Ananda warm', builtin: false,
    note: 'Edited from the built-in Bass shelf.', preampDb: -4.5, filters: [band()],
  }],
  active: 'custom:1',
};

const draw = (
  eq: EqSettings,
  line: EqResponse = curve,
  native = true,
  presets: EqPresetList = shipped,
  patchError = '',
) => renderToStaticMarkup(<Equaliser
  eq={eq}
  curve={line}
  native={native}
  onChange={() => {}}
  presets={presets}
  patchError={patchError}
  onApplyPreset={() => {}}
  onRenamePreset={async () => true}
  onDeletePreset={() => {}}
/>);

describe('equaliser panel', () => {
  it('plots the curve the engine returned, not a shape of its own', () => {
    const markup = draw(settings());
    expect(markup).toContain('class="eq-line"');
    expect(markup).toContain('role="group"');
    expect(markup).toContain('largest boost +4.5 dB');
  });

  it('lists every band in one line with its own numbers', () => {
    const markup = draw(settings({ filters: [band(), band({ kind: 'highshelf', frequency: 8_000, gainDb: -3.2 })] }));
    expect(markup).toContain('Band 1, Peaking, 1.20 kHz · +4.5 dB · Q 1.40');
    expect(markup).toContain('Band 2, High shelf, 8.00 kHz · −3.2 dB · Q 1.40');
    expect(markup).toContain('2 of 24 bands · preamp −4.5 dB');
  });

  it('edits the selected band with a scale on every control', () => {
    const markup = draw(settings());
    expect(markup).toContain('aria-label="Frequency"');
    expect(markup).toContain('aria-valuetext="1.20 kHz"');
    expect(markup).toContain('aria-label="Gain"');
    expect(markup).toContain('aria-valuetext="+4.5 dB"');
    expect(markup).toContain('aria-label="Q"');
    expect(markup).toContain('BAND 01');
  });

  it('takes the gain control away from a filter shape that has no gain', () => {
    const markup = draw(settings({ filters: [band({ kind: 'notch' })] }));
    expect(markup).toContain('aria-label="Gain" aria-valuemin="-24" aria-valuemax="24"');
    expect(markup).toContain('aria-disabled="true"');
    expect(markup).toContain('no gain');
  });

  it('says the stage is bypassed instead of calling it active', () => {
    const markup = draw(settings({ enabled: false }));
    expect(markup).toContain('BYPASSED');
    expect(markup).toContain('Bypassed: the path is untouched');
    // The stored bands stay readable while the stage is out of the path.
    expect(markup).toContain('Band 1, Peaking, 1.20 kHz');
    expect(markup).toContain('aria-pressed="false" aria-label="Equaliser in path"');
  });

  it('warns when a boost has no headroom left', () => {
    const markup = draw(settings(), { ...curve, maxBoostDb: 8, headroomDb: 3.5 });
    expect(markup).toContain('Boosts reach +3.5 dB past full scale');
  });

  it('reports the headroom when the preamp covers the boost', () => {
    const markup = draw(settings(), { ...curve, maxBoostDb: 8, headroomDb: -2.5 });
    expect(markup).toContain('−2.5 dB of headroom at the peak');
  });

  it('shows an empty state that invites a band, and keeps the stage controls', () => {
    const markup = draw(settings({ filters: [], enabled: false }));
    expect(markup).toContain('No bands yet.');
    expect(markup).toContain('aria-label="Preamp"');
    expect(markup).toContain('Add band');
    expect(markup).toContain('Neutral');
  });

  it('does not invent a curve in a browser tab', () => {
    const markup = draw(settings(), { frequencies: [], gains: [], maxBoostDb: 0, headroomDb: 0 }, false);
    expect(markup).toContain('The engine runs in the Windows app');
  });
});

describe('equaliser patches', () => {
  it('names the patch the bands came from, with what it does', () => {
    const markup = draw(settings());
    expect(markup).toContain('aria-label="Equaliser patch"');
    expect(markup).toContain('>Bass shelf</option>');
    expect(markup).toContain('>Flat</option>');
    expect(markup).toContain('Low shelf at 105 Hz, +4 dB.');
  });

  it('offers no rename or delete for a shipped patch', () => {
    const markup = draw(settings());
    expect(markup).not.toContain('aria-label="Rename patch"');
    expect(markup).not.toContain('aria-label="Delete patch"');
  });

  it('offers rename and delete for the saved copy, and lists it second', () => {
    const markup = draw(settings(), curve, true, forked);
    expect(markup).toContain('aria-label="Rename patch"');
    expect(markup).toContain('aria-label="Delete patch"');
    expect(markup).toContain('>Ananda warm</option>');
    expect(markup).toContain('Edited from the built-in Bass shelf.');
    // The shipped patch is still in the list beside the copy.
    expect(markup).toContain('>Bass shelf</option>');
    expect(markup.indexOf('>Ananda warm</option>')).toBeGreaterThan(markup.indexOf('>Bass shelf</option>'));
  });

  it('says so when the bands belong to no patch', () => {
    const markup = draw(settings(), curve, true, { presets: shipped.presets, active: null });
    expect(markup).toContain('Unsaved bands');
    expect(markup).toContain('These bands belong to no patch');
  });

  it('shows a refused name in place of the note', () => {
    const markup = draw(settings(), curve, true, forked, 'A patch name is at most 40 characters');
    expect(markup).toContain('A patch name is at most 40 characters');
    expect(markup).toContain('eq-patch-note warn');
  });

  it('leaves the patch row out of a browser tab, where there is nowhere to save one', () => {
    const markup = draw(settings(), curve, false);
    expect(markup).not.toContain('aria-label="Equaliser patch"');
  });
});

/** The same list with a published correction in front of the shapes. */
const corrected: EqPresetList = {
  presets: [
    {
      id: 'autoeq:sennheiser-hd-600', name: 'Sennheiser HD 600', builtin: true,
      note: '10 bands, preamp −6.3 dB. AutoEq\'s calculation from an oratory1990 measurement.',
      preampDb: -6.3, filters: [shelf],
      source: {
        label: 'AutoEq · measurement by oratory1990 · over-ear',
        url: 'https://example.invalid/hd600',
        terms: 'AutoEq is MIT. Its repository states no separate licence for its result files.',
      },
    },
    ...shipped.presets,
  ],
  active: 'autoeq:sennheiser-hd-600',
};

/** The same list with a profile this app fitted itself in front of both. */
const fitted: EqPresetList = {
  presets: [
    {
      id: 'fit:moondrop-lan-2-ref', name: 'Moondrop LAN 2 (Ref)', builtin: true,
      note: "10 bands, preamp −3.0 dB. This app's own fit to a Super* Review measurement, matched to the Harman IE 2019 target.",
      preampDb: -3, filters: [shelf],
      source: {
        label: 'AUDIORIUM fit · measurement by Super* Review · in-ear',
        url: 'https://squig.link/data/Moondrop%20Lan%202%20ref%20L.txt · target Harman IE 2019',
        terms: 'The measurement belongs to its publisher; the filter set is this app’s own fit to a published target.',
      },
    },
    ...corrected.presets,
  ],
  active: 'fit:moondrop-lan-2-ref',
};

describe('equaliser corrections', () => {
  it('keeps measured corrections apart from shapes written here, and says which is which', () => {
    const markup = draw(settings({ preampDb: -6.3 }), curve, true, corrected);
    expect(markup).toContain('Corrections · 1 measured by oratory1990, computed by AutoEq');
    expect(markup).toContain('Shapes · 2 starting points written here, no measurements');
    // The measured ones come first: they are the ones with evidence behind them.
    expect(markup.indexOf('Corrections · 1')).toBeLessThan(markup.indexOf('Shapes · 2'));
  });

  it('names a correction\'s measurement and terms above the plot', () => {
    const markup = draw(settings({ preampDb: -6.3 }), curve, true, corrected);
    expect(markup).toContain('AutoEq · measurement by oratory1990 · over-ear');
    expect(markup).toContain('AutoEq is MIT.');
    expect(markup).toContain('https://example.invalid/hd600');
    expect(markup).not.toContain('Written here: a starting point');
  });

  it('says a shape was written here rather than leaving the row blank or borrowing a source', () => {
    const markup = draw(settings(), curve, true, { presets: corrected.presets, active: 'builtin:bass-shelf' });
    expect(markup).toContain('Written here: a starting point, no measurement behind these bands.');
    expect(markup).not.toContain('AutoEq · measurement');
  });

  it('keeps a fit in its own group and names the measurement it was fitted from', () => {
    const markup = draw(settings({ preampDb: -3 }), curve, true, fitted);
    expect(markup).toContain('Fits · 1 fitted here to a published measurement');
    expect(markup.indexOf('Corrections · 1')).toBeLessThan(markup.indexOf('Fits · 1'));
    expect(markup.indexOf('Fits · 1')).toBeLessThan(markup.indexOf('Shapes · 2'));
    expect(markup).toContain('AUDIORIUM fit · measurement by Super* Review · in-ear');
    // A fit has to name both halves of what it was made from, or it reads as a vendored file.
    expect(markup).toContain('target Harman IE 2019');
    expect(markup).not.toContain('AutoEq · measurement by oratory1990');
  });

  it('offers no rename or delete for a correction, and does not call it a copy', () => {
    const markup = draw(settings({ preampDb: -6.3 }), curve, true, corrected);
    expect(markup).not.toContain('aria-label="Rename patch"');
    expect(markup).not.toContain('aria-label="Delete patch"');
  });
});
