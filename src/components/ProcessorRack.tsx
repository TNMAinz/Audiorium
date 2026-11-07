import { ChevronDown, ChevronUp, Dices } from 'lucide-react';
import {
  limiterDetectors, limiterReleases, moveStage, reflectionTimbres, reverbTime, stageModels,
  stageNames, stereoModes, type LimiterDetector, type LimiterRelease, type StageId, type StereoMode,
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
  /** The equaliser is one of the six stages and keeps a page of its own. */
  onOpenEq?: () => void;
}

const percent = (value: number) => `${Math.round(value * 100)}%`;
const signed = (value: number) => `${value > 0 ? '+' : value < 0 ? '−' : ''}${Math.abs(value).toFixed(1)}`;
const db = (value: number) => (value > -120 ? `${signed(value)} dBFS` : '−∞ dBFS');
const ratio = (value: number) => `${value.toFixed(1)}:1`;
const millisecondsValue = (value: number) => `${value >= 1000 ? (value / 1000).toFixed(1) : Math.round(value)} ${value >= 1000 ? 's' : 'ms'}`;

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

type Chain = ReturnType<typeof useChain>['chain'];

/** Whether the engine is doing anything in this stage right now. */
export function stageRunning(stage: StageId, config: Chain) {
  switch (stage) {
    case 'panner': return config.panner.enabled;
    case 'reverb': return config.reverb.enabled;
    case 'eq': return config.eq.enabled && (config.eq.preampDb !== 0 || config.eq.filters.some(filter => filter.enabled));
    case 'compressor': return config.compressor.enabled;
    case 'saturator': return config.saturator.enabled;
    case 'limiter': return config.limiter.enabled;
  }
}

/** What the rack is doing, as the page's one state line, in the order it runs. */
export function describeRack(chain: ReturnType<typeof useChain>) {
  const config = chain.chain;
  const order = config.order;
  const active = order.filter(stage => stageRunning(stage, config));
  const bypassed = order.filter(stage => !stageRunning(stage, config)).map(stage => stageNames[stage]);
  const total = order.length;
  if (active.length === 0) return `All ${total} bypassed · bit-exact pass-through`;
  const names = active.map(stage => stageNames[stage]).join(', ');
  return `${active.length} of ${total} engaged · ${names}${bypassed.length ? ` · ${listWords(bypassed)} bypassed` : ''}`;
}

