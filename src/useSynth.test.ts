import { describe, expect, it } from 'vitest';
import { SYNTH_IDLE_POLL_MS, SYNTH_POLL_MS, SYNTH_REDUCED_POLL_MS, synthPollInterval } from './useSynth';

describe('the instrument readout rate', () => {
  it('follows a moving trace, and settles once the engine stops', () => {
    expect(synthPollInterval(true, false)).toBe(SYNTH_POLL_MS);
    // A third of the engine's 30 ms trace window, so consecutive traces overlap:
    // the display scrolls instead of stepping.
    expect(SYNTH_POLL_MS).toBeLessThanOrEqual(40);
    // A stopped engine draws the patch, which does not move.
    expect(synthPollInterval(false, false)).toBe(SYNTH_IDLE_POLL_MS);
    expect(SYNTH_IDLE_POLL_MS).toBeGreaterThan(SYNTH_POLL_MS * 5);
  });

  it('asks a fifth as often under reduced motion, and is still live', () => {
    expect(synthPollInterval(true, true)).toBe(SYNTH_REDUCED_POLL_MS);
    expect(SYNTH_REDUCED_POLL_MS).toBe(SYNTH_POLL_MS * 5);
    // The trace still moves under reduced motion: it is slower, not frozen.
    expect(synthPollInterval(true, true)).toBeLessThan(1000);
  });
});
