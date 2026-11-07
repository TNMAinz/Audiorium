import { renderToStaticMarkup } from 'react-dom/server';
import type { ComponentProps } from 'react';
import { describe, expect, it } from 'vitest';
import { AudioInspector } from './AudioInspector';
import { defaultChain, defaultSpatial, emptyPresets, idleEqResponse, idleMeters, spatialModes } from '../model';
import { ANALYSIS_POLL_MS } from '../useAnalysis';

type Analysis = ComponentProps<typeof AudioInspector>['analysis'];
type Chain = ComponentProps<typeof AudioInspector>['chain'];
const status = { playing: false, source: 'playback', paused: false, format: '48 kHz · 32-bit float · stereo', rms: 0, peak: 0, low: 0, mid: 0, high: 0, correlation: 0, balance: 0, vectorscope: [] as number[], spectrum: [] as number[] };
const noop = async () => {};
const analysis: Analysis = {
  native: true, status, history: [], levels: { current: status }, surface: { current: [] }, pollMs: ANALYSIS_POLL_MS, devices: [], output: '', outputName: '', rememberedMissing: false,
  inputs: [], loopbacks: [], input: '', inputName: '', rememberedInputMissing: false,
  driver: { installed: false, active: false, renderName: '', captureName: '', secureBoot: false, installer: '' },
  refreshDriver: noop, installDriver: noop, installing: false, installLog: '', installError: '',
  capture: { active: false, device: '', deviceName: '', loopback: false, error: null, monitor: { active: false, deviceName: '', error: null } },
  captureBusy: false, captureError: '', startCapture: noop, stopCapture: noop,
  monitor: true, setMonitor: () => {},
  spatial: defaultSpatial, setSpatial: noop,
  spatialOutput: { enabled: false, maxObjects: 0, reason: '' }, refreshSpatialOutput: noop, error: '',
  setOutput: () => {}, setInput: () => {}, refreshDevices: noop,
};
const chain: Chain = {
  native: true, chain: defaultChain, meters: idleMeters, eqCurve: idleEqResponse, error: '',
  presets: emptyPresets, patchError: '',
  setPanner: () => {}, setReverb: () => {}, setLimiter: () => {}, setCompressor: () => {}, setSaturator: () => {}, setOrder: () => {}, setStereoMode: () => {}, randomizeReverb: () => {},
  setEq: () => {}, updateFilter: () => {}, addFilter: () => {}, removeFilter: () => {},
  resetEq: () => {}, matchHeadroom: () => {},
  applyPreset: () => {}, renamePreset: async () => true, deletePreset: () => {},
};
const render = (source: Analysis, settings: Chain = chain, spatialPath = '') => renderToStaticMarkup(<AudioInspector
  analysis={source} chain={settings} theme="Ion" onTheme={() => {}} reducedMotion={false} onReducedMotion={() => {}}
  volume={0.4} onVolume={() => {}} track="" spatialPath={spatialPath}
/>);

