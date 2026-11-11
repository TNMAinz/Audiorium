import { renderToStaticMarkup } from 'react-dom/server';
import type { ComponentProps } from 'react';
import { describe, expect, it } from 'vitest';
import { ProcessorRack } from './ProcessorRack';
import { ProcessorsPage } from '../pages/ProcessorsPage';
import { defaultChain, effectNames, emptyPresets, idleEqResponse, idleMeters } from '../model';

type Chain = ComponentProps<typeof ProcessorRack>['chain'];

const chain: Chain = {
  native: true, chain: defaultChain, meters: idleMeters, eqCurve: idleEqResponse, error: '',
  presets: emptyPresets, patchError: '',
  setPanner: () => {}, setReverb: () => {}, setLimiter: () => {}, setCompressor: () => {}, setSaturator: () => {}, setEffect: () => {}, setOrder: () => {}, setStereoMode: () => {}, randomizeReverb: () => {},
  setEq: () => {}, updateFilter: () => {}, addFilter: () => {}, removeFilter: () => {},
  resetEq: () => {}, matchHeadroom: () => {},
  applyPreset: () => {}, renamePreset: async () => true, deletePreset: () => {},
};
const render = (settings: Chain) => renderToStaticMarkup(<ProcessorsPage chain={settings} onOpenEq={() => {}} />);

