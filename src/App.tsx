import { useEffect, useMemo, useState } from 'react';
import { Activity, AudioWaveform, Boxes, Disc3, Headphones, Home, SlidersHorizontal } from 'lucide-react';
import { AlbumGlobe } from './components/AlbumGlobe';
import { AudioInspector } from './components/AudioInspector';
import { PlayerBar } from './components/PlayerBar';
import { ProcessorRack } from './components/ProcessorRack';
import { WindowChrome } from './components/WindowChrome';
import { type Theme, type Workspace } from './model';
import { useAnalysis } from './useAnalysis';
import { useInputHeard, useLibrary, useSpatialLayout, useWindowAway } from './hooks';
import { usePlayer } from './usePlayer';
import { useArtwork } from './useArtwork';
import { useChain } from './useChain';
import { localMotionStorage, readReducedMotion, writeReducedMotion } from './motion';
import { localThemeStorage, readTheme, writeTheme } from './themeMemory';
import { SpatialFieldPage } from './pages/SpatialFieldPage';
import { AnalysisPage } from './pages/AnalysisPage';
import { SignalChainPage } from './pages/SignalChainPage';
import { EqualiserPage } from './pages/EqualiserPage';
import { DevicesPage } from './pages/DevicesPage';
import { LibraryPage } from './pages/LibraryPage';

const nav: { name: Workspace; label: string; icon: typeof Home; group: 'Explore' | 'System' }[] = [
  { name: 'Orbit', label: 'Observatory', icon: Home, group: 'Explore' },
  { name: 'Spatial', label: 'Spatial field', icon: Boxes, group: 'Explore' },
  { name: 'Spectrum', label: 'Analysis', icon: Activity, group: 'Explore' },
  { name: 'Flow', label: 'Signal chain', icon: SlidersHorizontal, group: 'Explore' },
  { name: 'Eq', label: 'Equaliser', icon: AudioWaveform, group: 'Explore' },
  { name: 'Library', label: 'Library', icon: Disc3, group: 'System' },
  { name: 'Devices', label: 'Devices', icon: Headphones, group: 'System' },
];
const groups = ['Explore', 'System'] as const;

export function App() {
  const [workspace, setWorkspace] = useState<Workspace>('Orbit');
  // The palette is a choice the user made, so it is read back on launch rather than reset.
  const [theme, setTheme] = useState<Theme>(() => readTheme(localThemeStorage()));
  useEffect(() => { writeTheme(localThemeStorage(), theme); }, [theme]);
  const [setupRequest, setSetupRequest] = useState(0);
  const [rackRequest, setRackRequest] = useState(0);
  // Quiet by default: the scene holds still until motion is asked for in settings, and the answer is remembered.
  const [reduceMotion, setReduceMotion] = useState(() => readReducedMotion(localMotionStorage()));
  useEffect(() => { writeReducedMotion(localMotionStorage(), reduceMotion); }, [reduceMotion]);
  // Playback is the app's only source: the analysis hook reports what the player
  // wrote to the output, and the same hook owns the output device and the field.
  // A window in the tray is not being watched: the meters stop asking the engine and
  // the scene stops drawing, while playback — which is the engine's, not the page's —
  // carries on.
  const away = useWindowAway();
  // Reduced motion asks the engine less often, so every live page draws a fifth of the
  // curves for the same readings; the tray and the toggle both quieten the same poll.
  const analysis = useAnalysis(away, reduceMotion);
  const { layout } = useSpatialLayout(analysis.spatial);
  // An input being heard is a reason not to invite a music folder; silence is not.
  const inputHeard = useInputHeard(analysis.capture.active, analysis.status.rms);
  const library = useLibrary();
  const player = usePlayer(library.result?.tracks ?? [], analysis.output);
  // The deck shows the print of what is playing, so it asks for that one cover only.
  const deckPaths = useMemo(() => (player.status.active ? [player.status.path] : []), [player.status.active, player.status.path]);
  const deckArtwork = useArtwork(library.result?.root ?? '', deckPaths);
  const chain = useChain(away);

  return <div className="app" data-theme={theme.toLowerCase()} data-workspace={workspace}>
    <AlbumGlobe theme={theme} reducedMotion={reduceMotion} active={workspace === 'Orbit'} suspended={away} levels={analysis.levels} library={library} player={player} inputHeard={inputHeard} onLibrary={() => setWorkspace('Library')} />
    <WindowChrome />
    <aside className="sidebar glass-panel" aria-label="Primary navigation">
      <div className="brand"><span className="brand-mark" aria-hidden="true"><i /></span><strong>AUDIORIUM</strong></div>
      {groups.map(group => <div className="nav-group" key={group}>
        <span className="nav-label">{group}</span>
        <nav aria-label={`${group} destinations`}>{nav.filter(item => item.group === group).map(({ name, label, icon: Icon }) => <button key={name} className={`nav-item ${workspace === name ? 'active' : ''}`} onClick={() => setWorkspace(name)} aria-label={label} aria-current={workspace === name ? 'page' : undefined} title={label}><Icon size={17} strokeWidth={1.5} /><span>{label}</span></button>)}</nav>
      </div>)}
    </aside>
    <main className={`main-content ${workspace === 'Orbit' ? '' : 'workspace-surface'}`} aria-labelledby="workspace-title">
      {workspace === 'Orbit' ? <section className="scene-panel" aria-label="Observatory">
        <h1 id="workspace-title" className="sr-only">Observatory</h1>

      </section> : <div className="workspace-host">
        {workspace === 'Spatial' && <SpatialFieldPage audio={analysis} layout={layout} spatialPath={player.status.spatial} />}
        {workspace === 'Spectrum' && <AnalysisPage audio={analysis} history={analysis.history} />}
        {workspace === 'Flow' && <SignalChainPage audio={analysis} chain={chain} track={player.status.active ? player.status.title : ''} volume={player.status.volume} onVolume={volume => void player.setVolume(volume)} onOpenRack={() => setRackRequest(count => count + 1)} onOpenEq={() => setWorkspace('Eq')} />}
        {workspace === 'Eq' && <EqualiserPage chain={chain} />}
        {workspace === 'Library' && <LibraryPage library={library} player={player} />}
        {workspace === 'Devices' && <DevicesPage audio={analysis} error={player.error} />}
      </div>}
    </main>
    <AudioInspector analysis={analysis} chain={chain} theme={theme} onTheme={setTheme} reducedMotion={reduceMotion} onReducedMotion={() => setReduceMotion(!reduceMotion)} setupRequest={setupRequest} onOpenRack={() => setRackRequest(count => count + 1)} volume={player.status.volume} onVolume={volume => void player.setVolume(volume)} track={player.status.active ? player.status.title : ''} spatialPath={player.status.spatial} />
    <ProcessorRack chain={chain} open={rackRequest} />
    <PlayerBar playback={player.status} cover={deckArtwork.covers.get(player.status.path)} hasQueue={player.hasQueue} busy={player.busy} shuffle={player.shuffle} repeat={player.repeat} onToggle={() => void player.toggle()} onNext={() => void player.next()} onPrevious={() => void player.previous()} onShuffle={player.toggleShuffle} onRepeat={player.cycleRepeat} onSeek={seconds => void player.seek(seconds)} onStop={() => void player.stop()} onControls={() => setSetupRequest(count => count + 1)} />
  </div>;
}
