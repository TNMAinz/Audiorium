import { Clock, Disc3, FolderOpen, Pause, Play } from 'lucide-react';
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react';
import type { useLibrary } from '../hooks';
import type { usePlayer } from '../usePlayer';
import type { AnalysisStatus } from '../useAnalysis';
import type { LibraryTrack, PlaybackStatus, Theme } from '../model';
import { useArtwork, type Artwork } from '../useArtwork';
import { useDurations } from '../useDurations';
import { formatClock } from './observatory';
import { circularDistance, wrapCursor, coverWindow, dragCursor, wheelSteps, dragVelocity, clampFling, flingStep, showsLibraryInvite, FLING_MIN, type CoverNode, type GlobeMotion } from './globeLibrary';
import { OrbitScene } from './OrbitScene';

interface Props {
  theme: Theme; reducedMotion: boolean; active: boolean;
  /** The window is in the tray: nothing here can be seen, so nothing here draws. */
  suspended: boolean;
  levels: React.RefObject<AnalysisStatus>;
  library: ReturnType<typeof useLibrary>;
  player: ReturnType<typeof usePlayer>;
  /** The chosen input is being heard, so the shelf must not ask for a library. */
  inputHeard: boolean;
  onLibrary: () => void;
}

export function AlbumGlobe({ theme, reducedMotion, active, suspended, levels, library, player, inputHeard, onLibrary }: Props) {
  const tracks = library.result?.tracks;
  const count = tracks?.length ?? 0;
  const [cursor, setCursor] = useState(0);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  // Where the clicked sleeve sat, so the full card can grow out of it rather than appear.
  const [origin, setOrigin] = useState<{ x: number; y: number; size: number } | null>(null);
  // A click on the dark behind the card asks it to fold away, rather than cutting it off.
  const [dismissing, setDismissing] = useState(false);
  const motion = useRef<GlobeMotion>({ cursor: 0, tilt: 0, dragging: false, gliding: false });
  const nodes = useRef(new Map<number, CoverNode>());
  const requestFrame = useRef<(() => void) | null>(null);
  const shell = useRef<HTMLDivElement | null>(null);
  const drag = useRef<{ id: number; x: number; y: number; cursor: number; index: number | null; moved: boolean } | null>(null);
  // The few pointer readings at release decide the flick: a thrown shelf keeps going
  // under friction, and settles on a song.
  const samples = useRef<{ x: number; t: number }[]>([]);
  const fling = useRef<{ raf: number } | null>(null);
  const indices = useMemo(() => coverWindow(count, cursor), [count, cursor]);
  // A rescan that returns the same songs must not pull the shelf back to the first one.
  const shelf = useMemo(() => tracks?.map(track => track.path).join('\n') ?? '', [tracks]);
  const selectedIndex = selectedPath ? tracks?.findIndex(track => track.path === selectedPath) ?? -1 : -1;
  const selected = selectedIndex >= 0 ? tracks![selectedIndex] : null;
  // The strip at the top names whatever is playing, so its art stays requested even after
  // the sleeve it belongs to has been browsed off the shelf.
  const paths = useMemo(() => {
    if (!active) return [];
    const list = indices.map(index => tracks![index].path);
    if (player.status.path && !list.includes(player.status.path)) list.push(player.status.path);
    return list;
  }, [active, indices, tracks, player.status.path]);
  const artwork = useArtwork(library.result?.root ?? '', paths);
  // Lengths arrive for the mounted sleeves only, so a big library pays for what is shown.
  const durations = useDurations(paths);

  useEffect(() => {
    const start = 0;
    setCursor(Math.round(start)); setSelectedPath(null); motion.current.cursor = start; motion.current.tilt = 0;
    requestFrame.current?.();
  }, [shelf]);

  const navigate = useCallback((to: number) => {
    motion.current.cursor = to;
    const rounded = wrapCursor(Math.round(to), count);
    setCursor(rounded);
    if (selectedIndex >= 0 && !coverWindow(count, rounded).includes(selectedIndex)) setSelectedPath(null);
    requestFrame.current?.();
  }, [count, selectedIndex]);

  const stopFling = useCallback(() => {
    if (fling.current) cancelAnimationFrame(fling.current.raf);
    fling.current = null;
    motion.current.gliding = false;
  }, []);
  useEffect(() => stopFling, [stopFling]);


  // A throw coasts under friction, then settles on the nearest song: the shelf keeps the
  // speed the hand gave it and runs out of it, rather than snapping the moment you let go.
  const coast = useCallback((velocity: number) => {
    stopFling();
    motion.current.gliding = true;
    let speed = clampFling(velocity);
    let last = performance.now();
    const step = () => {
      const now = performance.now();
      const next = flingStep(motion.current.cursor, speed, (now - last) / 1000);
      last = now;
      speed = next.velocity;
      navigate(next.cursor);
      if (!speed) {
        fling.current = null;
        motion.current.gliding = false;
        navigate(Math.round(motion.current.cursor));
        return;
      }
      fling.current = { raf: requestAnimationFrame(step) };
    };
    fling.current = { raf: requestAnimationFrame(step) };
  }, [navigate, stopFling]);

  // Wheel travel is accumulated so trackpads (many small deltas) browse as smoothly
  // as a mouse wheel (one 100 px notch), and both directions wrap the library. The
  // target stays unbounded: binding it back into 0..count would send the eased shelf
  // the long way round at the seam, which reads as the wheel running backwards.
  useEffect(() => {
    const node = shell.current;
    if (!node || !active || count < 2) return;
    let carry = 0;
    const onWheel = (event: WheelEvent) => {
      if ((event.target as HTMLElement | null)?.closest('[data-globe-controls]')) return;
      event.preventDefault();
      const raw = Math.abs(event.deltaX) > Math.abs(event.deltaY) ? event.deltaX : event.deltaY;
      const delta = event.deltaMode === 1 ? raw * 16 : event.deltaMode === 2 ? raw * 400 : raw;
      const next = wheelSteps(carry, delta);
      carry = next.carry;
      if (!next.steps) return;
      stopFling();
      if (selectedPath) setSelectedPath(null);
      navigate(Math.round(motion.current.cursor) + next.steps);
    };
    node.addEventListener('wheel', onWheel, { passive: false });
    return () => node.removeEventListener('wheel', onWheel);
  }, [active, count, navigate, selectedPath, stopFling]);

  const select = (index: number) => {
    const card = nodes.current.get(index)?.card;
    const box = card?.getBoundingClientRect();
    setOrigin(box ? { x: box.left + box.width / 2, y: box.top + box.height / 2, size: box.width } : null);
    setDismissing(false);
    setSelectedPath(tracks?.[index]?.path ?? null);
    requestFrame.current?.();
  };
  // Closing hands the focus back to the sleeve it came from, so the shelf is where the
  // keyboard left it and Escape never drops focus onto the page.
  const close = () => {
    const back = selectedIndex >= 0 ? nodes.current.get(selectedIndex)?.card : null;
    setSelectedPath(null); setOrigin(null); setDismissing(false);
    requestFrame.current?.();
    back?.focus({ preventScroll: true });
  };

  const startDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!active || drag.current || event.button !== 0 || (event.target as HTMLElement).closest('[data-globe-controls]')) return;
    stopFling();
    const cover = (event.target as HTMLElement).closest<HTMLElement>('[data-track-index]');
    drag.current = {
      id: event.pointerId, x: event.clientX, y: event.clientY, cursor: motion.current.cursor,
      index: cover ? Number(cover.dataset.trackIndex) : null, moved: false,
    };
    samples.current = [{ x: event.clientX, t: performance.now() }];
    // No manual pointer capture: the 3D canvas already captures this pointer, and a
    // second capture would be released as soon as the canvas took it back.
    motion.current.dragging = true;
    event.currentTarget.dataset.dragging = 'true';
  };
  const moveDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    const from = drag.current;
    if (!from || event.pointerId !== from.id) return;
    const dx = event.clientX - from.x, dy = event.clientY - from.y;
    if (Math.hypot(dx, dy) > 6) from.moved = true;
    if (!from.moved) return;
    const now = performance.now();
    samples.current = [...samples.current.filter(sample => now - sample.t < 80), { x: event.clientX, t: now }].slice(-6);
    if (selectedPath) setSelectedPath(null);
    navigate(dragCursor(from.cursor, dx, count));
    motion.current.tilt = Math.max(-0.2, Math.min(0.2, dy / 700));
    requestFrame.current?.();
  };
  const endDrag = (event: ReactPointerEvent<HTMLDivElement>, cancel = false) => {
    const from = drag.current;
    if (!from || event.pointerId !== from.id) return;
    drag.current = null; motion.current.dragging = false;
    event.currentTarget.dataset.dragging = 'false';
    if (!cancel && !from.moved && from.index !== null) select(from.index);
    const thrown = cancel || reducedMotion ? 0 : dragVelocity(samples.current);
    if (from.moved && Math.abs(thrown) >= FLING_MIN) coast(thrown);
    else if (from.moved) navigate(Math.round(motion.current.cursor));
  };

  return <div className="album-globe" ref={shell} data-active={active} data-cursor={cursor} data-total={count}
    data-mounted-covers={active ? indices.length : 0} data-art-cache={artwork.cached} data-art-pending={artwork.pending}
    onPointerDown={startDrag} onPointerMove={moveDrag} onPointerUp={event => endDrag(event)}
    onPointerCancel={event => endDrag(event, true)}
  >
    <div className="cosmos-layer" aria-hidden="true"><OrbitScene theme={theme} reducedMotion={reducedMotion} active={active} suspended={suspended} playing={player.status.playing}
      levels={levels} motion={motion} nodes={nodes} count={count} selected={selectedIndex >= 0 ? selectedIndex : null} requestFrame={requestFrame} /></div>
    {active && <>
      <div className="globe-vignette" aria-hidden="true" />
      <p id="globe-instructions" className="sr-only">Drag, scroll the wheel, or use the arrow keys to browse the library. Press Enter to select a song.</p>
      <div className="globe-covers" role="group" aria-label="Songs on the library globe" aria-describedby="globe-instructions"
        tabIndex={count ? 0 : -1}
        onKeyDown={event => {
          if ((event.target as HTMLElement).closest('[data-globe-controls]')) return;
          const steps: Record<string, number> = { ArrowRight: 1, ArrowLeft: -1, PageDown: 12, PageUp: -12 };
          if (event.key in steps) {
            event.preventDefault();
            stopFling();
            navigate(Math.round(motion.current.cursor) + steps[event.key]);
          }
          if (event.key === 'Home' || event.key === 'End') {
            event.preventDefault();
            stopFling();
            navigate(motion.current.cursor + circularDistance(event.key === 'Home' ? 0 : count - 1, motion.current.cursor, count));
          }
          if (event.key === 'Escape') setSelectedPath(null);
        }}>
        {indices.map(index => {
          const track = tracks![index];
          const cover = artwork.covers.get(track.path);
          const isSelected = selectedPath === track.path;
          const isPlaying = player.status.active && player.status.path === track.path;
          return <div key={track.path} className="globe-cover-anchor">
            <button className={`globe-cover tone-${index % 3} ${isSelected ? 'selected' : ''}`} data-track-index={index}
              data-art-state={cover?.state ?? 'loading'} aria-label={`Select ${track.title}${track.artist ? ` by ${track.artist}` : ''}`}
              aria-pressed={isSelected} title={`${track.title}${track.artist ? ` — ${track.artist}` : ''}`}
              ref={card => {
                if (!card) { nodes.current.delete(index); return; }
                nodes.current.set(index, { card }); requestFrame.current?.();
              }}
              onClick={event => { if (event.detail === 0) select(index); }}
            >
              <span className="globe-art-fallback"><Disc3 strokeWidth={0.7} /><small>{track.title.slice(0, 22)}</small></span>
              {cover?.url && <img src={cover.url} alt="" draggable={false} onError={event => {
                event.currentTarget.hidden = true;
                event.currentTarget.parentElement?.setAttribute('data-art-state', 'error');
              }} />}
              <span className="globe-cover-orbit" aria-hidden="true"><i /><i /><b /><b /></span>
              {isPlaying && <span className="globe-cover-playing">{player.status.playing ? 'PLAYING' : 'PAUSED'}</span>}
            </button>
          </div>;
        })}
      </div>
      {showsLibraryInvite(count, library.busy, library.error, inputHeard) && <div className="globe-empty" data-globe-controls role="status">
        <Disc3 size={26} strokeWidth={1} />
        <strong>{library.busy ? 'Mapping your music' : library.error ? 'Library unavailable' : library.result ? 'No songs in this folder' : 'Bring your music into orbit'}</strong>
        <p>{library.error || (library.busy ? 'Covers will appear as the local library opens.' : 'Choose a music folder. Embedded album covers appear here as you explore.')}</p>
        {!library.busy && <button className="page-button" onClick={onLibrary}><FolderOpen size={13} />Open Library</button>}
      </div>}
      {selected && <>
        <div className="globe-card-scrim" data-globe-controls aria-hidden="true" onClick={() => setDismissing(true)} />
        <SongCard key={selected.path} origin={origin} track={selected}
          cover={artwork.covers.get(selected.path)} animate={!reducedMotion} dismiss={dismissing}
          playing={player.status.active && player.status.path === selected.path && player.status.playing}
          duration={durations.has(selected.path) ? formatClock(durations.get(selected.path)!) : null}
          onPlay={() => { setDismissing(true); void player.play(selected); }} busy={player.busy || !player.native} onClose={close} />
      </>}
      {/* Keyed by the song: every new track mounts the strip fresh, so it drops in as the card
          that started it folds away, and again when playback moves on by itself. */}
      {player.status.active && <NowPlaying key={player.status.path} status={player.status}
        cover={artwork.covers.get(player.status.path)} animate={!reducedMotion} onToggle={() => void player.toggle()} />}
      {player.error && <p className="globe-error" role="alert" data-globe-controls>{player.error}</p>}
    </>}
  </div>;
}