describe('processor rack', () => {
  it('exposes the additional effects with independent bypass and controls', () => {
    const markup = render(chain);
    for (const name of effectNames) {
      expect(markup).toContain(`aria-label="${name} enabled"`);
      expect(markup).toContain(`aria-label="${name} Mix"`);
    }
    expect(markup).toContain('fixed order · before limiter');
  });

  it('switches the rack between left/right and mid/side', () => {
    const markup = render(chain);
    expect(markup).toContain('aria-label="Channels"');
    expect(markup).toContain('>L / R<');
    expect(markup).toContain('>M / S<');
    expect(markup).toContain('Rack runs on left and right');

    // The mode is the engine's, so the checked switch is the one the chain reports.
    const midSide = render({
      ...chain,
      chain: { ...defaultChain, stereoMode: 'ms' },
    });
    expect(midSide).toContain('Rack runs on mid and side');
    expect(midSide).not.toContain('Rack runs on left and right');
  });

  it('is built from instruments, not a form of sliders', () => {
    const markup = render(chain);
    expect(markup).not.toContain('type="range"');
    expect(markup).not.toContain('<select');
    // Knobs, a position field, a meter and a routing diagram all exist.
    expect(markup).toContain('class="knob-dial"');
    expect(markup).toContain('aria-label="Source position"');
    expect(markup).toContain('role="meter"');
    expect(markup).toContain('role="radiogroup"');
  });

  it('shows each engine with the values it is running', () => {
    const markup = render(chain);
    expect(markup).toContain('Limiter');
    expect(markup).toContain('Reverb');
    expect(markup).toContain('Binaural panner');
    expect(markup).toContain('aria-valuetext="−3.0 dB"');
    // The decay is the time it is worth, not a percentage of a sweep.
    expect(markup).toContain('>2.2 s<');
    expect(markup).toContain('azimuth 0 degrees, elevation 0 degrees');
  });

  it('gives every knob a scale and a role with real bounds', () => {
    const markup = render(chain);
    expect(markup).toContain('aria-label="Threshold"');
    expect(markup).toContain('aria-valuemin="-24"');
    expect(markup).toContain('aria-valuemax="0"');
    // The output level is a control of its own, not a fixed constant in the engine.
    expect(markup).toContain('aria-label="Output"');
    expect(markup).toContain('aria-valuetext="−0.3 dBFS"');
    // Dry and wet are two gains with their own scales, not one balance.
    expect(markup).toContain('aria-label="Dry"');
    expect(markup).toContain('aria-label="Wet"');
    expect(markup).toContain('aria-label="Decay"');
    expect(markup).toContain('aria-label="Reflection level"');
    expect(markup).toContain('aria-label="Focus"');
    expect(markup).toContain('aria-label="Release"');
    expect(markup).toContain('aria-label="Detector"');
    expect(markup).toContain('aria-label="Reflection timbre"');
  });

  it('chooses room routing from diagrams instead of a dropdown', () => {
    const markup = render(chain);
    expect(markup).toContain('aria-label="parallel routing"');
    expect(markup).toContain('aria-label="series routing"');
    expect(markup).toContain('aria-label="woven routing"');
    expect(markup).toContain('aria-checked="true" aria-label="woven routing"');
  });

  it('says why a reflection material cannot be heard yet', () => {
    // Bypassed stage: the material is still selectable, and the strip says it is silent.
    const bypassed = render(chain);
    expect(bypassed).toContain('panner-footer inactive');
    expect(bypassed).toContain('switch the panner in to hear it');
    expect(bypassed).toContain('aria-label="Reflection timbre"');
    // The chosen material is named on the readout, marked as not running.
    expect(bypassed).toContain('>Studio · off</dd>');

    // Stage in, reflections off: the taps are what the material shapes, so it is still quiet.
    const reflectionsOff: Chain = {
      ...chain,
      chain: { ...defaultChain, panner: { ...defaultChain.panner, enabled: true, room: false } },
    };
    const off = render(reflectionsOff);
    expect(off).toContain('panner-footer inactive');
    expect(off).toContain('switch Reflections on, and raise Reflection level to hear it');

    // Both on: the plain note, and the bank no longer reads as inactive.
    const audible: Chain = {
      ...chain,
      chain: { ...defaultChain, panner: { ...defaultChain.panner, enabled: true, room: true } },
    };
    const live = render(audible);
    expect(live).not.toContain('panner-footer inactive');
    expect(live).not.toContain('cannot be heard');
    expect(live).toContain('>Studio</dd>');
    expect(live).not.toContain('Studio · off');
    expect(live).toContain('Four discrete taps, not a licensed HRTF');
  });

  it('draws the four reflection taps the panner renders', () => {
    const markup = render(chain);
    expect(markup).toContain('aria-label="Early reflections at 4.04 milliseconds, 6.64 milliseconds, 9.82 milliseconds, 13.28 milliseconds"');
    expect(markup).toContain('reflections off');
  });

  it('reports the limiter from its own meters, with graded scales', () => {
    const running: Chain = {
      ...chain,
      chain: { ...defaultChain, limiter: { ...defaultChain.limiter, enabled: true, thresholdDb: -12 } },
      meters: { ...idleMeters, gainReductionDb: -6.4, outputPeakDb: -0.4, limiterEnabled: true },
    };
    const markup = render(running);
    expect(markup).toContain('aria-valuenow="-6.4"');
    expect(markup).toContain('6.4 dB GR');
    expect(markup).toContain('L −6.4 dB');
    expect(markup).toContain('ceiling <b class="mono">−0.3</b>');
    expect(markup).toContain('peak <b class="mono">−0.4</b>');
  });

  it('is one signal path in the order the audio travels it', () => {
    const markup = render(chain);
    const panner = markup.indexOf('Binaural panner');
    const reverb = markup.indexOf('>Reverb<');
    const limiter = markup.indexOf('>Limiter<');
    expect(panner).toBeGreaterThan(-1);
    expect(reverb).toBeGreaterThan(panner);
    expect(limiter).toBeGreaterThan(reverb);
    // Each strip is numbered, and the numbers are in that same order.
    expect(markup).toContain('>01<');
    expect(markup).toContain('>02<');
    expect(markup).toContain('>03<');
  });

  it('moves a stage through the path instead of only drawing it', () => {
    const markup = render(chain);
    // Every stage can be moved, and the two ends say where the path stops.
    expect(markup).toContain('aria-label="Move Reverb earlier"');
    expect(markup).toContain('aria-label="Move Limiter later"');
    expect(markup).toContain('aria-label="Move Binaural panner earlier" title="Earlier in the signal path" disabled');
    expect(markup).toContain('aria-label="Move Limiter later" title="Later in the signal path" disabled');
  });

  it('carries the compressor and the saturator as processors of their own', () => {
    const markup = render(chain);
    expect(markup).toContain('>Compressor<');
    expect(markup).toContain('>Saturator<');
    expect(markup).toContain('aria-label="Ratio"');
    expect(markup).toContain('aria-label="Knee"');
    expect(markup).toContain('aria-label="Make-up"');
    expect(markup).toContain('aria-label="Drive"');
    expect(markup).toContain('aria-label="Tone"');
    // The oversampling is a figure on the plate, not a word in the stage's model line.
    expect(markup).toContain('>Oversampling<');
    expect(markup).toContain('>4×<');
  });

  it('draws the rack in the order the engine was given', () => {
    const reordered: Chain = {
      ...chain,
      chain: { ...defaultChain, order: ['limiter', 'compressor', 'saturator', 'eq', 'reverb', 'panner'] },
    };
    const markup = render(reordered);
    const limiter = markup.indexOf('>Limiter<');
    const compressor = markup.indexOf('>Compressor<');
    const panner = markup.indexOf('>Binaural panner<');
    expect(limiter).toBeGreaterThan(-1);
    expect(compressor).toBeGreaterThan(limiter);
    expect(panner).toBeGreaterThan(compressor);
    // The numbers follow the order, they are not the default sequence's numbers.
    expect(markup.indexOf('>01<')).toBeLessThan(limiter);
  });

  it('states how many stages are in the path, and which are not', () => {
    expect(render(chain)).toContain('All 6 bypassed · bit-exact pass-through');
    const roomOnly: Chain = {
      ...chain,
      chain: { ...defaultChain, reverb: { ...defaultChain.reverb, enabled: true } },
      meters: { ...idleMeters, reverbEnabled: true, wetMix: 0.42 },
    };
    expect(render(roomOnly)).toContain('1 of 6 engaged · Reverb · Binaural panner, Equaliser, Compressor, Saturator and Limiter bypassed');
  });

  it('reports the output level it is holding, not a constant', () => {
    const quieter: Chain = {
      ...chain,
      chain: { ...defaultChain, limiter: { ...defaultChain.limiter, enabled: true, ceilingDb: -12 } },
      meters: { ...idleMeters, limiterEnabled: true, outputPeakDb: -12, gainReductionDb: -4 },
    };
    const markup = render(quieter);
    expect(markup).toContain('−12.0 dBFS');
    // The ladder marks the same ceiling the limiter is holding.
    expect(markup).toContain('ceiling <b class="mono">−12.0</b>');
  });

  it('shows the engine figures the knobs stand for', () => {
    const markup = render(chain);
    expect(markup).toContain('± 32 smp · 0.67 ms');
    expect(markup).toContain('72 smp · 1.5 ms');
    expect(markup).toContain('8 · prime');
    expect(markup).toContain('181–4099 smp');
    expect(markup).toContain('3.8–85.4 ms');
    expect(markup).toContain('−0.3 dBFS');
  });

  it('shows a dash, not a fabricated zero, while the engine reports nothing', () => {
    const armed: Chain = {
      ...chain,
      chain: { ...defaultChain, limiter: { ...defaultChain.limiter, enabled: true } },
    };
    const markup = render(armed);
    expect(markup).toContain('no engine data');
    expect(markup).not.toContain('0.0 dB GR');
    expect(markup).toContain('L idle');
  });

  it('says what each engine is not, instead of overclaiming', () => {
    const markup = render(chain);
    expect(markup).toContain('Not true-peak, not a loudness normaliser');
    expect(markup).toContain('not a licensed HRTF');
    expect(markup).toContain('Every line feeds every other');
    expect(markup).toContain('bit-exact pass-through');
  });

  it('rolls the room from one button instead of a string to edit', () => {
    // The network string is the engine's own identity for a room, not a control: the
    // rack keeps the routing diagrams and one roll, and fits Decay, Dry and Wet in a row.
    const markup = render(chain);
    expect(markup).not.toContain('<input');
    expect(markup).toContain('aria-label="New room"');
    expect(markup).toContain('Roll a new room: a new routing and new prime delays');
    expect(markup).toContain('aria-label="parallel routing"');
    // One row: routing diagrams and the roll on the left, the three gains beside them.
    expect(markup).toContain('class="routing-field"');
    expect(markup).not.toContain('network-field');
  });
});
