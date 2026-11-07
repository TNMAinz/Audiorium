import { Dices } from 'lucide-react';
import {
  limiterDetectors, limiterReleases, reflectionTimbres, reverbTime,
  type LimiterDetector, type LimiterRelease,
} from '../model';
import type { useChain } from '../useChain';
import { Knob } from './instrument/Knob';
import { Segmented } from './instrument/Segmented';
import { GainReductionMeter, LevelLadder } from './instrument/Meters';
import { PositionPad } from './instrument/PositionPad';
import { RoutingDiagram } from './instrument/RoutingDiagram';
import { TapStrip } from './instrument/TapStrip';

interface Props {
  chain: ReturnType<typeof useChain>;
}

const percent = (value: number) => `${Math.round(value * 100)}%`;
const signed = (value: number) => `${value > 0 ? '+' : value < 0 ? '−' : ''}${Math.abs(value).toFixed(1)}`;
const db = (value: number) => (value > -120 ? `${signed(value)} dBFS` : '−∞ dBFS');

/**
 * The engine runs at 48 kHz. Its delay lines are counted in samples, so the
 * rack reports both: the count the Rust code actually holds, and the time that
 * count is worth.
 */
export const RATE_KHZ = 48;
const ITD_SAMPLES = 32;
const LOOKAHEAD_SAMPLES = 72;
const ROOM_DELAY_SAMPLES: [number, number] = [181, 4099];
const milliseconds = (samples: number) => `${Number((samples / RATE_KHZ).toFixed(2))} ms`;
const delayRange = `${(ROOM_DELAY_SAMPLES[0] / RATE_KHZ).toFixed(1)}–${(ROOM_DELAY_SAMPLES[1] / RATE_KHZ).toFixed(1)} ms`;

/** What the three stages are doing, as the page's one state line. */
export function describeRack(chain: ReturnType<typeof useChain>) {
  const { panner, reverb, limiter } = chain.chain;
  const stages = [
    { label: 'Binaural panner', on: panner.enabled },
    { label: 'Reverb', on: reverb.enabled },
    { label: 'Limiter', on: limiter.enabled },
  ];
  const active = stages.filter(stage => stage.on);
  const bypassed = stages.filter(stage => !stage.on).map(stage => stage.label);
  return active.length === 0
    ? 'All three bypassed · bit-exact pass-through'
    : `${active.length} of 3 in path · ${active.map(stage => stage.label).join(', ')}${bypassed.length ? ` · ${bypassed.join(' and ')} bypassed` : ''}`;
}

function Power({ on, label, onToggle }: { on: boolean; label: string; onToggle: () => void }) {
  return <button
    type="button"
    role="switch"
    aria-checked={on}
    aria-label={label}
    className={`stage-power ${on ? 'on' : ''}`}
    onClick={onToggle}
  ><i aria-hidden="true" /><span>{on ? 'IN' : 'BYPASS'}</span></button>;
}

/** One row of the stage's spec plate: a name and the figure the engine is holding. */
function Spec({ name, value }: { name: string; value: string }) {
  return <div><dt>{name}</dt><dd className="mono" title={value}>{value}</dd></div>;
}

