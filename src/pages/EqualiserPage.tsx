import { Equaliser } from '../components/Equaliser';
import { describeEq, describePatch } from '../model';
import type { useChain } from '../useChain';
import { Page } from './Page';

/**
 * The equaliser gets the page rather than a card at the bottom of another one:
 * the plot, the patch row, the band rack and the editor are one instrument, and
 * all of them have to be visible while you turn a knob.
 */
export function EqualiserPage({ chain }: { chain: ReturnType<typeof useChain> }) {
  const eq = chain.chain.eq;
  return <Page
    title="Equaliser"
    state={`${describePatch(chain.presets)} · ${describeEq(eq)} · 48 kHz stereo · playback`}
    actions={<span className={`source-badge ${eq.enabled ? 'live' : ''}`}><i />{eq.enabled ? 'In path' : 'Bypassed'}</span>}
  >
    <div className="page-grid equaliser-grid">
      <Equaliser
        eq={eq}
        curve={chain.eqCurve}
        native={chain.native}
        onChange={chain.setEq}
        presets={chain.presets}
        patchError={chain.patchError}
        onApplyPreset={chain.applyPreset}
        onRenamePreset={chain.renamePreset}
        onDeletePreset={chain.deletePreset}
      />
    </div>
  </Page>;
}
