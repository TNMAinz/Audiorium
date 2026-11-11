import type { SynthPack, SynthPackCatalogue, SynthPresetSummary } from './model';

/** Every word the search box can match: the pack, its family, its blurb and its patches. */
export function packMatches(pack: SynthPack, query: string): boolean {
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return true;
  const haystack = `${pack.name} ${pack.family} ${pack.blurb} ${pack.patches.map(patch => patch.name).join(' ')}`.toLowerCase();
  return words.every(word => haystack.includes(word));
}

/**
 * The packs on show: one family at a time, narrowed by the search box. Both are
 * narrowing steps, so nothing can widen the list back out.
 */
export function filterPacks(packs: SynthPack[], query: string, family: string): SynthPack[] {
  return packs.filter(pack => (family === '' || pack.family === family) && packMatches(pack, query));
}

/** How many packs each family holds, for the counts on the filter row. */
export function familyCounts(packs: SynthPack[], families: string[]): { family: string; count: number }[] {
  const counts = new Map<string, number>();
  packs.forEach(pack => counts.set(pack.family, (counts.get(pack.family) ?? 0) + 1));
  return families.map(family => ({ family, count: counts.get(family) ?? 0 }));
}

/** Every patch in the catalogue, so the page can count what it is offering. */
export function patchCount(catalogue: SynthPackCatalogue): number {
  return catalogue.packs.reduce((total, pack) => total + pack.patches.length, 0);
}

/**
 * The name of the patch the engine is holding, whichever book it came from: a
 * shipped starting point, a pack's patch, or one this user saved. A pack patch is
 * named with its pack so a header never shows a bare word from a shelf of two
 * hundred.
 */
export function activePatchName(
  catalogue: SynthPackCatalogue,
  presets: SynthPresetSummary[],
  id: string | null,
): string {
  if (!id) return '';
  const shipped = presets.find(preset => preset.id === id);
  if (shipped) return shipped.name;
  for (const pack of catalogue.packs) {
    const found = pack.patches.find(patch => patch.id === id);
    if (found) return `${found.name} · ${pack.name}`;
  }
  return '';
}
