import { FolderOpen, Pause, Play, RefreshCw, X } from 'lucide-react';
import { ScrollArea } from '../components/instrument/ScrollArea';
import { formatBytes, formatDate, Page } from './Page';
import type { useLibrary } from '../hooks';
import type { usePlayer } from '../usePlayer';

interface Props {
  library: ReturnType<typeof useLibrary>;
  player: ReturnType<typeof usePlayer>;
}

export function LibraryPage({ library, player }: Props) {
  const result = library.result;
  const current = player.status;
  return <Page
    title="Library"
    state={library.busy ? `scanning · ${library.path}` : result ? `${result.tracks.length} tracks · ${result.root}` : 'nothing scanned · files are read in place'}
    actions={<div className="page-actions-row">
      <button className="page-button" onClick={() => void library.browse()} disabled={library.busy}><FolderOpen size={13} />Choose folder</button>
      <button className="page-button ghost" onClick={() => void library.scan(library.path)} disabled={library.busy || !library.path}><RefreshCw size={13} />Rescan</button>
      {(result || library.path || library.error) && <button className="page-button ghost" onClick={library.clear} aria-label="Clear library"><X size={13} /></button>}
    </div>}
  >
    <div className="page-grid library-grid">
      <article className="page-card wide-card library-card">
        <div className="panel-heading">
          <h2>{result ? result.root.split(/[\\/]/).filter(Boolean).pop() || result.root : 'No folder scanned'}</h2>
          <span className="panel-meta">{library.busy ? 'SCANNING' : result ? `${result.tracks.length} TRACKS` : 'READY'}</span>
        </div>
        <div className="library-path">
          <input className="path-input" value={library.path} placeholder="D:\Music" aria-label="Library folder path" onChange={event => library.setPath(event.target.value)} onKeyDown={event => { if (event.key === 'Enter') void library.scan(library.path); }} />
          <button className="page-button" onClick={() => void library.scan(library.path)} disabled={library.busy || !library.path}>Scan</button>
        </div>
        {library.error && <p className="audio-error">{library.error}</p>}
        {player.error && <p className="audio-error">{player.error}</p>}
        {!result && !library.error && !library.busy && <div className="page-empty"><FolderOpen size={26} strokeWidth={1} /><strong>Nothing scanned yet</strong><span>Pick a folder, or paste a path and press Scan. MP3 and FLAC tags are read directly; other files fall back to their file name.</span><button className="page-button" onClick={() => void library.browse()} disabled={library.busy}><FolderOpen size={13} />Choose folder</button></div>}
        {result && result.tracks.length === 0 && <div className="page-empty"><strong>No audio files found</strong><span>{result.root} contains no .mp3, .flac, .wav, .m4a, .aac, .ogg, .opus, .aiff or .aif files.</span></div>}
        {result && result.tracks.length > 0 && <div className="track-table" role="table" aria-label="Scanned tracks">
          <div className="track-head" role="row"><span>Title</span><span>Artist</span><span>Album</span><span>Format</span><span>Size</span><span>Added</span></div>
          <ScrollArea className="track-body">
            {result.tracks.map(track => {
              const isCurrent = current.path === track.path;
              const isPlaying = isCurrent && current.playing;
              return <div
                key={track.path}
                className={`track-row playable ${isCurrent ? 'current' : ''}`}
                role="row"
                tabIndex={0}
                title={`Play ${track.title}`}
                aria-selected={isCurrent}
                onClick={() => void player.play(track)}
                onKeyDown={event => {
                  if (event.key === 'Enter' || event.key === ' ') {
                    event.preventDefault();
                    void player.play(track);
                  }
                }}
              >
                <span className="track-title"><i className="track-play" aria-hidden="true">{isPlaying ? <Pause size={9} /> : <Play size={9} />}</i>{track.title}</span>
                <span>{track.artist || '—'}</span>
                <span>{track.album || '—'}</span>
                <span>{track.format}</span>
                <span>{formatBytes(track.bytes)}</span>
                <span>{formatDate(track.modified)}</span>
              </div>;
            })}
          </ScrollArea>
        </div>}
        {result && <div className="fact-row">
          <span>{result.tracks.length} indexed</span>
          <span>{result.skipped} skipped</span>
          {result.truncated && <span>stopped at the 5000 track limit</span>}
        </div>}
      </article>

      <div className="page-stack">
        <article className="page-card">
          <div className="panel-heading"><h2>Playback</h2><span className="instrument-tag">{current.active ? (current.playing ? 'PLAYING' : 'PAUSED') : 'IDLE'}</span></div>
          <dl className="stat-list">
            <div><dt>Decoder</dt><dd title="Symphonia, decoded in the app">Symphonia, in app</dd></div>
            <div><dt>Output</dt><dd title={current.device || 'Windows default'}>{current.device || 'Windows default'}</dd></div>
            <div><dt>Stream</dt><dd title="48 kHz · 32-bit float · stereo">48 kHz · float · stereo</dd></div>
            <div><dt>Now</dt><dd title={current.active ? current.format || 'playing' : 'Nothing playing'}>{current.active ? `${current.format || 'playing'}` : '—'}</dd></div>
          </dl>
          <p className="panel-note">Decoded and resampled in the app, then rendered through the spatial renderer and the processors to the chosen output. AIFF and Opus have no decoder here and will report an error.</p>
        </article>
        <article className="page-card">
          <div className="panel-heading"><h2>What is read</h2></div>
          <dl className="stat-list">
            <div><dt>ID3v2 / ID3v1</dt><dd>MP3</dd></div>
            <div><dt>Vorbis comments</dt><dd>FLAC</dd></div>
            <div><dt>File name</dt><dd title="MP3 and FLAC tags are read; other files fall back to their name">Everything else</dd></div>
            <div><dt>Scanned</dt><dd>{result ? result.tracks.length : 0}</dd></div>
          </dl>
        </article>
      </div>
    </div>
  </Page>;
}
