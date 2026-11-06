import { Activity } from 'lucide-react';
import { type AnalysisStatus, type useAnalysis } from '../useAnalysis';
import { PeakMeter, SpectrumLandscape, StereoField } from '../components/SignalLandscape';
import { Page } from './Page';

const FLOOR = -60;
const toDb = (value: number) => (value > 0 ? Math.max(FLOOR, 20 * Math.log10(value)) : FLOOR);
const formatDb = (value: number) => (value <= FLOOR ? '—' : `${value.toFixed(1).replace('-', '−')} dBFS`);

function Trend({ history }: { history: { rms: number; peak: number }[] }) {
  const width = 300;
  const height = 64;
  const line = (key: 'rms' | 'peak') => history
    .map((frame, index) => `${(index / Math.max(1, history.length - 1)) * width},${height - 2 - Math.min(1, frame[key]) * (height - 6)}`)
    .join(' ');
  return <svg className="trend-svg" viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" role="img" aria-label={history.length ? 'Measured peak and RMS over the last few seconds' : 'No trend: nothing is being measured'}>
    {[0, 0.25, 0.5, 0.75, 1].map(share => <line key={share} x1="0" x2={width} y1={Math.min(height - 0.5, Math.max(0.5, height * share))} y2={Math.min(height - 0.5, Math.max(0.5, height * share))} className="trend-rule" />)}
    {history.length > 1 && <>
      <polyline points={line('peak')} fill="none" stroke="#ffc08a" strokeWidth={1.2} opacity={0.75} />
      <polyline points={line('rms')} fill="none" stroke="#8ee7ff" strokeWidth={1.4} />
    </>}
  </svg>;
}

function Band({ label, value, status }: { label: string; value: number; status: AnalysisStatus }) {
  // A pause holds the last reading, so the bands stay where the output left them.
  const shown = status.playing || status.paused;
  const db = toDb(value);
  const fill = shown ? `${Math.min(1, Math.max(0, (db - FLOOR) / -FLOOR)) * 100}%` : '0%';
  return <div className="band-row">
    <span>{label}</span>
    <div className="band-track"><div className="band-fill" style={{ width: fill }} /></div>
    <strong>{shown ? formatDb(db) : '—'}</strong>
  </div>;
}

export function AnalysisPage({ audio, history }: { audio: ReturnType<typeof useAnalysis>; history: { rms: number; peak: number }[] }) {
  const status = audio.status;
  const paused = status.paused && !status.playing;
  const peakBand = status.spectrum.length ? status.spectrum.indexOf(Math.max(...status.spectrum)) : -1;
  const bandHz = peakBand >= 0 ? Math.round(30 * Math.pow(16_000 / 30, (peakBand + 0.5) / 56)) : 0;
  const input = status.source === 'input';
  return <Page
    title="Analysis"
    state={`${status.format} · ${input ? 'input capture' : 'playback'} · 2048-pt FFT · ${status.playing ? 'live' : paused ? 'paused' : 'idle'}`}
    actions={<span className="instrument-tag">{status.playing ? 'LIVE' : paused ? 'PAUSED' : 'IDLE'}</span>}
  >
    <div className="page-grid analysis-grid">
      <article className="page-card wide-card">
        <div className="panel-heading"><h2>Spectrum</h2><span className="panel-meta">{peakBand >= 0 ? `PEAK BAND ≈ ${bandHz} Hz` : 'FLAT'}</span></div>
        <div className="analysis-chart"><SpectrumLandscape status={status} surface={audio.surface.current} /></div>
        <div className="fact-row">
          <span>FFT 2048 · Hann</span>
          <span>56 bands</span>
          <span>30 Hz – 16 kHz</span>
          <span>−84 dBFS floor</span>
        </div>
      </article>

      <div className="page-stack">
        <article className="page-card">
          <div className="panel-heading"><h2>Level</h2><Activity size={14} /></div>
          <PeakMeter status={status} />
        </article>
        <article className="page-card">
          <div className="panel-heading"><h2>Energy bands</h2><span className="panel-meta">{input ? 'INPUT RMS' : 'OUTPUT RMS'}</span></div>
          <div className="band-list">
            <Band label="Low" value={status.low} status={status} />
            <Band label="Mid" value={status.mid} status={status} />
            <Band label="High" value={status.high} status={status} />
          </div>
        </article>
        <article className="page-card stereo-card">
          <div className="panel-heading"><h2>Stereo image</h2><span className="panel-meta">GONIOMETER</span></div>
          <StereoField status={status} />
        </article>
      </div>

      <article className="page-card wide-card">          <div className="panel-heading"><h2>Level history</h2><span className="panel-meta">{history.length ? `${((history.length * audio.pollMs) / 1000).toFixed(1)} s` : 'STOPPED'}</span></div>
        <div className="trend-wrap">
          <div className="trend-db" aria-hidden="true"><span>0</span><span>−20</span><span>−40</span><span>−60</span></div>
          <Trend history={history} />
        </div>
        <div className="fact-row"><span><i className="trend-key rms" />RMS</span><span><i className="trend-key peak" />Peak</span><span>{input ? 'Measured from the chosen input, in shared mode' : 'Measured after the chain, from the output'}</span></div>
      </article>
    </div>
  </Page>;
}
