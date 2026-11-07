import { invoke } from '@tauri-apps/api/core';
import { Disc3, Square } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { FileScanView } from '../components/FileScanView';
import { ScrollArea } from '../components/instrument/ScrollArea';
import { Segmented } from '../components/instrument/Segmented';
import {
  depthsFor, masterFormats, masterRates, rateLabel,
  type BitDepth, type LibraryTrack, type LoudnessReadings, type MasterFormat, type SampleRate,
} from '../model';
import { checkMaster, gainToTarget, masterTargets, TARGET_TOLERANCE_LU, type TargetId } from '../masterSpec';
import type { useRender } from '../useRender';
import type { useChain } from '../useChain';
import type { useLibrary } from '../hooks';
import type { usePlayer } from '../usePlayer';
import { formatBytes, Page } from './Page';

interface Props {
  library: ReturnType<typeof useLibrary>;
  player: ReturnType<typeof usePlayer>;
  chain: ReturnType<typeof useChain>;
  /** The render lives above the page, so a render carries on while the user is elsewhere. */
  render: ReturnType<typeof useRender>;
}

/** A real minus sign, like every other reading in the app. */
const minus = (value: number, places = 1) => `${value < 0 ? '−' : ''}${Math.abs(value).toFixed(places)}`;
const lufs = (value: number) => (value <= -69.9 ? '—' : `${minus(value)} LUFS`);
const lu = (value: number) => (value <= 0 ? '—' : `${minus(value)} LU`);
const dbtp = (value: number) => (value <= -119.9 ? '—' : `${minus(value)} dBTP`);
const dbfs = (value: number) => (value <= -119.9 ? '—' : `${minus(value)} dBFS`);
const correlation = (value: number) => (Math.abs(value) < 0.005 ? '0.00' : `${value > 0 ? '+' : '−'}${Math.abs(value).toFixed(2)}`);

/** One row of the before/after table, with the rule for when it is a measured number. */
interface Measure {
  name: string;
  format: (readings: LoudnessReadings) => string;
  ready: (readings: LoudnessReadings) => boolean;
}

const measures: Measure[] = [
  { name: 'Integrated loudness', format: readings => lufs(readings.integratedLufs), ready: readings => readings.blocks > 0 },
  { name: 'Loudness range', format: readings => lu(readings.loudnessRange), ready: readings => readings.blocks > 3 },
  { name: 'True peak', format: readings => dbtp(readings.truePeakDb), ready: readings => readings.blocks > 0 },
  { name: 'Sample peak', format: readings => dbfs(readings.samplePeakDb), ready: readings => readings.blocks > 0 },
  { name: 'DC offset', format: readings => dbfs(readings.dcOffsetDb), ready: readings => readings.blocks > 0 },
  { name: 'Correlation', format: readings => correlation(readings.correlation), ready: readings => readings.blocks > 0 },
];

/** How many of the six processors are switched into the rack right now. */
function activeStages(chain: ReturnType<typeof useChain>) {
  const settings = chain.chain;
  return [
    settings.panner.enabled,
    settings.reverb.enabled,
    settings.eq.enabled && settings.eq.filters.some(filter => filter.enabled),
    settings.compressor.enabled,
    settings.saturator.enabled,
    settings.limiter.enabled,
  ].filter(Boolean).length;
}

/** Tracks that share an album, in the order the library lists them. */
function albumsOf(tracks: LibraryTrack[]) {
  const groups = new Map<string, LibraryTrack[]>();
  for (const track of tracks) {
    const key = `${track.artist || 'Unknown artist'} · ${track.album || 'No album'}`;
    const list = groups.get(key);
    if (list) list.push(track);
    else groups.set(key, [track]);
  }
  return [...groups.entries()]
    .filter(([, list]) => list.length > 1)
    .map(([name, list]) => ({ name, tracks: list }));
}

