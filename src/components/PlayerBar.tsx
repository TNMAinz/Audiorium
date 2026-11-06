import { AudioLines, Pause, Play, Radio, Repeat, Repeat1, Shuffle, SkipBack, SkipForward, Square } from 'lucide-react';
import type { CSSProperties } from 'react';
import type { PlaybackStatus } from '../model';
import type { Artwork } from '../useArtwork';
import type { RepeatMode } from '../usePlayer';

export function clock(seconds: number) {
  if (!Number.isFinite(seconds) || seconds <= 0) return '0:00';
  const minutes = Math.floor(seconds / 60);
  const rest = Math.floor(seconds % 60);
  return `${minutes}:${rest.toString().padStart(2, '0')}`;
}

interface Props {
  playback: PlaybackStatus;
  /** The playing track's embedded cover, when one has been read. */
  cover?: Artwork;
  hasQueue: boolean;
  busy: boolean;
  shuffle: boolean;
  repeat: RepeatMode;
  onToggle: () => void;
  onNext: () => void;
  onPrevious: () => void;
  onShuffle: () => void;
  onRepeat: () => void;
  onSeek: (seconds: number) => void;
  /** Closes the output stream and forgets the track; pause keeps it open. */
  onStop: () => void;
  onControls: () => void;
}

export function PlayerBar({ playback, cover, hasQueue, busy, shuffle, repeat, onToggle, onNext, onPrevious, onShuffle, onRepeat, onSeek, onStop, onControls }: Props) {
  const playing = playback.active && playback.playing;
  const state = playing ? 'PLAYING' : playback.active ? 'PAUSED' : 'STANDBY';
  const repeatLabel = repeat === 'one' ? 'Repeat one track' : repeat === 'all' ? 'Repeat queue' : 'Repeat off';
  // The rail is painted by CSS, so the played share travels as a single custom property.
  const fill = playback.duration > 0 ? Math.min(100, Math.max(0, (playback.position / playback.duration) * 100)) : 0;
  const title = playback.active ? playback.title : 'No track loaded';
  const subtitle = playback.active
    ? [playback.artist, playback.album].filter(Boolean).join(' · ') || playback.format
    : 'Select a track in Library';
  // Three stations: what is playing, the transport with its rail, and the way into the settings.
  return <footer className="player glass-panel">
    <div className="track">
      <div className={`mini-artwork ${playing ? 'playing' : ''}`}>
        <AudioLines size={20} strokeWidth={1.1} aria-hidden="true" />
        {cover?.url && <img src={cover.url} alt="" draggable={false} onError={event => { event.currentTarget.hidden = true; }} />}
      </div>
      <div className="track-text"><strong title={title}>{title}</strong><span title={subtitle}>{subtitle}</span></div>
    </div>
    <div className="player-center">
      <div className="transport" role="group" aria-label="Playback controls">
        <button className={`transport-button toggle ${shuffle ? 'on' : ''}`} onClick={onShuffle} aria-pressed={shuffle} aria-label={shuffle ? 'Shuffle on' : 'Shuffle off'} title={shuffle ? 'Shuffle is on' : 'Shuffle the queue'}><Shuffle size={15} /></button>
        <button className="transport-button" onClick={onPrevious} disabled={!hasQueue} aria-label="Previous track" title="Previous track"><SkipBack size={16} fill="currentColor" strokeWidth={1.4} /></button>
        <button className="transport-button primary" onClick={onToggle} disabled={busy || (!hasQueue && !playback.active)} aria-label={playing ? 'Pause playback' : 'Play playback'} title={playing ? 'Pause' : 'Play'}>{playing ? <Pause size={15} fill="currentColor" strokeWidth={0} /> : <Play size={15} fill="currentColor" strokeWidth={0} />}</button>
        <button className="transport-button" onClick={onNext} disabled={!hasQueue} aria-label="Next track" title="Next track"><SkipForward size={16} fill="currentColor" strokeWidth={1.4} /></button>
        <button className={`transport-button toggle ${repeat === 'off' ? '' : 'on'}`} onClick={onRepeat} aria-pressed={repeat !== 'off'} aria-label={repeatLabel} title={repeat === 'off' ? 'Repeat off' : repeat === 'all' ? 'Repeat the queue' : 'Repeat this track'}>{repeat === 'one' ? <Repeat1 size={15} /> : <Repeat size={15} />}</button>
      </div>
      <div className="player-progress">
        <span className="time">{clock(playback.active ? playback.position : 0)}</span>
        <input
          className="progress-range"
          style={{ '--fill': `${fill}%` } as CSSProperties}
          type="range"
          min={0}
          max={Math.max(0.1, playback.duration)}
          step={0.25}
          value={Math.min(playback.position, playback.duration || 0)}
          aria-label="Seek"
          aria-valuetext={`${clock(playback.position)} of ${clock(playback.duration)}`}
          onChange={event => onSeek(Number(event.target.value))}
          disabled={!playback.active || playback.duration <= 0}
        />
        <span className="time">{playback.duration > 0 ? clock(playback.duration) : '—'}</span>
      </div>
    </div>
    <div className="player-utility">
      <span className={`player-state-label ${playing ? 'live' : ''}`}><i />{state}</span>
      <button className="transport-button" onClick={onStop} disabled={!playback.active} aria-label="Stop playback" title="Stop and close the output"><Square size={11} fill="currentColor" strokeWidth={0} /></button>
      <button className="audio-controls-shortcut" onClick={onControls} aria-label="Audio controls" title="Audio controls"><Radio size={16} /></button>
    </div>
  </footer>;
}