describe('reference instrument stack', () => {
  it('renders the five reference instruments without fake processing', () => {
    const [panel, dialog] = render(analysis).split('<dialog');
    expect(panel).toContain('Spatialization');
    expect(panel).toContain('Spectrum');
    expect(panel).toContain('Level');
    expect(panel).toContain('Signal chain');
    expect(panel).toContain('Reverb');
    expect(panel).toContain('not LUFS');
    expect(panel).toContain('woven:4211');
    // The room reads as shaped settings: the number and the bar that draws it agree,
    // the decay is the time it is worth, and dry and wet are two separate levels.
    expect(panel).toMatch(/<dt>Decay<\/dt><dd><span class="readout-scale"[^>]*><i style="width:55%"/);
    expect(panel).toMatch(/<dt>Dry<\/dt><dd><span class="readout-scale"[^>]*><i style="width:100%"/);
    expect(panel).toMatch(/<dt>Wet<\/dt><dd><span class="readout-scale"[^>]*><i style="width:30%"/);
    expect(panel).toContain('2.2 s</span>');
    expect(panel).toContain('100 %</span>');
    expect(panel).toContain('30 %</span>');
    expect(panel).toContain('BYPASSED');
    expect(panel).toContain('48 kHz');
    // A bypassed limiter says so rather than reporting a reduction it never measured.
    expect(panel).toContain('>Limiter</span> bypassed');
    // One row per stage: the rail is too narrow for the sentence to stay on one line.
    expect(panel).toMatch(/<dl class="route-list readout[^"]*">/);
    // The source is the app's own playback, not a Windows mix captured elsewhere.
    expect(panel).toMatch(/<dt>Source<\/dt><dd[^>]*>Library playback<\/dd>/);
    expect(panel).not.toMatch(/Windows mix/);
    // The level is the playback level, reported as the setting and the dB it makes.
    expect(panel).toMatch(/<dt>Level<\/dt><dd[^>]*>40 % · −8\.0 dB<\/dd>/);
    expect(panel).toMatch(/<dt>Output<\/dt><dd[^>]*>Windows default<\/dd>/);
    expect(panel).not.toContain('chain-arrow');
    // Nothing to capture and nothing to monitor: the dialog sets the level and stops there.
    expect(dialog).toContain('id="playback-volume"');
    expect(dialog).not.toContain('id="monitor-device"');
    expect(dialog).not.toContain('id="monitor-gain"');
    expect(dialog).toMatch(/Devices page/);
    expect(dialog).not.toMatch(/Start capture|Stop capture/);
    expect(panel).not.toContain('Stop capture');
  });
  it('reports an audio error in the chain panel, with the long message in the dialog', () => {
    const [panel, dialog] = render({ ...analysis, error: 'Device unavailable: retry after reconnecting.' }).split('<dialog');
    // One state line: the error replaces the format spec rather than adding a row, so the
    // fixed-height panel cannot grow and the title strip is left to the window buttons.
    expect(panel).toContain('Audio error · View details');
    expect(panel).toContain('title="Device unavailable: retry after reconnecting."');
    expect(panel).not.toContain('32-bit float');
    // The full message is the line's own title, and the dialog repeats it where View details leads.
    expect(dialog).toContain('Device unavailable:');
    expect(dialog).toContain('Reduced motion');
    expect(dialog).toContain('aria-labelledby="scene-settings-title"');
  });
  it('names the output device playback renders to, not a monitor device', () => {
    const markup = render({ ...analysis, output: 'b', outputName: '1 - HDMI' });
    expect(markup).toMatch(/<dt>Output<\/dt><dd[^>]*>1 - HDMI<\/dd>/);
    expect(markup).toMatch(/<dt>Output<\/dt><dd[^>]*>1 - HDMI<\/dd>/);
  });
  it('names the input as the source while a capture is what the panels measure', () => {
    const markup = render({
      ...analysis,
      status: { ...status, playing: true, source: 'input', rms: 0.2, spectrum: Array.from({ length: 56 }, () => 0.2) },
      capture: { active: true, device: 'e', deviceName: 'Microphone (Audiorium)', loopback: false, error: null, monitor: { active: false, deviceName: '', error: null } },
    });
    expect(markup).toMatch(/<dt>Source<\/dt><dd[^>]*>Input capture<\/dd>/);
    expect(markup).toContain('Measuring Microphone (Audiorium), read in shared mode');
    expect(markup).not.toMatch(/<dt>Source<\/dt><dd[^>]*>Library playback<\/dd>/);
  });
  it('draws the live spectrum and level only from frames the engine wrote', () => {
    const spectrum = Array.from({ length: 56 }, (_, index) => (index === 20 ? 0.9 : 0.1));
    const markup = render({ ...analysis, status: { ...status, playing: true, rms: 0.2, peak: 0.8, spectrum } });
    expect(markup).toContain("Real FFT spectrum of the app&#x27;s output");
    expect(markup).toContain('LIVE');
    expect(markup).toContain('dBFS RMS');
    expect(markup).not.toContain('Spectrum idle');
  });
  it('draws the frames behind the live curve as a surface, newest on top', () => {
    const spectrum = Array.from({ length: 56 }, (_, index) => (index === 20 ? 0.9 : 0.1));
    const older = Array.from({ length: 56 }, () => 0.12);
    const markup = render({
      ...analysis,
      surface: { current: [older, older] },
      status: { ...status, playing: true, rms: 0.2, peak: 0.8, spectrum },
    });
    // Two past frames and the live one, each a filled ridge with its own glow.
    expect((markup.match(/url\(#spectrum-ridge-[0-9]+\)/g) ?? []).length).toBe(3);
    // Only the live frame carries an outline, and it is the last path drawn.
    expect((markup.match(/stroke="#eaf2ff"/g) ?? []).length).toBe(1);
    expect(markup.lastIndexOf('stroke="#eaf2ff"')).toBeGreaterThan(markup.lastIndexOf('url(#spectrum-ridge-2)'));
    expect(markup).toContain('with the last half second behind it');
  });

  it('says playback is idle, not that nothing is being captured', () => {
    const markup = render(analysis);
    expect(markup).toContain('IDLE');
    expect(markup).not.toContain('NO INPUT');
    expect(markup).not.toMatch(/Start capture|Stop capture|Capture source|MONITORING/);
  });
  it('does not expose native actions in browser preview', () => {
    const markup = render({ ...analysis, native: false });
    expect(markup).toContain('Playback runs in the Windows app');
    expect(markup).not.toContain('id="playback-volume"');
  });
});

describe('spatialization modes', () => {
  it('offers every mode as a lit position instead of a dropdown', () => {
    const markup = render(analysis);
    expect(markup).not.toContain('aria-label="Spatial mode" hidden');
    expect(markup).toContain('role="radiogroup" aria-label="Spatial mode"');
    for (const mode of spatialModes) {
      expect(markup).toContain(`role="radio"`);
      expect(markup).toContain(`aria-label="${mode.label}"`);
      expect(markup).toContain(`>${mode.short}<`);
    }
    expect(markup).toContain('aria-checked="true" aria-label="Stereo 2.0"');
    expect(markup).toContain('Pass-through. No spatial processing.');
  });
  it('describes the Atmos bed as Windows-rendered without claiming a decode', () => {
    const atmos = { ...defaultSpatial, mode: 'atmos' as const };
    const markup = render({
      ...analysis, spatial: atmos,
      spatialOutput: { enabled: true, maxObjects: 128, reason: '' },
    });
    expect(markup).toContain('A 7.1.4 bed from the mix, rendered by Dolby Atmos for Headphones or Home Theater in Windows.');
    expect(markup).toContain('aria-checked="true" aria-label="Dolby Atmos (Windows spatial)"');
    expect(markup).toContain('Windows spatial is on for this output');
    // The endpoint's own answer is reported, and the fallback is named when it applies.
    expect(markup).not.toContain('Width, crossfeed, height and rotation are on the Spatial field page');
    const off = render({ ...analysis, spatial: atmos }, chain, 'derived');
    expect(off).toContain('Spatial sound is off for this output');
    expect(off).toContain('This track is using the app’s own 7.1.4 render instead.');
  });
  it('leaves the field parameters to the page that draws them', () => {
    for (const mode of ['binaural', 'custom'] as const) {
      const markup = render({ ...analysis, spatial: { ...defaultSpatial, mode } });
      expect(markup).not.toContain('id="spatial-width"');
      expect(markup).not.toContain('id="spatial-crossfeed"');
      expect(markup).toContain('Width, crossfeed, height and rotation are on the Spatial field page');
    }
    expect(render({ ...analysis, spatial: { ...defaultSpatial, mode: 'stereo' } })).not.toContain('are on the Spatial field page');
  });
  it('marks the panel as ready until playback runs', () => {
    expect(render(analysis)).toContain('READY');
    expect(render({ ...analysis, status: { ...status, playing: true } })).toContain('ACTIVE');
    // A pause is reported as a pause, not as an idle panel: the readings are held.
    expect(render({ ...analysis, status: { ...status, paused: true } })).toContain('PAUSED');
  });
});