export function MasterPage({ library, player, chain, render }: Props) {
  const status = render.status;
  const tracks = library.result?.tracks ?? [];
  const [selectedPath, setSelectedPath] = useState('');
  const [referencePath, setReferencePath] = useState('');
  const [reference, setReference] = useState<LoudnessReadings | null>(null);
  const [depth, setDepth] = useState<BitDepth>('24');
  const [targetId, setTargetId] = useState<TargetId>('streaming');
  // Which side of the comparison is playing, and the gain it is being heard at.
  const [side, setSide] = useState<'' | 'source' | 'master' | 'reference'>('');
  const [trim, setTrim] = useState(0);
  const [compareError, setCompareError] = useState('');
  const selected = tracks.find(track => track.path === selectedPath) ?? null;
  const referenceTrack = tracks.find(track => track.path === referencePath) ?? null;
  const albums = useMemo(() => albumsOf(tracks), [tracks]);
  const active = activeStages(chain);
  const depths = depthsFor(render.format);
  const depthLabel = depths.find(option => option.id === depth)?.label ?? '';
  const containerLabel = render.format === 'flac' ? 'FLAC' : 'WAV';
  const target = masterTargets.find(entry => entry.id === targetId) ?? masterTargets[0];
  const spec = checkMaster(target, status.output);
  const gain = gainToTarget(target, status.output);
  const albumDone = status.phase === 'done' && status.trackTotal > 1;

  // FLAC has no float sample, so switching to it drops a float choice to 24-bit
  // rather than sending the engine a pair it refuses.
  const chooseFormat = (next: MasterFormat) => {
    render.setFormat(next);
    if (!depthsFor(next).some(option => option.id === depth)) setDepth('24');
  };

  // A rescan can take the chosen file away. The page then has no source rather
  // than holding a name that no longer exists.
  useEffect(() => {
    if (selectedPath && !selected) setSelectedPath('');
    if (referencePath && !referenceTrack) { setReferencePath(''); setReference(null); }
  }, [selected, selectedPath, referenceTrack, referencePath]);

  const choose = (path: string) => {
    setSelectedPath(path);
    render.chooseSource(path);
  };

  const chooseReference = async (path: string) => {
    setReferencePath(path);
    setSide('');
    setTrim(0);
    if (!path) { setReference(null); return; }
    setReference(await render.measureReference(path));
  };

  const progress = Math.round(status.progress * 100);
  const done = status.phase === 'done' && !!status.destination;
  const error = render.error || status.error;

  // A new render is a new file, so the comparison starts over rather than
  // leaving a monitor gain that was matched to the last one.
  useEffect(() => {
    setSide('');
    setTrim(0);
    if (render.native) {
      void invoke('set_monitor_trim', { db: 0 }).catch(() => {});
      void invoke('set_monitor_direct', { direct: false }).catch(() => {});
    }
  }, [status.destination, render.native]);

  // The monitor gain is the page's, not the file's: leaving the page with it on
  // would quietly attenuate everything played after the comparison.
  useEffect(() => () => {
    if (render.native) {
      void invoke('set_monitor_trim', { db: 0 }).catch(() => {});
      void invoke('set_monitor_direct', { direct: false }).catch(() => {});
    }
  }, [render.native]);

  // Level-matched by pulling the louder side down, never by lifting the quieter
  // one: a comparison that added gain could clip the file it is defending.
  const compare = async (next: 'source' | 'master' | 'reference') => {
    if (!selected || !done || !render.native) return;
    const levels = {
      source: status.source.integratedLufs,
      master: status.output.integratedLufs,
      reference: reference?.integratedLufs ?? status.output.integratedLufs,
    };
    const other = next === 'source' ? levels.master : next === 'reference' ? levels.master : levels.source;
    const wanted = next === 'source' ? other - levels.source : next === 'reference' ? other - levels.reference : other - levels.master;
    const db = Number.isFinite(wanted) ? Math.max(-24, Math.min(0, wanted)) : 0;
    setCompareError('');
    setSide(next);
    setTrim(db);
    try {
      // The files themselves, not the rack twice: the master already contains it.
      await invoke('set_monitor_direct', { direct: true });
      await invoke('set_monitor_trim', { db });
      if (next === 'reference' && referenceTrack) {
        await player.play(referenceTrack);
      } else {
        await player.play(next === 'master'
          ? { ...selected, path: status.destination, title: `${selected.title} (mastered)` }
          : selected);
      }
    } catch (reason) {
      setSide('');
      setTrim(0);
      setCompareError(String(reason));
    }
  };

  const verdict = !spec
    ? 'Render to check a target'
    : !spec.judged
      ? `${minus(spec.peakOverDb + target.ceilingDb)} dBTP · no target`
      : spec.meets
        ? `Meets ${minus(target.lufs ?? 0)} LUFS, ${minus(target.ceilingDb)} dBTP`
        : [
            spec.loudnessHolds ? '' : `${Math.abs(spec.deltaLu).toFixed(1)} LU ${spec.deltaLu < 0 ? 'quiet' : 'loud'}`,
            spec.peakHolds ? '' : `${spec.peakOverDb.toFixed(1)} dB over the ceiling`,
          ].filter(Boolean).join(' · ');
  const verdictTone = spec && spec.judged ? (spec.meets ? 'ok' : 'off') : '';

  return <Page
    title="Master"
    state={tracks.length
      ? `48 kHz · stereo · ${active} of 6 engaged · ${depthLabel} ${containerLabel} · ${rateLabel(render.rate)} · ${target.lufs === null ? 'no target' : `${minus(target.lufs)} LUFS`}`
      : `48 kHz · stereo · ${active} of 6 engaged · nothing scanned`}
    actions={<div className="page-actions-row">
      <span className="instrument-tag" title="A master renders the rack, not the spatial layout">48 kHz · stereo</span>
    </div>}
  >
    <div className="page-grid master-grid">
      <article className="page-card master-source-card">
        <div className="panel-heading">
          <h2>Source</h2>
          <span className="panel-meta">{tracks.length ? `${tracks.length} TRACKS` : 'NOTHING SCANNED'}</span>
        </div>
        {!render.native && <p className="audio-warning">Exporting runs in the Windows app. Nothing on this page is simulated.</p>}
        {tracks.length === 0 && <div className="page-empty">
          <Disc3 size={26} strokeWidth={1} />
          <strong>No tracks yet</strong>
          <span>Scan a folder on the Library page first. Files are read in place; a master is a new file in its “Mastered” folder.</span>
        </div>}
        {tracks.length > 0 && <ScrollArea className="master-source-list" ariaLabel="Tracks to master">
          {tracks.map(track => <button
            key={track.path}
            type="button"
            className="master-track"
            aria-current={selectedPath === track.path ? 'true' : undefined}
            title={`Master ${track.title}`}
            onClick={() => choose(track.path)}
          >
            <span className="master-track-title">{track.title}</span>
            <span className="master-track-format">{track.format}</span>
            <span className="master-track-meta">{track.artist || '—'} · {track.album || '—'} · {formatBytes(track.bytes)}</span>
          </button>)}
        </ScrollArea>}
        <label className="master-reference-pick">
          <span>Reference</span>
          <select
            value={referencePath}
            onChange={event => void chooseReference(event.target.value)}
            aria-label="Reference track"
          >
            <option value="">None</option>
            {tracks.map(track => <option key={track.path} value={track.path}>{track.title}</option>)}
          </select>
        </label>
        {reference && <p className="master-reference-line mono">
          {`${lufs(reference.integratedLufs)} · ${dbtp(reference.truePeakDb)} · ${lu(reference.loudnessRange)}`}
        </p>}
        {/* An album is a group of the tracks above, so it sits with its source: the
            one button writes every one of them, in order, as one programme. */}
        {albums.length > 0 && <div className="master-albums">
          {albums.map(album => <div className="master-album" key={album.name}>
            <span className="master-album-name">{album.name}</span>
            <span className="master-album-count">{album.tracks.length} tracks</span>
            <button
              type="button"
              className="page-button ghost"
              disabled={status.running || !render.native}
              title={`Master every track of ${album.name}, in order, as one programme`}
              onClick={() => void render.startAlbum(album.tracks.map(track => track.path), depth)}
            >Master album</button>
          </div>)}
        </div>}
      </article>

      <article className="page-card master-export-card">
          <div className="panel-heading">
            <h2>Export</h2>
            <span className="panel-meta">
              {status.running
                ? 'EXPORTING'
                : albumDone
                  ? `${status.trackTotal} TRACKS · ${lufs(status.albumLufs)}`
                  : status.phase.toUpperCase()}
            </span>
          </div>
          <div className="master-actions">
            <Segmented
              compact
              label="Container"
              options={masterFormats}
              value={render.format}
              onChange={id => chooseFormat(id as MasterFormat)}
            />
            <Segmented
              compact
              label="Bit depth"
              options={depths}
              value={depth}
              onChange={id => setDepth(id as BitDepth)}
            />
            <Segmented
              compact
              label="Sample rate"
              options={masterRates}
              value={render.rate}
              onChange={id => render.setRate(id as SampleRate)}
            />
          </div>
          <div className="master-actions">
            <button
              type="button"
              className="page-button"
              disabled={!selected || status.running || !render.native}
              title={selected ? `Export ${selected.title}` : 'Choose a track first'}
              onClick={() => { if (selected) void render.start(selected.path, depth); }}
            >Export</button>
            {status.running && <button type="button" className="page-button ghost" onClick={() => void render.cancel()}><Square size={11} aria-hidden="true" />Cancel</button>}
            {done && <Segmented
              compact
              label="Compare"
              options={[
                { id: 'source', label: 'Source' },
                { id: 'master', label: 'Master' },
                ...(reference ? [{ id: 'reference', label: 'Reference' }] : []),
              ]}
              value={side}
              onChange={id => void compare(id as 'source' | 'master' | 'reference')}
            />}
            {done && side && <span
              className="master-trim"
              title="Both files are heard without the rack, with the louder side pulled down; your level is not changed"
            >{`${side === 'master' ? 'Master' : side === 'reference' ? 'Reference' : 'Source'} ${minus(trim)} dB`}</span>}
            {compareError && <span className="audio-error">{compareError}</span>}
          </div>
          {/* The progress line already names the file it wrote, so the path is only
              read before a render, when it is the promise of where it will go. */}
          {!status.running && !done && <p className="master-path mono" title={render.destination || 'No source chosen'}>
            {render.destination || 'Choose a track to see where its master goes.'}
          </p>}
          {(status.running || done) && <div className="master-progress">
            <div
              className="meter-track"
              role="progressbar"
              aria-label="Export progress"
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={progress}
              aria-valuetext={status.running ? `${progress} percent · ${status.frames} frames exported` : 'Export finished'}
            >
              <div className="meter-bar" aria-hidden="true"><div className="meter-fill" style={{ width: `${progress}%` }} /></div>
            </div>
            <span className="master-progress-line mono">
              <span>{status.running
                ? `${progress} % · ${status.trackTotal > 1 ? `${status.trackIndex} of ${status.trackTotal} · ${status.track} · ` : ''}${status.frames.toLocaleString()} frames`
                : `${status.seconds.toFixed(1)} s · ${formatBytes(status.bytes)} · ${status.depth} · ${status.rate || rateLabel(render.rate)}`}</span>
              <span>{status.destination.split(/[\\/]/).filter(Boolean).pop() ?? ''}</span>
            </span>
          </div>}
          {error && <p className="audio-error">{error}</p>}
      </article>

      <article className="page-card master-before-card">
          <div className="panel-heading">
            <h2>Before / after</h2>
            <span className="panel-meta" title="Loudness is gated BS.1770; true peak is oversampled four times. A dash means the render has not measured that figure yet.">{status.output.blocks ? `${status.output.blocks} BLOCKS` : 'WAITING'}</span>
          </div>
          <div className="master-spec">
            <Segmented
              label="Delivery target"
              options={masterTargets.map(entry => ({ id: entry.id, label: entry.label }))}
              value={targetId}
              onChange={id => setTargetId(id as TargetId)}
            />
            <span
              className={`master-verdict ${verdictTone}`}
              title={spec && spec.judged
                ? `Judged within ±${TARGET_TOLERANCE_LU} LU of the target, with the true peak at or under the ceiling`
                : 'A target judges the file the engine measured; it never normalises it'}
            >{verdict}</span>
            {spec && spec.judged && !spec.loudnessHolds && gain !== null && <span
              className="master-trim"
              title="A target judges the file the engine measured; it never normalises it"
            >{`→ ${minus(gain)} dB`}<span className="master-trim-target">{` lands on ${minus(target.lufs ?? 0)} LUFS`}</span></span>}
          </div>
          <table className="master-measurements">
            <thead>
              <tr><th scope="col">Figure</th><th scope="col">Source</th><th scope="col">Master</th></tr>
            </thead>
            <tbody>
              {measures.map(measure => <tr key={measure.name}>
                <th scope="row">{measure.name}</th>
                <td className="mono">{measure.ready(status.source) ? measure.format(status.source) : '—'}</td>
                <td className="mono">{measure.ready(status.output) ? measure.format(status.output) : '—'}</td>
              </tr>)}
            </tbody>
          </table>
          {reference && status.output.blocks > 0 && <p className="master-reference-compare mono">
            {`Reference ${minus(reference.integratedLufs)} LUFS · master ${minus(status.output.integratedLufs)} LUFS · ${minus(status.output.integratedLufs - reference.integratedLufs)} LU apart`}
          </p>}
      </article>

      <article className="page-card master-file-card">
          <div className="panel-heading">
            <h2>Whole file</h2>
            <span className="panel-meta">{render.scan.frames ? `${render.scan.columns} COLUMNS` : 'NOT READ'}</span>
          </div>
          <div className="master-actions">
            <button
              type="button"
              className="page-button"
              disabled={!selected || !render.native}
              title={selected ? `Read ${selected.title} whole` : 'Choose a track first'}
              onClick={() => { if (selected) void render.scanFile(selected.path); }}
            >Read the file</button>
            <button
              type="button"
              className="page-button ghost"
              disabled={!selected || !render.native}
              title="Render this file with every stage bypassed and compare it with the source, sample by sample"
              onClick={() => { if (selected) void render.nullTest(selected.path); }}
            >Bypass test</button>
            {render.nullReport && <span className={`master-null ${render.nullReport.differing === 0 ? 'ok' : 'off'}`}>
              {render.nullReport.differing === 0
                ? `Identical · ${render.nullReport.frames.toLocaleString()} frames`
                : `${render.nullReport.differing.toLocaleString()} samples differ · ${minus(render.nullReport.maxDifferenceDb)} dBFS`}
            </span>}
            {render.nullError && <span className="audio-error">{render.nullError}</span>}
            {render.scanError && <span className="audio-error">{render.scanError}</span>}
          </div>            {render.scan.frames > 0
            ? <FileScanView scan={render.scan} caption={`${selected?.title ?? 'The file'} · ${render.scan.seconds.toFixed(1)} s`} />
            : <p className="master-hint mono">Read the file to draw its envelope, loudness and spectrum.</p>}
      </article>
    </div>
  </Page>;
}