export function ProcessorRack({ chain }: Props) {
  const { panner, reverb, limiter } = chain.chain;
  const meters = chain.meters;
  // A material only shapes the four early taps, so it can be heard only when the stage is
  // in the path and Reflections is on. The strip says which switch is missing rather than
  // leaving a live-looking bank that changes nothing audible.
  const materialAudible = panner.enabled && panner.room;
  const structure = reverb.network.split(':')[0].toLowerCase();
  const seed = reverb.network.split(':')[1] ?? '';

  return <div className="rack-body">
    {!chain.native && <p className="audio-warning">Processors run in the Windows app. These controls drive the real engine; nothing here is simulated.</p>}
    {chain.error && <p className="audio-error error-message">{chain.error}</p>}

    <ol className="rack-stages">
      <li className={`stage panner ${panner.enabled ? 'in-path' : ''}`} aria-labelledby="stage-panner">
        <header className="stage-head">
          <span className="stage-index mono" aria-hidden="true">01</span>
          <div className="stage-name">
            <h3 id="stage-panner">Binaural panner</h3>
            <span className="stage-model mono">spherical head</span>
          </div>
          <Power on={panner.enabled} label="Binaural panner enabled" onToggle={() => chain.setPanner({ enabled: !panner.enabled })} />
        </header>
        <div className="stage-body">
          <div className="stage-controls panner">
            <div className="pad-cell">
              <PositionPad
                azimuth={panner.azimuth}
                elevation={panner.elevation}
                reflections={panner.room}
                onChange={next => chain.setPanner(next)}
              />
            </div>
            <div className="panner-right">
              <div className="stage-knobs">
                <Knob size={44} label="Width" value={panner.width} min={0} max={1} step={0.01} reset={0.25} format={percent} description="Stereo width before placement" onChange={value => chain.setPanner({ width: value })} />
                <Knob size={44} label="Focus" value={panner.focus} min={0} max={1} step={0.01} reset={0.5} format={percent} description="Tonal character against externalisation" onChange={value => chain.setPanner({ focus: value })} />
                <Knob size={44} label="Reflection level" value={panner.reflectionLevel} min={0} max={1} step={0.01} reset={0.2} format={percent} description="Level of the four early taps" onChange={value => chain.setPanner({ reflectionLevel: value })} />
                <Knob size={44} label="Reflection length" value={panner.reflectionLength} min={0} max={1} step={0.01} reset={0.35} format={percent} description="Scales the early reflection delays" onChange={value => chain.setPanner({ reflectionLength: value })} />
              </div>
            </div>
            <div className={`panner-footer ${materialAudible ? '' : 'inactive'}`}>
              <span className="rack-label">Reflection timbre</span>
              <Segmented
                compact
                label="Reflection timbre"
                value={String(panner.timbre)}
                options={reflectionTimbres.map((label, index) => ({ id: String(index), label }))}
                onChange={id => chain.setPanner({ timbre: Number(id) })}
              />
              <button type="button" role="switch" aria-checked={panner.room} aria-label="Early reflections" className={`toggle-line ${panner.room ? 'on' : ''}`} onClick={() => chain.setPanner({ room: !panner.room })}><i aria-hidden="true" />Reflections</button>
            </div>
          </div>
          <div className="stage-readout">
            <dl className="spec-list">
              <Spec name="ITD" value={`± ${ITD_SAMPLES} smp · ${milliseconds(ITD_SAMPLES)}`} />
              <Spec name="Taps" value={panner.room ? '4 early' : 'off'} />
              {/* The material is named even when it is not running, so choosing one always
                  answers on the readout instead of leaving a dash where the click landed. */}
              <Spec name="Timbre" value={`${reflectionTimbres[panner.timbre] ?? '—'}${materialAudible ? '' : ' · off'}`} />
            </dl>
            <TapStrip length={panner.reflectionLength} level={panner.reflectionLevel} active={panner.room} />
          </div>
        </div>
        <p className="stage-note">{materialAudible
          ? 'Head shadow, level and delay set the cue. Four discrete taps, not a licensed HRTF, and no tail of its own.'
          : panner.enabled
            ? 'Four discrete taps, not a licensed HRTF. Reflections are off, so no material can be heard: switch Reflections on, and raise Reflection level to hear it.'
            : 'Four discrete taps, not a licensed HRTF. The stage is bypassed, so the material is stored but not heard: switch the panner in to hear it.'}</p>
      </li>

      <li className={`stage room ${reverb.enabled ? 'in-path' : ''}`} aria-labelledby="stage-reverb">
        <header className="stage-head">
          <span className="stage-index mono" aria-hidden="true">02</span>
          <div className="stage-name">
            <h3 id="stage-reverb">Reverb</h3>
            <span className="stage-model mono">seeded delay network</span>
          </div>
          <Power on={reverb.enabled} label="Reverb enabled" onToggle={() => chain.setReverb({ enabled: !reverb.enabled })} />
        </header>
        <div className="stage-body">
          <div className="stage-controls room">
            <div className="routing-field">
              <RoutingDiagram value={structure} onChange={next => chain.setReverb({ network: `${next}:${seed}` })} />
              {/* The string behind this button is not a control the user has to read: one
                  click rolls a new room, and the routing diagrams say which shape it takes. */}
              <button
                type="button"
                className="page-button ghost room-roll"
                aria-label="New room"
                title="Roll a new room: a new routing and new prime delays"
                onClick={() => chain.randomizeReverb(false)}
              ><Dices size={12} aria-hidden="true" />New room</button>
            </div>
            <div className="stage-knobs">
              <Knob size={44} label="Decay" value={reverb.decay} min={0} max={1} step={0.01} reset={0.55} format={reverbTime} description="Reverb time, a fifth of a second to sixteen" onChange={value => chain.setReverb({ decay: value })} />
              <Knob size={44} label="Dry" value={reverb.dry} min={0} max={1} step={0.01} reset={1} format={percent} description="Level of the unprocessed signal" onChange={value => chain.setReverb({ dry: value })} />
              <Knob size={44} label="Wet" value={reverb.wet} min={0} max={1} step={0.01} reset={0.3} format={percent} description="Level of the room, which adds to the dry signal" onChange={value => chain.setReverb({ wet: value })} />
            </div>
          </div>
          <div className="stage-readout">
            <dl className="spec-list">
              <Spec name="Lines" value="8 · prime" />
              <Spec name="Delay" value={`${ROOM_DELAY_SAMPLES[0]}–${ROOM_DELAY_SAMPLES[1]} smp`} />
              <Spec name="Spread" value={delayRange} />
              <Spec name="Time" value={reverbTime(reverb.decay)} />
              <Spec name="Wet out" value={reverb.enabled && meters.reverbEnabled ? `${Math.round(meters.wetMix * 100)} %` : '—'} />
            </dl>
          </div>
        </div>
        <p className="stage-note">Every line feeds every other, so the tail is dense, not a row of echoes. Dry and Wet add the room; New room rolls a network that is kept.</p>
      </li>

      <li className={`stage limiter ${limiter.enabled ? 'in-path' : ''}`} aria-labelledby="stage-limiter">
        <header className="stage-head">
          <span className="stage-index mono" aria-hidden="true">03</span>
          <div className="stage-name">
            <h3 id="stage-limiter">Limiter</h3>
            <span className="stage-model mono">look-ahead peak</span>
          </div>
          <Power on={limiter.enabled} label="Limiter enabled" onToggle={() => chain.setLimiter({ enabled: !limiter.enabled })} />
        </header>
        <div className="stage-body">
          <div className="stage-controls limiter">
            <div className="stage-knobs">
              <Knob
                size={44}
                label="Threshold"
                value={limiter.thresholdDb}
                min={-24}
                max={0}
                step={0.5}
                reset={-3}
                ticks={[-24, -18, -12, -6, -3, 0]}
                format={value => `${signed(value)} dB`}
                description="Level where limiting starts"
                onChange={value => chain.setLimiter({ thresholdDb: value })}
              />
              {/* Output is the level the stage delivers, not a second threshold: the
                  limited signal is trimmed to it, so pulling it down cannot make the
                  gain work harder and grain up. */}
              <Knob
                size={44}
                label="Output"
                value={limiter.ceilingDb}
                min={-24}
                max={0}
                step={0.5}
                reset={-0.3}
                ticks={[-24, -18, -12, -6, -3, 0]}
                format={value => `${signed(value)} dBFS`}
                description="The level the stage delivers. With Make-up on, the limited peak is trimmed to it; turning it down only attenuates."
                onChange={value => chain.setLimiter({ ceilingDb: value })}
              />
            </div>
            <div className="seg-field">
              <span className="rack-label">Release</span>
              <Segmented
                compact
                label="Release"
                value={limiter.release}
                options={limiterReleases}
                onChange={id => chain.setLimiter({ release: id as LimiterRelease })}
              />
            </div>
            <div className="seg-field">
              <span className="rack-label">Detector</span>
              <Segmented
                compact
                label="Detector"
                value={limiter.detector}
                options={limiterDetectors}
                onChange={id => chain.setLimiter({ detector: id as LimiterDetector })}
              />
            </div>
            <div className="limiter-flags">
              <span className="rack-label">Options</span>
              <div className="limiter-options">
                <button type="button" role="switch" aria-checked={limiter.softClip} aria-label="Soft clip" className={`toggle-line ${limiter.softClip ? 'on' : ''}`} onClick={() => chain.setLimiter({ softClip: !limiter.softClip })}><i aria-hidden="true" />Soft clip</button>
                <button type="button" role="switch" aria-checked={limiter.autoMakeup} aria-label="Automatic make-up gain" title="Normalises the limited peak to the output level" className={`toggle-line ${limiter.autoMakeup ? 'on' : ''}`} onClick={() => chain.setLimiter({ autoMakeup: !limiter.autoMakeup })}><i aria-hidden="true" />Make-up</button>
              </div>
            </div>
          </div>
          <div className="stage-readout">
            <dl className="spec-list">
              <Spec name="Output" value={`${signed(limiter.ceilingDb)} dBFS`} />
              <Spec name="Look-ahead" value={`${LOOKAHEAD_SAMPLES} smp · ${milliseconds(LOOKAHEAD_SAMPLES)}`} />
              <Spec name="Reduction" value={limiter.enabled && meters.limiterEnabled ? `${signed(meters.gainReductionDb)} dB` : '—'} />
              <Spec name="Peak" value={limiter.enabled && meters.limiterEnabled ? db(meters.outputPeakDb) : '—'} />
            </dl>
          </div>
        </div>
        <p className="stage-note">Threshold is where the gain starts; Output is a trim after it, so a quiet output keeps the waveform's shape. Both channels are linked, so the louder one decides. Not true-peak, not a loudness normaliser.</p>
      </li>
    </ol>

    <section className="meter-bridge" aria-label="Limiter meters">
      <header className="stage-head">
        <span className="stage-index mono" aria-hidden="true">bridge</span>
        <div className="stage-name">
          <h3>Meter bridge</h3>
          <span className="stage-model mono">{limiter.enabled ? (meters.limiterEnabled ? 'live' : 'waiting') : 'bypassed'}</span>
        </div>
      </header>
      <div className="bridge-meters">
        <div className="bridge-meter">
          <span className="rack-label">Gain reduction</span>
          <GainReductionMeter
            reductionDb={meters.gainReductionDb}
            enabled={limiter.enabled && meters.limiterEnabled}
            idleLabel={limiter.enabled ? 'no engine data' : 'limiter bypassed'}
          />
        </div>
        <div className="bridge-meter">
          <span className="rack-label">Output peak</span>
          <LevelLadder
            peakDb={meters.outputPeakDb}
            thresholdDb={limiter.thresholdDb}
            ceilingDb={limiter.ceilingDb}
            enabled={limiter.enabled && meters.limiterEnabled}
            idleLabel={limiter.enabled ? 'no engine data' : 'limiter bypassed'}
          />
        </div>
      </div>
      <footer className="rack-footer">
        <span className="mono" title="Panner in path">{panner.enabled ? 'P in' : 'P –'}</span>
        <span className="mono" title="Reverb wet mix">{reverb.enabled ? (meters.reverbEnabled ? `R ${percent(meters.wetMix)} wet` : 'R idle') : 'R –'}</span>
        <span className="mono" title="Limiter gain reduction">{limiter.enabled ? (meters.limiterEnabled ? `L ${signed(meters.gainReductionDb)} dB` : 'L idle') : 'L –'}</span>
        <span className="rack-footer-note mono">Meters read the engine, not the interface.</span>
      </footer>
    </section>
  </div>;
}