/** "A", "A and B", "A, B and C": a list a sentence can hold. */
function listWords(names: string[]) {
  if (names.length <= 1) return names.join('');
  return `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
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

interface StageProps {
  chain: ReturnType<typeof useChain>;
  index: number;
  count: number;
  onMove: (from: number, to: number) => void;
}

/** The stripe every stage carries: its number in the path, its name, and its place. */
function Head({ index, count, onMove, stage, on, onToggle }: Omit<StageProps, 'chain'> & {
  stage: StageId;
  on: boolean;
  onToggle: () => void;
}) {
  const name = stageNames[stage];
  const id = `stage-${stage}`;
  return <header className="stage-head">
    <span className="stage-index mono" aria-hidden="true">{String(index + 1).padStart(2, '0')}</span>
    <div className="stage-name">
      <h3 id={id}>{name}</h3>
      <span className="stage-model mono">{stageModels[stage]}</span>
    </div>
    {/* Moving a stage is a press, not a drag: a drag would fight the page's own scroll
        and could not be done from the keyboard. */}
    <div className="stage-move">
      <button
        type="button"
        className="icon-button"
        aria-label={`Move ${name} earlier`}
        title="Earlier in the signal path"
        disabled={index === 0}
        onClick={() => onMove(index, index - 1)}
      ><ChevronUp size={13} aria-hidden="true" /></button>
      <button
        type="button"
        className="icon-button"
        aria-label={`Move ${name} later`}
        title="Later in the signal path"
        disabled={index === count - 1}
        onClick={() => onMove(index, index + 1)}
      ><ChevronDown size={13} aria-hidden="true" /></button>
    </div>
    <Power on={on} label={`${name} enabled`} onToggle={onToggle} />
  </header>;
}

function PannerStage(props: StageProps) {
  const { chain } = props;
  const panner = chain.chain.panner;
  const materialAudible = panner.enabled && panner.room;
  // A material only shapes the four early taps, so it can be heard only when the stage is
  // in the path and Reflections is on. The strip says which switch is missing rather than
  // leaving a live-looking bank that changes nothing audible.
  return <li className={`stage panner ${panner.enabled ? 'in-path' : ''}`} aria-labelledby="stage-panner">
    <Head {...props} stage="panner" on={panner.enabled} onToggle={() => chain.setPanner({ enabled: !panner.enabled })} />
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
  </li>;
}

function ReverbStage(props: StageProps) {
  const { chain } = props;
  const reverb = chain.chain.reverb;
  const structure = reverb.network.split(':')[0].toLowerCase();
  const seed = reverb.network.split(':')[1] ?? '';
  return <li className={`stage room ${reverb.enabled ? 'in-path' : ''}`} aria-labelledby="stage-reverb">
    <Head {...props} stage="reverb" on={reverb.enabled} onToggle={() => chain.setReverb({ enabled: !reverb.enabled })} />
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
          <Spec name="Wet out" value={reverb.enabled && chain.meters.reverbEnabled ? `${Math.round(chain.meters.wetMix * 100)} %` : '—'} />
        </dl>
      </div>
    </div>
    <p className="stage-note">Every line feeds every other, so the tail is dense, not a row of echoes. Dry and Wet add the room; New room rolls a network that is kept.</p>
  </li>;
}

function EqStage(props: StageProps & { onOpenEq?: () => void }) {
  const { chain, onOpenEq } = props;
  const eq = chain.chain.eq;
  const bands = eq.filters.filter(filter => filter.enabled).length;
  const running = stageRunning('eq', chain.chain);
  return <li className={`stage equaliser ${running ? 'in-path' : ''}`} aria-labelledby="stage-eq">
    <Head {...props} stage="eq" on={running} onToggle={() => chain.setEq({ ...eq, enabled: !eq.enabled })} />
    <div className="stage-body">
      <div className="stage-controls eq-summary">
        <div className="stage-readout">
          <dl className="spec-list">
            <Spec name="Bands" value={`${bands} of ${eq.filters.length} in`} />
            <Spec name="Preamp" value={`${signed(eq.preampDb)} dB`} />
            <Spec name="Largest boost" value={`${signed(chain.eqCurve.maxBoostDb)} dB`} />
            <Spec name="Headroom" value={`${signed(chain.eqCurve.headroomDb)} dB`} />
          </dl>
        </div>
        <button type="button" className="page-button" onClick={onOpenEq}>Open the equaliser</button>
      </div>
    </div>
    <p className="stage-note">A parametric cascade of up to 24 bands and one preamp. The bands are edited on the equaliser's own page; this strip only switches the stage in and shows what it is holding.</p>
  </li>;
}

function CompressorStage(props: StageProps) {
  const { chain } = props;
  const compressor = chain.chain.compressor;
  const live = compressor.enabled && chain.meters.compressorEnabled;
  return <li className={`stage compressor ${compressor.enabled ? 'in-path' : ''}`} aria-labelledby="stage-compressor">
    <Head {...props} stage="compressor" on={compressor.enabled} onToggle={() => chain.setCompressor({ enabled: !compressor.enabled })} />
    <div className="stage-body">
      <div className="stage-controls compressor">
        <div className="stage-knobs">
          <Knob size={44} label="Threshold" value={compressor.thresholdDb} min={-60} max={0} step={0.5} reset={-20} ticks={[-60, -40, -20, -12, -6, 0]} format={value => `${signed(value)} dB`} description="Level where the curve starts to bend" onChange={value => chain.setCompressor({ thresholdDb: value })} />
          <Knob size={44} label="Ratio" value={compressor.ratio} min={1} max={20} step={0.1} reset={3} ticks={[1, 2, 4, 8, 12, 20]} format={ratio} description="How far the level above the threshold is turned down" onChange={value => chain.setCompressor({ ratio: value })} />
          <Knob size={44} label="Attack" value={compressor.attackMs} min={0.5} max={200} step={0.5} reset={10} format={millisecondsValue} description="How quickly the gain reaches its reduction" onChange={value => chain.setCompressor({ attackMs: value })} />
          <Knob size={44} label="Release" value={compressor.releaseMs} min={10} max={2000} step={5} reset={200} format={millisecondsValue} description="How slowly the gain comes back" onChange={value => chain.setCompressor({ releaseMs: value })} />
          <Knob size={44} label="Knee" value={compressor.kneeDb} min={0} max={24} step={0.5} reset={6} format={value => `${value.toFixed(1)} dB`} description="Width of the soft corner around the threshold" onChange={value => chain.setCompressor({ kneeDb: value })} />
          <Knob size={44} label="Make-up" value={compressor.makeupDb} min={0} max={24} step={0.5} reset={0} format={value => `${signed(value)} dB`} description="Gain added after the reduction" onChange={value => chain.setCompressor({ makeupDb: value })} />
        </div>
      </div>
      <div className="stage-readout">
        <dl className="spec-list">
          <Spec name="Threshold" value={`${compressor.thresholdDb.toFixed(1)} dB`} />
          <Spec name="Ratio" value={ratio(compressor.ratio)} />
          <Spec name="Knee" value={`${compressor.kneeDb.toFixed(1)} dB`} />
          <Spec name="Gain reduction" value={compressor.enabled ? (chain.meters.compressorEnabled ? `${signed(chain.meters.compressorReductionDb)} dB` : '—') : '—'} />
        </dl>
        <GainReductionMeter
          reductionDb={chain.meters.compressorReductionDb}
          enabled={live}
          idleLabel={compressor.enabled ? 'no engine data' : 'compressor bypassed'}
        />
      </div>
    </div>
    <p className="stage-note">One linked detector on the louder channel, so a loud side ducks the whole image. The knee bends the curve instead of cornering it, and the reduction never steps the waveform. Not a multiband compressor, and not a limiter.</p>
  </li>;
}

function SaturatorStage(props: StageProps) {
  const { chain } = props;
  const saturator = chain.chain.saturator;
  return <li className={`stage saturator ${saturator.enabled ? 'in-path' : ''}`} aria-labelledby="stage-saturator">
    <Head {...props} stage="saturator" on={saturator.enabled} onToggle={() => chain.setSaturator({ enabled: !saturator.enabled })} />
    <div className="stage-body">
      <div className="stage-controls saturator">
        <div className="stage-knobs">
          <Knob size={44} label="Drive" value={saturator.driveDb} min={0} max={24} step={0.5} reset={6} ticks={[0, 6, 12, 18, 24]} format={value => `${signed(value)} dB`} description="How hard the curve is pushed" onChange={value => chain.setSaturator({ driveDb: value })} />
          <Knob size={44} label="Tone" value={saturator.tone} min={0} max={1} step={0.01} reset={0.5} format={percent} description="Darker below half, brighter above it, neutral at half" onChange={value => chain.setSaturator({ tone: value })} />
          <Knob size={44} label="Mix" value={saturator.mix} min={0} max={1} step={0.01} reset={1} format={percent} description="Blend of the unprocessed signal against the saturated one" onChange={value => chain.setSaturator({ mix: value })} />
        </div>
      </div>
      <div className="stage-readout">
        <dl className="spec-list">
          <Spec name="Oversampling" value="4×" />
          <Spec name="Tone" value={saturator.tone === 0.5 ? 'neutral' : `${percent(saturator.tone)} ${saturator.tone < 0.5 ? 'dark' : 'bright'}`} />
          <Spec name="Mix" value={percent(saturator.mix)} />
        </dl>
      </div>
    </div>
    <p className="stage-note">A tanh curve run at four times the sample rate, so the harmonics it adds stay below the band instead of folding back into it. A 10 Hz DC blocker follows it, so no standing offset leaves the stage.</p>
  </li>;
}

function LimiterStage(props: StageProps) {
  const { chain } = props;
  const limiter = chain.chain.limiter;
  return <li className={`stage limiter ${limiter.enabled ? 'in-path' : ''}`} aria-labelledby="stage-limiter">
    <Head {...props} stage="limiter" on={limiter.enabled} onToggle={() => chain.setLimiter({ enabled: !limiter.enabled })} />
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
          <Spec name="Gain reduction" value={limiter.enabled && chain.meters.limiterEnabled ? `${signed(chain.meters.gainReductionDb)} dB` : '—'} />
          <Spec name="Peak" value={limiter.enabled && chain.meters.limiterEnabled ? db(chain.meters.outputPeakDb) : '—'} />
        </dl>
      </div>
    </div>
    <p className="stage-note">Threshold is where the gain starts; Output is a trim after it, so a quiet output keeps the waveform's shape. Both channels are linked, so the louder one decides. Not true-peak, not a loudness normaliser.</p>
  </li>;
}

export function ProcessorRack({ chain, onOpenEq = () => {} }: Props) {
  const { order, limiter } = chain.chain;
  const meters = chain.meters;

  const move = (from: number, to: number) => chain.setOrder(moveStage(order, from, to));

  // Each stage is one strip, and the strips are the order the audio travels: moving
  // one rewrites the order the engine runs, not a picture of it.
  const strip = (stage: StageId, index: number) => {
    const shared = { chain, index, count: order.length, onMove: move };
    switch (stage) {
      case 'panner': return <PannerStage key={stage} {...shared} />;
      case 'reverb': return <ReverbStage key={stage} {...shared} />;
      case 'eq': return <EqStage key={stage} {...shared} onOpenEq={onOpenEq} />;
      case 'compressor': return <CompressorStage key={stage} {...shared} />;
      case 'saturator': return <SaturatorStage key={stage} {...shared} />;
      case 'limiter': return <LimiterStage key={stage} {...shared} />;
    }
  };

  const mode = stereoModes.find(option => option.id === chain.chain.stereoMode) ?? stereoModes[0];

  return <div className="rack-body">
    {!chain.native && <p className="audio-warning">Processors run in the Windows app. These controls drive the real engine; nothing here is simulated.</p>}
    {chain.error && <p className="audio-error error-message">{chain.error}</p>}

    <div className="rack-mode">
      <Segmented
        label="Channels"
        options={stereoModes.map(option => ({ id: option.id, label: option.label }))}
        value={chain.chain.stereoMode}
        onChange={id => chain.setStereoMode(id as StereoMode)}
      />
      <span className="rack-mode-note mono" title={mode.note}>{mode.id === 'ms' ? 'Rack runs on mid and side' : 'Rack runs on left and right'}</span>
    </div>

    <ol className="rack-stages">
      {order.map((stage, index) => strip(stage, index))}
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
        <span className="mono" title="Panner engaged">{chain.chain.panner.enabled ? 'P in' : 'P –'}</span>
        <span className="mono" title="Reverb wet mix">{chain.chain.reverb.enabled ? (meters.reverbEnabled ? `R ${percent(meters.wetMix)} wet` : 'R idle') : 'R –'}</span>
        <span className="mono" title="Compressor gain reduction">{chain.chain.compressor.enabled ? (meters.compressorEnabled ? `C ${signed(meters.compressorReductionDb)} dB` : 'C idle') : 'C –'}</span>
        <span className="mono" title="Limiter gain reduction">{limiter.enabled ? (meters.limiterEnabled ? `L ${signed(meters.gainReductionDb)} dB` : 'L idle') : 'L –'}</span>
        <span className="rack-footer-note mono">Meters read the engine, not the interface.</span>
      </footer>
    </section>
  </div>;
}
