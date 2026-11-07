import { AudioWaveform } from 'lucide-react';
import { describeRack, ProcessorRack, RATE_KHZ } from '../components/ProcessorRack';
import type { useChain } from '../useChain';
import { Page } from './Page';

interface Props {
  chain: ReturnType<typeof useChain>;
  /** The equaliser is the fourth processor and keeps its own page, so this page links to it. */
  onOpenEq: () => void;
}

export function ProcessorsPage({ chain, onOpenEq }: Props) {
  return <Page
    title="Processors"
    state={describeRack(chain)}
    actions={<div className="page-actions-row">
      {/* The equaliser has a workspace of its own; the rest of the path is edited here. */}
      <button className="page-button" onClick={onOpenEq}><AudioWaveform size={13} />Equaliser</button>
      <span className="instrument-tag" title="Engine rate and channel layout">{RATE_KHZ} kHz · stereo</span>
    </div>}
  >
    <div className="page-grid processors-grid">
      <ProcessorRack chain={chain} onOpenEq={onOpenEq} />
    </div>
  </Page>;
}
