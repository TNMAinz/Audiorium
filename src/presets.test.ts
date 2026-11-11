import { describe, expect, it } from 'vitest';
import { emptyCoverage, emptyPackCatalogue, type SynthPack } from './model';
import { activePatchName, familyCounts, filterPacks, packMatches, patchCount } from './presets';

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
  pack('paper-glow', 'Paper Glow', 'Pad', 'Soft lit pads with little motion.', ['Lantern', 'Wick', 'Lamplit', 'Glow Drift']),
];

describe('the preset shelf', () => {
  it('finds a pack by its own name, ignoring case and spacing', () => {
    expect(packMatches(packs[0], 'amber')).toBe(true);
    expect(packMatches(packs[0], '  AMBER   horizon ')).toBe(true);
    expect(packMatches(packs[0], 'graphite')).toBe(false);
    // Every word has to land somewhere: one match is not a match.
    expect(packMatches(packs[0], 'amber graphite')).toBe(false);
  });

  it('finds a pack by a patch it holds, its family, or its blurb', () => {
    expect(packMatches(packs[0], 'tape choir')).toBe(true);
    expect(packMatches(packs[2], 'pad')).toBe(true);
    expect(packMatches(packs[1], 'low end')).toBe(true);
  });

  it('narrows by family and by search together, and widens again when both are off', () => {
    expect(filterPacks(packs, '', 'Pad').map(entry => entry.id)).toEqual(['amber-horizon', 'paper-glow']);
    // "Amber Glow" is a patch of the first pack, so the word glow finds both; the
    // lantern is only in the second.
    expect(filterPacks(packs, 'glow', 'Pad').map(entry => entry.id)).toEqual(['amber-horizon', 'paper-glow']);
    expect(filterPacks(packs, 'lantern', 'Pad').map(entry => entry.id)).toEqual(['paper-glow']);
    // A search word from the other family cannot survive the family filter.
    expect(filterPacks(packs, 'anchor', 'Pad')).toEqual([]);
    expect(filterPacks(packs, '', '')).toHaveLength(3);
  });

  it('counts what each family holds, and what the catalogue holds in all', () => {
    expect(familyCounts(packs, ['Bass', 'Pad', 'Sub'])).toEqual([
      { family: 'Bass', count: 1 },
      { family: 'Pad', count: 2 },
      // A family the catalogue does not have is still listed, at zero.
      { family: 'Sub', count: 0 },
    ]);
    expect(patchCount({ families: ['Bass', 'Pad'], packs, coverage: emptyCoverage })).toBe(12);
    expect(patchCount(emptyPackCatalogue)).toBe(0);
  });

  it('names the loaded patch from whichever book it came from', () => {
    const catalogue = { families: ['Bass', 'Pad'], packs, coverage: emptyCoverage };
    const presets = [{ id: 'builtin:init', name: 'Init saw', builtin: true }];
    expect(activePatchName(catalogue, presets, 'builtin:init')).toBe('Init saw');
    // A pack patch carries its pack, because its own name is one of eight hundred.
    expect(activePatchName(catalogue, presets, 'pack:paper-glow#2')).toBe('Lamplit · Paper Glow');
    expect(activePatchName(catalogue, presets, null)).toBe('');
    expect(activePatchName(catalogue, presets, 'pack:gone#0')).toBe('');
  });
});
