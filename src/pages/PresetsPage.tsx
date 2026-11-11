import { useMemo, useState } from 'react';
import type { useSynth } from '../useSynth';
import { activePatchName, familyCounts, filterPacks, patchCount } from '../presets';
import { Page } from './Page';

interface Props {
  synth: ReturnType<typeof useSynth>;
  /** Where the loaded patch can be played; the page loads, the Synth page plays. */
  onOpenSynth?: () => void;
}

/**
 * The preset ecosystem: the factory packs, four patches each, browsable by family
 * and by word. A card is a pack — what it is called, what it is for, and the four
 * patches with the one the engine is holding already marked — so nothing needs a
 * second panel to become playable.
 */
export function PresetsPage({ synth, onOpenSynth }: Props) {
  const { catalogue, patches } = synth;
  const { coverage } = catalogue;
  const [query, setQuery] = useState('');
  const [family, setFamily] = useState('');
  const shown = useMemo(() => filterPacks(catalogue.packs, query, family), [catalogue.packs, query, family]);
  const counts = useMemo(() => familyCounts(catalogue.packs, catalogue.families), [catalogue.packs, catalogue.families]);
  const patchesInAll = useMemo(() => patchCount(catalogue), [catalogue]);
  const loaded = activePatchName(catalogue, patches.presets, patches.active);
  const state = catalogue.packs.length
    ? `${catalogue.packs.length} packs · ${patchesInAll} patches${loaded ? ` · ${loaded}` : ' · none loaded'}`
    : 'The packs are read from the engine when it is running.';

  return <Page
    title="Presets"
    state={state}
    className="presets-page"
    actions={onOpenSynth && <button type="button" className="page-button ghost" onClick={onOpenSynth}>Open Synth</button>}
  >
    <section className="page-card presets-toolbar" aria-label="Find a pack">
      <div className="presets-find">
        <input
          className="synth-name"
          type="search"
          value={query}
          placeholder="Search packs, patches, families"
          aria-label="Search packs"
          onChange={event => setQuery(event.target.value)}
        />
        <span className="mono presets-count">{shown.length} of {catalogue.packs.length}</span>
      </div>
      <div className="presets-families" role="group" aria-label="Pack families">
        <button
          type="button"
          className={`presets-family ${family === '' ? 'on' : ''}`}
          aria-pressed={family === ''}
          onClick={() => setFamily('')}
        >All<span className="mono">{catalogue.packs.length}</span></button>
        {counts.map(entry => <button
          key={entry.family}
          type="button"
          className={`presets-family ${family === entry.family ? 'on' : ''}`}
          aria-pressed={family === entry.family}
          disabled={entry.count === 0}
          onClick={() => setFamily(family === entry.family ? '' : entry.family)}
        >{entry.family}<span className="mono">{entry.count}</span></button>)}
      </div>
      {coverage.rows.length > 0 && <div className="presets-coverage" aria-label="Catalogue coverage">
        <span className="presets-coverage-title mono">Coverage</span>
        <ul className="presets-coverage-rows">
          {coverage.rows.map(row => <li
            key={row.label}
            className={`presets-coverage-row ${row.covered === row.total ? 'full' : 'gap'}`}
          >
            <span className="mono">{row.covered} of {row.total}</span>
            <em>{row.label}</em>
          </li>)}
        </ul>
        <span className="presets-coverage-grid mono">
          {coverage.gridPacks} grid packs · {coverage.gridPatches} patches
        </span>
      </div>}
      <p className="panel-note">Each pack is four patches of one voice: the pack's own, a softer take, a harder one, and one that moves. A grid pack is one cell of the systematic shelf — a family, one of the oscillator's shapes and a filter model — so the four takes are that cell moved four ways. Loading one puts it in the instrument; Open Synth plays it.</p>
    </section>

    {!catalogue.packs.length && <p className="panel-note presets-empty">No packs to show.</p>}
    {catalogue.packs.length > 0 && !shown.length && <p className="panel-note presets-empty">
      Nothing matches {query ? `“${query}”` : 'this family'}{family ? ` in ${family}` : ''}.
    </p>}

    <div className="presets-grid">
      {shown.map(pack => <article className="presets-card" key={pack.id} aria-label={`${pack.name} pack`}>
        <header className="presets-head">
          <span className="presets-family-tag mono">{pack.family}</span>
          <h2>{pack.name}</h2>
          <p>{pack.blurb}</p>
        </header>
        <div className="presets-patches">
          {pack.patches.map(patch => {
            const on = patches.active === patch.id;
            return <button
              key={patch.id}
              type="button"
              className={`presets-patch ${on ? 'loaded' : ''}`}
              aria-pressed={on}
              title={`${patch.name} — load into the instrument`}
              onClick={() => synth.applyPreset(patch.id)}
            >
              <i className="presets-lamp" aria-hidden="true" />
              <span>{patch.name}</span>
              <em className="mono">{on ? 'loaded' : 'load'}</em>
            </button>;
          })}
        </div>
      </article>)}
    </div>
  </Page>;
}