/**
 * The card a sleeve opens into. The print takes the top of it and the foot names the piece:
 * title, artist, running time. It is the same card as the sleeve, grown: it starts at the
 * sleeve's box and size and eases out to full size, so the shelf itself never jumps, and
 * closing folds it back into the sleeve it came from.
 */
function SongCard({ origin, track, cover, playing, duration, busy, animate, dismiss, onPlay, onClose }: {
  origin: { x: number; y: number; size: number } | null;
  track: LibraryTrack; cover: Artwork | undefined; playing: boolean;
  duration: string | null; busy: boolean; animate: boolean; dismiss: boolean;
  onPlay: () => void; onClose: () => void;
}) {
  const card = useRef<HTMLElement | null>(null);
  const closing = useRef(false);

  // Where the card sits when folded back into its sleeve, in the card's own coordinates:
  // the sleeve's centre, at the sleeve's share of the card's width.
  const folded = (box: DOMRect) => origin
    ? `translate(calc(-50% + ${origin.x - box.left - box.width / 2}px), calc(-50% + ${origin.y - box.top - box.height / 2}px)) scale(${Math.max(0.22, origin.size / box.width)})`
    : 'translate(-50%, -50%) scale(.9)';

  // An explicit keyframe animation, not a CSS transition: the first paint of a freshly
  // mounted card has no previous transform to ease from, so the card would snap open.
  // Focus is taken a frame late: a keyboard activation is still finishing on the sleeve
  // button when the card mounts, and that button takes the focus back.
  useEffect(() => {
    const frame = requestAnimationFrame(() => card.current?.focus({ preventScroll: true }));
    return () => cancelAnimationFrame(frame);
  }, []);

  useLayoutEffect(() => {
    const node = card.current;
    if (!node) return;
    if (!animate) return;
    // Measure the card's untouched layout box: a fold already in flight (a re-run of this
    // effect) would otherwise be read as the starting point and cancel the growth.
    node.getAnimations().forEach(animation => animation.cancel());
    const box = node.getBoundingClientRect();
    if (!box.width) return;
    node.animate(
      [{ transform: folded(box), opacity: 0 }, { transform: 'translate(-50%, -50%)', opacity: 1 }],
      { duration: 340, easing: 'cubic-bezier(.2, .8, .2, 1)', fill: 'forwards' },
    );
    // The origin is read once, at mount: the card grows from where it was clicked.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [animate]);

  const close = () => {
    if (closing.current) return;
    closing.current = true;
    const node = card.current;
    const box = node?.getBoundingClientRect();
    if (!node || !animate || !box?.width) { onClose(); return; }
    // Held on its last frame until it is unmounted: an animation left on `fill: auto` drops
    // back to the card's resting transform the instant it ends, which flashes the full card
    // back for a frame before React takes it away.
    const shrink = node.animate(
      [{ transform: 'translate(-50%, -50%)', opacity: 1 }, { transform: folded(box), opacity: 0 }],
      { duration: 240, easing: 'cubic-bezier(.4, 0, .9, .4)', fill: 'forwards' },
    );
    shrink.finished.then(() => onClose(), () => onClose());
  };

  // The dark behind the card is a close button in disguise: pressing it folds the card away
  // instead of cutting it off mid-air.
  useEffect(() => { if (dismiss) close(); }, [dismiss]); // eslint-disable-line react-hooks/exhaustive-deps

  // Escape closes from wherever the focus sits — a click inside the card can leave it on the
  // page body, and the key must still work.
  const escape = useRef(close);
  escape.current = close;
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.stopPropagation();
      escape.current();
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, []);

  return <section className="globe-card" ref={node => { card.current = node; }} role="dialog" aria-label={`${track.title}${track.artist ? ` by ${track.artist}` : ''}`}
    data-globe-controls tabIndex={-1}
    onPointerDown={event => event.stopPropagation()}
    onWheel={event => event.stopPropagation()}
  >
    <span className="globe-card-orbit" aria-hidden="true"><i /><i /><i /></span>
    <span className="globe-card-moon" aria-hidden="true" />
    <span className="globe-card-moon far" aria-hidden="true" />
    <div className="globe-card-print">
      <span className="globe-art-fallback"><Disc3 strokeWidth={0.6} /><small>{track.title.slice(0, 22)}</small></span>
      {cover?.url && <img src={cover.url} alt="" draggable={false} onError={event => { event.currentTarget.hidden = true; }} />}
      {playing && <span className="globe-cover-playing">PLAYING</span>}
    </div>
    <div className="globe-card-body">
      <h3 title={track.title}>{track.title}</h3>
      <p title={track.artist || undefined}>{track.artist || 'Unknown artist'}</p>
      <span className="globe-card-rule" aria-hidden="true"><i /></span>
      <div className="globe-card-foot">
        <button className="globe-play" disabled={busy} onClick={onPlay} aria-label={`Play ${track.title}`}>
          <Play size={16} fill="currentColor" aria-hidden="true" />
        </button>
        <span className="globe-card-time"><Clock size={11} strokeWidth={1.6} aria-hidden="true" />{duration ?? '--:--'}</span>
      </div>
      <small className="globe-card-note">{cover?.state === 'missing' ? 'No embedded cover' : cover?.state === 'error' ? 'Cover unavailable' : track.album || track.format}</small>
    </div>
  </section>;
}

/**
 * What is playing, at the top of the scene. It arrives after a song starts — pressing Play on
 * the card hands the song up here while the card folds away — and then simply stays, keeping its
 * own position rather than the card's.
 */
function NowPlaying({ status, cover, animate, onToggle }: {
  status: PlaybackStatus; cover: Artwork | undefined; animate: boolean; onToggle: () => void;
}) {
  const second = [status.artist || 'Unknown artist', status.album].filter(Boolean).join(' · ');
  const strip = useRef<HTMLElement | null>(null);
  const state = useRef<HTMLSpanElement | null>(null);
  // The equaliser runs as an explicit animation too, for the same reason as the entrance: it
  // should follow the app's own motion setting, not be flattened by the system one.
  useEffect(() => {
    const node = state.current;
    if (!node || !animate || !status.playing) return;
    const runs = [...node.querySelectorAll<HTMLElement>('i')].map((mark, at) => mark.animate(
      [{ transform: 'scaleY(.3)' }, { transform: 'scaleY(1)' }],
      { duration: 880, delay: -at * 190, iterations: Infinity, direction: 'alternate', easing: 'ease-in-out' },
    ));
    return () => { for (const run of runs) run.cancel(); };
  }, [animate, status.playing]);
  // The entrance is an explicit animation, like the card's, rather than a CSS one: it has to
  // wait a beat for the card to fold, and a CSS animation is flattened to nothing when the
  // system asks for reduced motion.
  useLayoutEffect(() => {
    const node = strip.current;
    if (!node || !animate) return;
    node.animate(
      [{ opacity: 0, transform: 'translate(-50%, -22px) scale(.95)' }, { opacity: 1, transform: 'translate(-50%, 0) scale(1)' }],
      { duration: 420, delay: 160, easing: 'cubic-bezier(.2, .8, .2, 1)', fill: 'backwards' },
    );
  }, [animate]);
  return <div className="globe-now-shell" ref={node => { strip.current = node; }}>
    {/* Two long orbits pass behind the bar and the planets hang off its edge, so the strip is an
        object in the same space as the shelf rather than a panel laid over it. */}
    <span className="globe-now-rings" aria-hidden="true"><i /><i /><i /></span>
    <section className="globe-now" data-globe-controls data-playing={status.playing}
      aria-label={`Now playing: ${status.title}${status.artist ? ` by ${status.artist}` : ''}`}>
      <span className="globe-now-art">
        <Disc3 strokeWidth={0.9} aria-hidden="true" />
        {cover?.url && <img src={cover.url} alt="" draggable={false} onError={event => { event.currentTarget.hidden = true; }} />}
      </span>
      <span className="globe-now-copy">
        <span className="globe-now-state" ref={node => { state.current = node; }} aria-hidden="true"><i /><i /><i /><i /><em>{status.playing ? 'PLAYING' : 'PAUSED'}</em></span>
        <strong title={status.title}>{status.title}</strong>
        <small title={second}>{second}</small>
      </span>
      <button className="globe-now-toggle" onClick={onToggle} aria-label={status.playing ? 'Pause' : 'Resume'}>
        {status.playing ? <Pause size={13} fill="currentColor" aria-hidden="true" /> : <Play size={13} fill="currentColor" aria-hidden="true" />}
      </button>
    </section>
    <span className="globe-now-moons" aria-hidden="true"><b /><b /><b /></span>
  </div>;
}
