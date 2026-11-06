import { useEffect, useRef, useState } from 'react';
import { clamp, dbTicks, spacedLabels } from './scales';

const GR_TICKS = [0, -3, -6, -12, -24];
const LEVEL_TICKS = [-48, -36, -24, -12, -6, -3, 0];
const db = (value: number) => value.toFixed(1).replace('-', '−');

/** A scale drawn under a bar: every tick marked, only the readable ones labelled. */
function Scale({ ticks }: { ticks: { db: number; fraction: number }[] }) {
  const labelled = new Set(spacedLabels(ticks).map(tick => tick.db));
  return <div className="meter-ticks" aria-hidden="true">
    {ticks.map(tick => <span
      key={tick.db}
      className={labelled.has(tick.db) ? '' : 'unlabelled'}
      style={{
        left: `${tick.fraction * 100}%`,
        transform: tick.fraction <= 0.001 ? 'none' : tick.fraction >= 0.999 ? 'translateX(-100%)' : 'translateX(-50%)',
      }}
    >{tick.db}</span>)}
  </div>;
}

/** Horizontal gain-reduction meter, 0 dB at the right, −24 dB at the left. */
export function GainReductionMeter({ reductionDb, enabled, idleLabel = 'limiter bypassed' }: { reductionDb: number; enabled: boolean; idleLabel?: string }) {
  const value = enabled ? clamp(-reductionDb, 0, 24) : 0;
  const [held, setHeld] = useState(0);
  const timer = useRef<number | null>(null);

  useEffect(() => {
    if (!enabled) { setHeld(0); return; }
    setHeld(current => Math.max(current, clamp(-reductionDb, 0, 24)));
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setHeld(0), 1500);
    return () => { if (timer.current) window.clearTimeout(timer.current); };
  }, [reductionDb, enabled]);

  // 0 dB at the left, where the needle rests, and reduction growing to the right.
  const ticks = GR_TICKS.map(db => ({ db, fraction: Math.abs(db) / 24 }));
  const at = (amount: number) => ((clamp(amount, 0, 24) / 24) * 100);
  return <div className={`gr-meter ${enabled ? '' : 'idle'}`}>
    <div className="meter-track" role="meter" aria-label="Gain reduction" aria-valuemin={-24} aria-valuemax={0}
      aria-valuenow={Number((-value).toFixed(1))} aria-valuetext={`${value.toFixed(1)} decibels of gain reduction`}>
      <div className="meter-grades" aria-hidden="true">
        {ticks.map(tick => <i key={tick.db} style={{ left: `${tick.fraction * 100}%` }} />)}
      </div>
      <div className="meter-bar" aria-hidden="true">
        <div className="meter-fill" style={{ width: `${at(value)}%` }} />
      </div>
      {held > 0.2 && <i className="meter-hold" style={{ left: `${at(held)}%` }} />}
    </div>
    <Scale ticks={ticks} />
    <span className="meter-caption mono">{enabled ? `${value.toFixed(1)} dB GR` : idleLabel}</span>
  </div>;
}

/** Level ladder: a −48…0 dBFS scale with the threshold, the ceiling and the live peak. */
export function LevelLadder({ peakDb, thresholdDb, ceilingDb, enabled, idleLabel = 'limiter bypassed' }: { peakDb: number; thresholdDb: number; ceilingDb: number; enabled: boolean; idleLabel?: string }) {
  const at = (value: number) => ((clamp(value, -48, 0) + 48) / 48) * 100;
  const peak = enabled ? clamp(peakDb, -48, 0) : -48;
  return <div className={`level-ladder ${enabled ? '' : 'idle'}`}>
    <div className="meter-track" role="img" aria-label={`Output peak ${peak.toFixed(1)} dBFS, threshold ${thresholdDb.toFixed(1)}, ceiling ${ceilingDb.toFixed(1)}`}>
      <div className="ladder-above" aria-hidden="true" style={{ left: `${at(thresholdDb)}%` }} />
      <div className="meter-bar" aria-hidden="true">
        <div className="ladder-peak" style={{ width: `${at(peak)}%` }} />
      </div>
      <i className="ladder-threshold" aria-hidden="true" style={{ left: `${at(thresholdDb)}%` }} />
      <i className="ladder-ceiling" aria-hidden="true" style={{ left: `${at(ceilingDb)}%` }} />
    </div>
    <Scale ticks={dbTicks(0, -48, LEVEL_TICKS)} />
    <span className="meter-caption">
      {enabled
        ? <>threshold <b className="mono">{db(thresholdDb)}</b> · ceiling <b className="mono">{db(ceilingDb)}</b> · peak <b className="mono">{db(peakDb)}</b> dBFS</>
        : <>{idleLabel} · ceiling <b className="mono">{db(ceilingDb)}</b></>}
    </span>
  </div>;
}
