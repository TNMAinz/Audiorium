import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { defaultSynth, emptyPackCatalogue, type SynthCoverage, type SynthPack, type SynthPackCatalogue, type SynthPatchList } from '../model';
import type { useSynth } from '../useSynth';
import { PresetsPage } from './PresetsPage';

const pack = (id: string, name: string, family: string, blurb: string, patches: string[]): SynthPack => ({
  id,
  name,
  family,
  blurb,
  patches: patches.map((patch, index) => ({ id: `pack:${id}#${index}`, name: patch })),
});

const packs = [
  pack('amber-horizon', 'Amber Horizon', 'Pad', 'Sun warmed pads that drift.', ['Amber Glow', 'Sundown', 'Tape Choir', 'Slow Burn']),
  pack('graphite-anchor', 'Graphite Anchor', 'Bass', 'Short tuned low end.', ['Anchor', 'Ballast', 'Keel', 'Girder']),
];

const coverage: SynthCoverage = {
  rows: [
    { label: 'Families', covered: 12, total: 12 },
    { label: 'Wave pairs', covered: 16, total: 16 },
    { label: 'Filter models', covered: 7, total: 7 },
    { label: 'Grid cells', covered: 336, total: 336 },
  ],
  gridPacks: 336,
  gridPatches: 1344,
};
const shelf: SynthPackCatalogue = { families: ['Bass', 'Pad'], packs, coverage };

/** The page reads three fields of the hook; the rest of it is not its business. */
const synth = (patches: Partial<SynthPatchList> = {}, catalogue = shelf) =>
  ({ catalogue, patches: { patch: defaultSynth, active: null, presets: [], tables: [], mpe: { lowerMembers: 0, upperMembers: 0, memberBend: 48, masterBend: 2 }, ...patches }, applyPreset: () => {} } as unknown as ReturnType<typeof useSynth>);

const draw = (patches: Partial<SynthPatchList> = {}, catalogue = shelf) =>
  renderToStaticMarkup(<PresetsPage synth={synth(patches, catalogue)} />);
const loadedRows = (markup: string) => [...markup.matchAll(/class="presets-patch loaded"/g)].length;

describe('presets page', () => {
  it('draws a card per pack with its family, its blurb and its four patches', () => {
    const markup = draw();
    for (const name of ['Amber Horizon', 'Graphite Anchor']) expect(markup).toContain(`>${name}</h2>`);
    for (const family of ['Pad', 'Bass']) expect(markup).toContain(`class="presets-family-tag mono">${family}<`);
    expect(markup).toContain('Sun warmed pads that drift.');
    // Every patch of every pack is a button: eight rows for two packs.
    expect((markup.match(/class="presets-patch /g) ?? []).length).toBe(8);
    expect(markup).toContain('Tape Choir');
    expect(markup).toContain('Girder');
  });

  it('counts the shelf and the family filter row from the catalogue itself', () => {
    const markup = draw();
    expect(markup).toContain('2 packs · 8 patches · none loaded');
    // What the shelf shows out of what it holds, beside the search box.
    expect(markup).toContain('class="mono presets-count">2 of 2<');
    // All, then each family the engine named, with its count.
    expect(markup).toContain('>All<span class="mono">2</span>');
    expect(markup).toContain('>Bass<span class="mono">1</span>');
    expect(markup).toContain('>Pad<span class="mono">1</span>');
  });

  it('marks the loaded patch and only that one', () => {
    const markup = draw({ active: 'pack:paper-glow#0' });
    expect(loadedRows(markup)).toBe(0);
    const on = draw({ active: 'pack:amber-horizon#2' });
    expect(loadedRows(on)).toBe(1);
    // The lit row is the patch the engine holds, not the first row of the card.
    expect(on).toMatch(/class="presets-patch loaded"[^>]*>.*?Tape Choir/s);
    expect(on).toContain('>loaded</em>');
    expect(on).toContain('2 packs · 8 patches · Tape Choir · Amber Horizon');
  });

  it('reports what the shelf covers on each axis, counted from the catalogue', () => {
    const markup = draw();
    expect(markup).toContain('aria-label="Catalogue coverage"');
    for (const label of ['Families', 'Wave pairs', 'Filter models', 'Grid cells']) expect(markup).toContain(`<em>${label}</em>`);
    // Every row is full, so no row is drawn as a gap.
    expect(markup).toContain('12 of 12');
    expect((markup.match(/presets-coverage-row full/g) ?? []).length).toBe(4);
    expect(markup).not.toContain('presets-coverage-row gap');
    expect(markup).toContain('336 grid packs · 1344 patches');
  });

  it('shows a short row as a gap rather than hiding it', () => {
    const short = draw({}, { ...shelf, coverage: { ...coverage, rows: [{ label: 'Grid cells', covered: 335, total: 336 }] } });
    expect(short).toContain('presets-coverage-row gap');
    expect(short).toContain('335 of 336');
  });

  it('offers the search box and an honest empty shelf', () => {
    expect(draw()).toContain('aria-label="Search packs"');
    const empty = draw({}, emptyPackCatalogue);
    expect(empty).toContain('No packs to show.');
    expect(empty).toContain('read from the engine when it is running');
    expect(empty).not.toContain('class="presets-card"');
  });
});
