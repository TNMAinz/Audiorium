import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { SpatialFieldPage } from './SpatialFieldPage';
import { AnalysisPage } from './AnalysisPage';
import { SignalChainPage } from './SignalChainPage';
import { ProcessorsPage } from './ProcessorsPage';
import { MasterPage } from './MasterPage';
import { EqualiserPage } from './EqualiserPage';
import { DevicesPage } from './DevicesPage';
import { LibraryPage } from './LibraryPage';
import { defaultChain, defaultSpatial, emptyPresets, idleEqResponse, idleLoudness, idleMeters, idleRender, idleScan, type MasterFormat } from '../model';
import { ANALYSIS_POLL_MS, normalizeCapture, type useAnalysis } from '../useAnalysis';
import type { useLibrary } from '../hooks';
import type { useChain } from '../useChain';
import type { usePlayer } from '../usePlayer';
import type { useRender } from '../useRender';

type Audio = ReturnType<typeof useAnalysis>;
type Library = ReturnType<typeof useLibrary>;
type Player = ReturnType<typeof usePlayer>;

const status = { playing: false, source: 'playback', paused: false, format: '48 kHz · 32-bit float · stereo', rms: 0, peak: 0, low: 0, mid: 0, high: 0, correlation: 0, balance: 0, vectorscope: [] as number[], spectrum: [] as number[] };
const noop = async () => {};
const driver = { installed: false, active: false, renderName: '', captureName: '', secureBoot: false, installer: '' };
const monitor = { active: false, deviceName: '', error: null };
const capture = { active: false, device: '', deviceName: '', loopback: false, error: null, monitor };
const audio: Audio = {
  native: true, status, history: [], levels: { current: status }, surface: { current: [] }, pollMs: ANALYSIS_POLL_MS, devices: [], output: '', outputName: 'Windows default', rememberedMissing: false,
  inputs: [], loopbacks: [], input: '', inputName: '', rememberedInputMissing: false,
  driver, refreshDriver: noop, installDriver: noop, installing: false, installLog: '', installError: '',
  capture, captureBusy: false, captureError: '', startCapture: noop, stopCapture: noop,
  monitor: true, setMonitor: () => {},
  spatial: defaultSpatial, setSpatial: noop,
  spatialOutput: { enabled: false, maxObjects: 0, reason: '' }, refreshSpatialOutput: noop, error: '',
  setOutput: () => {}, setInput: () => {}, refreshDevices: noop,
};
const library: Library = {
  native: true, path: '', setPath: () => {}, result: null, busy: false, error: '',
  scan: noop, browse: noop, clear: () => {},
};
const player: Player = {
  native: true, busy: false, hasQueue: false, error: '',
  status: {
    active: false, playing: false, finished: false, title: '', artist: '', album: '', path: '',
    format: '', device: '', position: 0, duration: 0, volume: 0.7, spatial: '', error: null,
  },
  shuffle: false, repeat: 'off', toggleShuffle: () => {}, cycleRepeat: () => {},
  play: noop, toggle: noop, next: noop, previous: noop, seek: noop, setVolume: noop, stop: noop,
};
const chain: ReturnType<typeof useChain> = {
  native: true, chain: defaultChain, meters: idleMeters, eqCurve: idleEqResponse, error: '',
  presets: emptyPresets, patchError: '',
  setPanner: () => {}, setReverb: () => {}, setLimiter: () => {}, setCompressor: () => {}, setSaturator: () => {}, setEffect: () => {}, setOrder: () => {}, setStereoMode: () => {}, randomizeReverb: () => {},
  setEq: () => {}, updateFilter: () => {}, addFilter: () => {}, removeFilter: () => {},
  resetEq: () => {}, matchHeadroom: () => {},
  applyPreset: () => {}, renamePreset: async () => true, deletePreset: () => {},
};
const speakers = [
  { azimuth: -30, elevation: 0, gain: 0.7, source: 'Left' },
  { azimuth: 30, elevation: 0, gain: 0.7, source: 'Right' },
  { azimuth: 0, elevation: 0, gain: 0.56, source: 'Mid' },
  { azimuth: 0, elevation: 45, gain: 0.35, source: 'High' },
];

describe('spatial field workspace', () => {
  it('draws the speakers the renderer reports', () => {
    const markup = renderToStaticMarkup(<SpatialFieldPage audio={audio} layout={speakers} />);
    expect(markup).toContain('4 SPEAKERS');
    expect(markup).toContain('1 HEIGHT CHANNEL');
    // A trim is a signed dB change; a linear gain above zero must never print "+-".
    expect(markup).not.toContain('+-');
    expect(markup).toContain('−3.1 dB');
    expect(markup).toContain('Mid');
    expect(markup).toContain('Derived sources');
  });
  it('explains stereo pass-through instead of drawing nothing', () => {
    const markup = renderToStaticMarkup(<SpatialFieldPage audio={audio} layout={[]} />);
    expect(markup).toContain('No virtual speakers');
    expect(markup).toContain('left stays left and right stays right');
  });
  it('places the manual field by hand on the page, not in the settings dialog', () => {
    const manual = renderToStaticMarkup(<SpatialFieldPage audio={{ ...audio, spatial: { ...defaultSpatial, mode: 'custom' } }} layout={speakers} />);
    for (const knob of ['Width', 'Crossfeed', 'Height', 'Rotation']) expect(manual).toContain(`aria-label="${knob}"`);
    // The readout is the renderer's own layout: two speakers on the plane, two overhead.
    expect(manual).toContain('pair −30° / +30°');
    expect(manual).toContain('crossfeed 35 %');
    expect(manual).toContain('constant-power normalised');
  });
  it('names every routing and marks the one in use, so pass-through is not a blank column', () => {
    const passThrough = renderToStaticMarkup(<SpatialFieldPage audio={audio} layout={[]} />);
    for (const label of ['Stereo 2.0', '5.1 virtual', '7.1 virtual', 'Dolby Atmos (Windows spatial)', 'Binaural', 'Manual field']) {
      expect(passThrough).toContain(label);
    }
    // Six routings, one of them the running one, each with what it actually derives.
    expect((passThrough.match(/role="radio"/g) ?? []).length).toBe(6);
    expect(passThrough).toMatch(/aria-checked="true" aria-label="Stereo 2.0: Pass-through/);
    expect(passThrough).toContain('WHAT EACH ROUTING DOES');
    // The list is the panel's own scroll region, so a short window scrolls it instead of
    // clipping the last routing.
    expect(passThrough).toContain('class="scroll-area mode-list"');
    expect(passThrough).toContain('role="radiogroup" aria-label="Spatial mode"');
    const binaural = renderToStaticMarkup(<SpatialFieldPage audio={{ ...audio, spatial: { ...defaultSpatial, mode: 'binaural' } }} layout={speakers} />);
    expect(binaural).toMatch(/aria-checked="true" aria-label="Binaural: Head shadow/);
    expect((binaural.match(/aria-checked="true"/g) ?? []).length).toBe(1);
  });
  it('offers only the controls a fixed layout can use', () => {
    // Binaural keeps its ±30° pair, so width and height would be lies: only crossfeed and rotation apply.
    const binaural = renderToStaticMarkup(<SpatialFieldPage audio={{ ...audio, spatial: { ...defaultSpatial, mode: 'binaural' } }} layout={speakers} />);
    expect(binaural).toContain('aria-label="Crossfeed"');
    expect(binaural).toContain('aria-label="Rotation"');
    expect(binaural).not.toContain('aria-label="Width"');
    expect(binaural).not.toContain('aria-label="Height"');
    // Atmos is Windows' fixed 7.1.4 static bed: no placement controls of its own,
    // unlike the app-rendered surround layouts that can be turned.
    const atmos = renderToStaticMarkup(<SpatialFieldPage audio={{ ...audio, spatial: { ...defaultSpatial, mode: 'atmos' } }} layout={speakers} />);
    expect(atmos).not.toContain('aria-label="Rotation"');
    expect(atmos).not.toContain('aria-label="Crossfeed"');
    expect(atmos).toContain('Windows spatial');
    // Pass-through has no field to shape, so it carries no controls at all.
    const stereo = renderToStaticMarkup(<SpatialFieldPage audio={audio} layout={[]} />);
    expect(stereo).not.toContain('aria-label="Rotation"');
    expect(stereo).not.toContain('aria-label="Crossfeed"');
  });
});

describe('analysis workspace', () => {
  it('states the real FFT facts and stays blank while stopped', () => {
    const markup = renderToStaticMarkup(<AnalysisPage audio={audio} history={[]} />);
    expect(markup).toContain('FFT 2048 · Hann');
    expect(markup).toContain('56 bands');
    expect(markup).toContain('30 Hz – 16 kHz');
    expect(markup).toContain('IDLE');
    expect(markup).not.toContain('PEAK BAND');
  });
  it('reports the loudest band and band levels from the output it measured', () => {
    const spectrum = Array.from({ length: 56 }, (_, index) => (index === 20 ? 0.9 : 0.05));
    const live = { ...audio, status: { ...status, playing: true, rms: 0.2, peak: 0.5, low: 0.3, mid: 0.2, high: 0.1, spectrum } };
    const markup = renderToStaticMarkup(<AnalysisPage audio={live} history={[{ rms: 0.1, peak: 0.4 }, { rms: 0.2, peak: 0.5 }]} />);
    expect(markup).toContain('PEAK BAND');
    expect(markup).toContain('LIVE');
    expect(markup).toContain('−14.0 dBFS');
    expect(markup).toContain('polyline');
  });
  it('reads the stereo image the output measured, not a guess at the pair', () => {
    const spectrum = Array.from({ length: 56 }, (_, index) => (index === 20 ? 0.9 : 0.05));
    // Two points of a mono pair (mid up, no side) and one panned right.
    const vectorscope = [0.4, 0, 0.2, 0, 0.1, 0.3];
    const live = {
      ...audio,
      status: { ...status, playing: true, rms: 0.2, peak: 0.5, correlation: 0.87, balance: -0.25, vectorscope, spectrum },
    };
    const markup = renderToStaticMarkup(<AnalysisPage audio={live} history={[{ rms: 0.2, peak: 0.5 }]} />);
    expect(markup).toContain('Stereo image');
    expect(markup).toContain('GONIOMETER');
    expect(markup).toContain('+0.87');
    expect(markup).toContain('MONO-COMPATIBLE');
    expect(markup).toContain('25 % LEFT');
    expect(markup).toContain('aria-valuenow="0.87"');
    expect(markup).toContain('aria-valuenow="-0.25"');
    // The cloud is drawn on a canvas, so the markup carries its box and its count, not
    // hundreds of nodes the poll would have to reconcile.
    expect(markup).toContain('class="goniometer-cloud"');
    expect(markup).toContain('3 points from the window just measured');
  });
  it('shows no correlation while nothing is leaving the app', () => {
    const markup = renderToStaticMarkup(<AnalysisPage audio={audio} history={[]} />);
    expect(markup).toContain('NO OUTPUT');
    expect(markup).toContain('aria-valuenow="0"');
    expect(markup).not.toContain('MONO-COMPATIBLE');
    expect(markup).toContain('Goniometer idle');
  });
  it('freezes the last frame on a pause and reports it as paused, not idle', () => {
    const spectrum = Array.from({ length: 56 }, (_, index) => (index === 20 ? 0.9 : 0.05));
    const paused = {
      ...audio,
      status: { ...status, paused: true, rms: 0.2, peak: 0.5, low: 0.3, mid: 0.2, high: 0.1, correlation: 0.87, balance: -0.25, vectorscope: [0.4, 0, 0.1, 0.3], spectrum },
    };
    const markup = renderToStaticMarkup(<AnalysisPage audio={paused} history={[{ rms: 0.2, peak: 0.5 }]} />);
    // The page says paused, and the plot and its readouts keep the window they stopped on.
    expect(markup).toContain('PAUSED');
    expect(markup).toContain('paused');
    expect(markup).not.toContain('IDLE');
    expect(markup).toContain('Spectrum paused');
    expect(markup).toContain('Goniometer paused');
    expect(markup).toContain('PEAK BAND');
    expect(markup).toContain('−14.0 dBFS');
    expect(markup).toContain('+0.87');
    expect(markup).toContain('MONO-COMPATIBLE');
    expect(markup).toContain('25 % LEFT');
    expect(markup).not.toContain('NO OUTPUT');
  });
  it('names the chosen input when the capture is what the panels measure', () => {
    const spectrum = Array.from({ length: 56 }, (_, index) => (index === 20 ? 0.9 : 0.05));
    const capturing = {
      ...audio,
      status: { ...status, playing: true, source: 'input', rms: 0.2, peak: 0.5, spectrum },
      capture: { active: true, device: 'e', deviceName: 'Microphone (Audiorium)', loopback: false, error: null, monitor },
    };
    const markup = renderToStaticMarkup(<AnalysisPage audio={capturing} history={[{ rms: 0.2, peak: 0.5 }]} />);
    // The header, the band caption, the trend and the spectrum all say what they measure.
    expect(markup).toContain('input capture');
    expect(markup).toContain('INPUT RMS');
    expect(markup).not.toContain('OUTPUT RMS');
    expect(markup).toContain('Measured from the chosen input, in shared mode');
    expect(markup).toContain('Real FFT spectrum of the chosen input');
    expect(markup).toContain('Goniometer of the chosen input');
  });
});

describe('signal chain workspace', () => {
  it('names every stage with the values in use', () => {
    const live = { ...audio, status: { ...status, playing: true }, outputName: '1 - HDMI' };
    const markup = renderToStaticMarkup(<SignalChainPage audio={live} chain={chain} track="Says" volume={0.4} onVolume={() => {}} onOpenProcessors={() => {}} onOpenEq={() => {}} />);
    // The source is the decoded file, and the output is the device playback renders to.
    expect(markup).toContain('Says');
    expect(markup).toContain('1 - HDMI');
    expect(markup).toContain('Decoded on this machine');
    // 0.4 linear is −8 dB, and the page says both.
    expect(markup).toContain('40 % · −8.0 dB');
    expect(markup).toContain('Spatial renderer');
    expect(markup).toContain('All bypassed');
    // The processors have a workspace of their own, so the page links to it.
    expect(markup).toContain('>Processors</button>');
    expect(markup).toContain('the Windows mix is never tapped. Capture, when it runs, reads one chosen input.');
  });
  it('names the processors that are actually in the path', () => {
    const active: ReturnType<typeof useChain> = {
      ...chain,
      chain: {
        ...defaultChain,
        panner: { ...defaultChain.panner, enabled: true, azimuth: 45, elevation: 20 },
        reverb: { ...defaultChain.reverb, enabled: true, wet: 0.4 },
        limiter: { ...defaultChain.limiter, enabled: true, thresholdDb: -6 },
      },
    };
    const markup = renderToStaticMarkup(<SignalChainPage audio={audio} chain={active} track="" volume={0.7} onVolume={() => {}} onOpenProcessors={() => {}} onOpenEq={() => {}} />);
    expect(markup).toContain('Panner 45° / 20°');
    expect(markup).toContain('Reverb 40% wet');
    expect(markup).toContain('Limiter -6.0 dB');
    expect(markup).toContain('3 active');
    // An equaliser that is off is bypassed, not "in path".
    expect(markup).toContain('BYPASSED');
  });
  it('counts the equaliser in the path once it has a band', () => {
    const bands = [{ kind: 'peaking' as const, frequency: 1_200, gainDb: 4.5, q: 1.4, enabled: true }];
    const active: ReturnType<typeof useChain> = {
      ...chain,
      chain: { ...defaultChain, eq: { enabled: true, preampDb: -4.5, filters: bands } },
    };
    const markup = renderToStaticMarkup(<SignalChainPage audio={audio} chain={active} track="" volume={0.7} onVolume={() => {}} onOpenProcessors={() => {}} onOpenEq={() => {}} />);
    expect(markup).toContain('Equaliser · 1 band');
    expect(markup).toContain('1 band · preamp −4.5 dB');
    expect(markup).toContain('1 active');
    // The page reports the stage and hands the bands to their own workspace.
    expect(markup).toContain('>Equaliser</button>');
    expect(markup).not.toContain('class="eq-card');
  });
});

describe('processors workspace', () => {
  it('is a page of the path, with all six stages and the meter bridge on it', () => {
    const markup = renderToStaticMarkup(<ProcessorsPage chain={chain} onOpenEq={() => {}} />);
    expect(markup).toContain('aria-labelledby="workspace-title"');
    expect(markup).toContain('>Processors<');
    expect(markup).toContain('All 6 bypassed · bit-exact pass-through');
    expect(markup).toContain('48 kHz · stereo');
    for (const index of ['01', '02', '03', '04', '05', '06']) expect(markup).toContain(`>${index}<`);
    expect(markup).toContain('Compressor');
    expect(markup).toContain('Saturator');
    expect(markup).toContain('Meter bridge');
  });
  it('is a workspace, not a dialog, and hands the equaliser to its own page', () => {
    const markup = renderToStaticMarkup(<ProcessorsPage chain={chain} onOpenEq={() => {}} />);
    expect(markup).toContain('class="page-grid processors-grid"');
    expect(markup).toContain('>Equaliser</button>');
    expect(markup).not.toContain('<dialog');
  });
});

describe('equaliser workspace', () => {
  it('opens on the bands the engine is running', () => {
    const bands = [{ kind: 'peaking' as const, frequency: 1_200, gainDb: 4.5, q: 1.4, enabled: true }];
    const active: ReturnType<typeof useChain> = {
      ...chain,
      chain: { ...defaultChain, eq: { enabled: true, preampDb: -4.5, filters: bands } },
      eqCurve: { frequencies: [20, 1_000, 20_000], gains: [0.2, 4.4, 0.1], maxBoostDb: 4.5, headroomDb: 0 },
    };
    const markup = renderToStaticMarkup(<EqualiserPage chain={active} />);
    expect(markup).toContain('1 band · preamp −4.5 dB · 48 kHz stereo · playback');
    expect(markup).toContain('1 of 24 bands · preamp −4.5 dB');
    expect(markup).toContain('eq-card engaged');
    expect(markup).toContain('Band 1, Peaking, 1.20 kHz');
    expect(markup).toContain('aria-label="Band type"');
  });
  it('says the stage is out of the path when it is', () => {
    const markup = renderToStaticMarkup(<EqualiserPage chain={chain} />);
    expect(markup).toContain('Bypassed');
    expect(markup).toContain('eq-card bypassed');
  });
});

describe('devices workspace', () => {
  const devices = [
    { id: 'a', name: 'Speakers (USB)', isDefault: true },
    { id: 'b', name: '1 - HDMI', isDefault: false },
  ];
  it('marks the output device playback is using and names the Windows default', () => {
    const markup = renderToStaticMarkup(<DevicesPage audio={{ ...audio, devices, status: { ...status, playing: true }, output: 'b', outputName: '1 - HDMI' }} />);
    expect(markup).toContain('2 FOUND');
    expect(markup).toContain('Windows default');
    expect(markup).toContain('Output');
    expect(markup).toContain('LIVE');
    // The device playback is rendering to is the chosen one, not the Windows default.
    expect(markup).toMatch(/aria-pressed="true" aria-label="Use 1 - HDMI for playback"/);
  });
  it('plays to the Windows default until another device is chosen', () => {
    const markup = renderToStaticMarkup(<DevicesPage audio={{ ...audio, devices }} />);
    expect(markup).toMatch(/aria-pressed="true" aria-label="Use Speakers \(USB\) for playback"/);
  });
  it('says so when Windows returns no output devices', () => {
    const markup = renderToStaticMarkup(<DevicesPage audio={audio} />);
    expect(markup).toContain('No devices listed');
  });
  it('shows why a list could not be read, instead of an empty list that looks empty', () => {
    const markup = renderToStaticMarkup(<DevicesPage audio={{ ...audio, error: 'list_audio_inputs not found' }} />);
    expect(markup).toContain('list_audio_inputs not found');
    expect(markup).toContain('role="alert"');
  });
  it('says when the output remembered from last time is not connected', () => {
    const markup = renderToStaticMarkup(<DevicesPage audio={{ ...audio, devices, rememberedMissing: true }} />);
    expect(markup).toContain('The output chosen last time is not connected');
    expect(markup).toMatch(/aria-pressed="true"[^>]*aria-label="Use Speakers \(USB\) for playback"/);
  });
  it('surfaces why a device could not be opened', () => {
    const markup = renderToStaticMarkup(<DevicesPage audio={{ ...audio, devices }} error="Cannot open the playback output (it may be in exclusive use)" />);
    expect(markup).toContain('exclusive use');
    expect(markup).toContain('role="alert"');
  });

  const inputs = [
    { id: 'c', name: 'Microphone (USB)', isDefault: true },
    { id: 'd', name: 'Microphone (Audiorium)', isDefault: false },
  ];
  it('draws the input side with the same anatomy as the output side', () => {
    const markup = renderToStaticMarkup(<DevicesPage audio={{ ...audio, devices, inputs }} />);
    // Two lists, the same row, the same count, a label that says which side it is.
    expect(markup).toContain('aria-label="Output devices"');
    expect(markup).toContain('aria-label="Input devices"');
    expect((markup.match(/FOUND/g) ?? []).length).toBe(2);
    expect(markup).toMatch(/aria-pressed="true" aria-label="Use Microphone \(USB\) as input"/);
    expect(markup).toMatch(/aria-pressed="true" aria-label="Use Speakers \(USB\) for playback"/);
    // The chosen input is the Windows default until another is picked, and it is a choice
    // of any device, not of one vendor's.
    expect(markup).toContain('Microphone (Audiorium)');
    expect(markup).toContain('>Input<');
  });
  it('keeps the page to what it does, without the long descriptions', () => {
    const markup = renderToStaticMarkup(<DevicesPage audio={{ ...audio, devices, inputs }} />);
    for (const note of [
      'Playback renders to one output device',
      'Capture reads the device chosen here',
      'The analysis panels measure the frames this app wrote',
      'Listening plays the captured frames',
    ]) expect(markup).not.toContain(note);
    // The optional device still says what installing it does, because that one is a
    // decision with consequences.
    expect(markup).toContain('Optional.');
  });
  it('toggles the capture from the route card and names what it is measuring', () => {
    const capturing = { active: true, device: 'd', deviceName: 'Microphone (Audiorium)', loopback: false, error: null, monitor };
    const markup = renderToStaticMarkup(<DevicesPage audio={{ ...audio, devices, inputs, capture: capturing, status: { ...status, playing: true, source: 'input' } }} />);
    expect(markup).toContain('CAPTURING');
    expect(markup).toMatch(/<dt>Source<\/dt><dd[^>]*>Input capture<\/dd>/);
    expect(markup).toMatch(/<dt>Input<\/dt><dd[^>]*>Microphone \(Audiorium\)<\/dd>/);
    expect(markup).toMatch(/aria-label="Stop capturing the input"/);
    expect(markup).toContain('capturing Microphone (Audiorium)');
    // The row of the endpoint actually being read says so, not just the button.
    expect(markup).toContain('<em class="output">Capturing</em>');
  });
  it('says why a capture could not start', () => {
    const markup = renderToStaticMarkup(<DevicesPage audio={{ ...audio, devices, inputs, captureError: 'Cannot open the chosen input (it may be in exclusive use)' }} />);
    expect(markup).toContain('exclusive use');
    expect(markup).toContain('role="alert"');
  });
  it('leaves the choice of source to the user, with or without the Audiorium device', () => {
    const markup = renderToStaticMarkup(<DevicesPage audio={{ ...audio, devices, inputs, input: 'd', inputName: 'Microphone (Audiorium)' }} />);
    expect(markup).toMatch(/aria-pressed="true" aria-label="Use Microphone \(Audiorium\) as input"/);
    expect(markup).toMatch(/<dt>Input<\/dt><dd[^>]*>Microphone \(Audiorium\)<\/dd>/);
  });
  it('offers every output as a loopback source, so an app can stay on its own device', () => {
    const loopbacks = devices.map(device => ({ ...device, isDefault: false, loopback: true }));
    const markup = renderToStaticMarkup(<DevicesPage audio={{ ...audio, devices, inputs, loopbacks }} />);
    // The same output appears on both sides: once as where sound goes, once as what
    // can be read back, and the rows say which they are.
    expect(markup).toContain('aria-label="Capture what Speakers (USB) is playing"');
    expect(markup).toMatch(/aria-label="Use 1 - HDMI for playback"/);
    expect(markup).toContain('<em class="idle">Loopback</em>');
    expect(markup).toContain('2 loopbacks');
  });
  it('says when a captured source is a loopback of an output', () => {
    const loopbacks = devices.map(device => ({ ...device, isDefault: false, loopback: true }));
    const capture = { active: true, device: 'b', deviceName: '1 - HDMI', loopback: true, error: null, monitor };
    const markup = renderToStaticMarkup(<DevicesPage audio={{ ...audio, devices, inputs, loopbacks, capture, input: 'b', status: { ...status, playing: true, source: 'input' } }} />);
    expect(markup).toContain('capturing 1 - HDMI (loopback)');
    expect(markup).toMatch(/<dt>Input<\/dt><dd[^>]*>1 - HDMI \(loopback\)<\/dd>/);
    expect(markup).toMatch(/aria-pressed="true" aria-label="Capture what 1 - HDMI is playing"/);
  });
  it('offers the driver as an optional install and says what this machine has', () => {
    const markup = renderToStaticMarkup(<DevicesPage audio={{ ...audio, devices, inputs }} />);
    expect(markup).toContain('Audiorium device');
    expect(markup).toContain('NOT INSTALLED');
    expect(markup).toContain('aria-label="Install the Audiorium audio device"');
    expect(markup).toContain('Optional.');
    expect(markup).toContain('every device Windows already lists works exactly as it does now');
  });
  it('reports the device as loaded once both endpoints run, and never asks twice', () => {
    const active = { ...driver, installed: true, active: true, renderName: 'Speakers (Audiorium)', captureName: 'Microphone (Audiorium)' };
    const markup = renderToStaticMarkup(<DevicesPage audio={{ ...audio, devices, inputs, driver: active }} />);
    expect(markup).toContain('ACTIVE');
    expect(markup).toContain('Speakers (Audiorium)');
    expect(markup).toContain('Microphone (Audiorium)');
    expect(markup).toContain('Endpoints running');
  });
  it('says Secure Boot will refuse the driver, and keeps the installer’s log when it fails', () => {
    const markup = renderToStaticMarkup(<DevicesPage audio={{
      ...audio, devices, inputs,
      driver: { ...driver, installed: true, secureBoot: true },
      installError: 'The installer reported a failure; its log is below.',
      installLog: 'bcdedit: Access is denied.',
    }} />);
    expect(markup).toContain('Secure Boot is on');
    expect(markup).toContain('Waiting for a reboot');
    expect(markup).toContain('Installer output');
    expect(markup).toContain('bcdedit: Access is denied.');
    expect(markup).toContain('role="alert"');
  });
  it('says when the input remembered from last time is not connected', () => {
    const markup = renderToStaticMarkup(<DevicesPage audio={{ ...audio, devices, inputs, rememberedInputMissing: true }} />);
    expect(markup).toContain('The input chosen last time is not connected');
  });
  it('plays what it captures to an output and names it, so a capture is heard', () => {
    const playing = { ...monitor, active: true, deviceName: 'Speakers (USB)' };
    const capturing = { active: true, device: 'd', deviceName: 'Microphone (Audiorium)', loopback: false, error: null, monitor: playing };
    const markup = renderToStaticMarkup(<DevicesPage audio={{ ...audio, devices, inputs, capture: capturing, status: { ...status, playing: true, source: 'input' } }} />);
    expect(markup).toMatch(/aria-pressed="true"[^>]*aria-label="Play the captured input to the chosen output"/);
    expect(markup).toMatch(/<dt>Monitor<\/dt><dd[^>]*>Speakers \(USB\)<\/dd>/);
    expect(markup).toContain('Listen: on');
  });
  it('says why listening was refused rather than leaving the capture silent', () => {
    const refused = { active: false, deviceName: '', error: 'Monitoring is off: playing this output back into itself would feed back.' };
    const capturing = { active: true, device: 'b', deviceName: '1 - HDMI', loopback: true, error: null, monitor: refused };
    const markup = renderToStaticMarkup(<DevicesPage audio={{ ...audio, devices, inputs, capture: capturing }} />);
    expect(markup).toContain('would feed back');
    expect(markup).toContain('role="alert"');
    expect(markup).toMatch(/<dt>Monitor<\/dt><dd[^>]*>Not playing<\/dd>/);
  });
  it('still draws the page when the engine predates listening', () => {
    // A running app with an older engine answers without `monitor`. That must read as
    // "not listening"; a missing field used to take the whole window blank.
    const older = { active: true, device: 'd', deviceName: 'Microphone (USB)', loopback: false, error: null } as unknown as typeof capture;
    expect(normalizeCapture(older).monitor).toEqual({ active: false, deviceName: '', error: null });
    expect(normalizeCapture(null).active).toBe(false);
    const markup = renderToStaticMarkup(<DevicesPage audio={{ ...audio, devices, inputs, capture: older }} />);
    expect(markup).toContain('Current route');
    expect(markup).toMatch(/<dt>Monitor<\/dt><dd[^>]*>Not playing<\/dd>/);
  });
  it('meters without playing when listening is off, and says so', () => {
    const markup = renderToStaticMarkup(<DevicesPage audio={{ ...audio, devices, inputs, monitor: false }} />);
    expect(markup).toContain('aria-pressed="false" aria-label="Play the captured input to the chosen output"');
    expect(markup).toContain('Listen: off');
    expect(markup).toMatch(/<dt>Monitor<\/dt><dd[^>]*>Off<\/dd>/);
  });
});

describe('library workspace', () => {
  it('asks for a folder before anything is scanned', () => {
    const markup = renderToStaticMarkup(<LibraryPage library={library} player={player} />);
    expect(markup).toContain('Nothing scanned yet');
    expect(markup).toContain('Symphonia, in app');
    expect(markup).toContain('IDLE');
    expect(markup).toContain('No folder scanned');
  });
  it('lists scanned tracks with the tags read from the files', () => {
    const result = {
      root: 'D:\\Music\\Ambient', skipped: 2, truncated: false,
      tracks: [
        { path: 'D:\\Music\\Ambient\\a.flac', fileName: 'a.flac', title: 'Says', artist: 'Nils Frahm', album: 'Spaces', format: 'FLAC', bytes: 24_000_000, modified: 1_700_000_000 },
        { path: 'D:\\Music\\Ambient\\b.wav', fileName: 'b.wav', title: 'b', artist: '', album: '', format: 'WAV', bytes: 900_000, modified: 0 },
      ],
    };
    const markup = renderToStaticMarkup(<LibraryPage library={{ ...library, path: result.root, result }} player={player} />);
    expect(markup).toContain('Ambient');
    expect(markup).toContain('2 TRACKS');
    expect(markup).toContain('Nils Frahm');
    expect(markup).toContain('24.0 MB');
    expect(markup).toContain('2 skipped');
    expect(markup).toContain('track-row playable');
    expect(markup).toContain('Play Says');
  });
  it('surfaces scan errors instead of inventing tracks', () => {
    const markup = renderToStaticMarkup(<LibraryPage library={{ ...library, error: 'That folder cannot be read' }} player={player} />);
    expect(markup).toContain('That folder cannot be read');
    expect(markup).not.toContain('track-row');
  });
  it('lets a remembered folder that no longer reads be forgotten', () => {
    // The launch scan can fail on a moved drive; Clear must still be reachable, or the
    // bad path would be retried on every launch with no way to remove it.
    const markup = renderToStaticMarkup(<LibraryPage library={{ ...library, path: 'Z:/gone', error: 'That folder cannot be read' }} player={player} />);
    expect(markup).toContain('aria-label="Clear library"');
    expect(renderToStaticMarkup(<LibraryPage library={library} player={player} />)).not.toContain('aria-label="Clear library"');
  });
});

describe('master workspace', () => {
  const renderStub = (status = idleRender, format: MasterFormat = 'wav'): ReturnType<typeof useRender> => ({
    native: true, status, error: '', source: '', format, setFormat: () => {}, rate: '48',
    setRate: () => {}, destination: '', chooseSource: () => {}, start: async () => {},
    startAlbum: async () => {}, cancel: async () => {},
    scan: idleScan, scanError: '', scanFile: async () => {},
    nullReport: null, nullError: '', nullTest: async () => {}, measureReference: async () => null,
  });
  const track = {
    path: '/music/Track.flac', fileName: 'Track.flac', title: 'Track', artist: 'Artist',
    album: 'Album', format: 'FLAC', bytes: 1_000_000, modified: 0,
  };
  const libraryStub = {
    ...({} as ReturnType<typeof useLibrary>),
    result: { root: '/music', tracks: [track], skipped: 0, truncated: false },
  };
  const playerStub = { ...({} as ReturnType<typeof usePlayer>), play: async () => {} };
  const renderPage = (status = idleRender, library = libraryStub, format: MasterFormat = 'wav') => renderToStaticMarkup(
    <MasterPage library={library} player={playerStub} chain={chain} render={renderStub(status, format)} />,
  );

  it('is a page with a source, an export and the before/after pair on it', () => {
    const markup = renderPage();
    expect(markup).toContain('>Master<');
    expect(markup).toContain('48 kHz · stereo');
    expect(markup).toContain('>Source<');
    expect(markup).toContain('>Export<');
    expect(markup).toContain('>Before / after<');
    // The card that exports also says where the file will go, and the list of
    // tracks it can come from is on the page.
    expect(markup).toContain('Choose a track to see where its master goes.');
    expect(markup).toContain('aria-label="Tracks to master"');
    // Nothing has been measured, so every figure is a dash rather than a zero.
    expect(markup).toContain('—');
    expect(markup).not.toContain('0.0 LUFS');
  });

  it('offers the depth switch and refuses an export with no source', () => {
    const markup = renderPage();
    expect(markup).toContain('aria-label="Bit depth"');
    expect(markup).toContain('>16-bit PCM<');
    expect(markup).toContain('>32-bit float<');
    expect(markup).toContain('title="Choose a track first"');
  });

  it('reports the pair the engine measured and lets the master be heard', () => {
    const measured = {
      ...idleLoudness,
      integratedLufs: -14.2, loudnessRange: 6.1, truePeakDb: -1.0, samplePeakDb: -1.2,
      dcOffsetDb: -60, correlation: 0.42, blocks: 40,
    };
    const done = {
      ...idleRender, running: false, phase: 'done', progress: 1, frames: 2_400_000,
      seconds: 50, bytes: 9_600_044, destination: '/music/Track (mastered).wav',
      depth: '24-bit PCM', source: { ...measured, integratedLufs: -18.6 }, output: measured,
    };
    const markup = renderPage(done);
    expect(markup).toContain('−18.6 LUFS');
    expect(markup).toContain('−14.2 LUFS');
    expect(markup).toContain('−1.0 dBTP');
    expect(markup).toContain('aria-label="Compare"');
    expect(markup).toContain('>Source<');
    expect(markup).toContain('>Master<');
    expect(markup).toContain('24-bit PCM');
  });

  it('offers WAV and FLAC, and drops the float depth a FLAC cannot carry', () => {
    const wav = renderPage();
    expect(wav).toContain('aria-label="Container"');
    expect(wav).toContain('>WAV<');
    expect(wav).toContain('>FLAC<');
    expect(wav).toContain('>32-bit float<');
    const flac = renderPage(idleRender, libraryStub, 'flac');
    expect(flac).toContain('24-bit PCM FLAC');
    expect(flac).toContain('aria-label="Bit depth"');
    expect(flac).not.toContain('>32-bit float<');
  });

  it('judges the master against the delivery target it is aimed at', () => {
    const measured = {
      ...idleLoudness,
      integratedLufs: -11.7, loudnessRange: 6.1, truePeakDb: -2.0, samplePeakDb: -2.2,
      dcOffsetDb: -62, correlation: 0.31, blocks: 40,
    };
    const done = {
      ...idleRender, running: false, phase: 'done', progress: 1, frames: 2_400_000,
      seconds: 50, bytes: 9_600_044, destination: '/music/Track (mastered).wav',
      depth: '24-bit PCM', source: { ...measured, integratedLufs: -18.6 }, output: measured,
    };
    const loud = renderPage(done);
    // Streaming is the default target: -14 LUFS and a -1 dBTP ceiling.
    expect(loud).toContain('aria-label="Delivery target"');
    expect(loud).toContain('>Streaming<');
    expect(loud).toContain('2.3 LU loud');
    expect(loud).toContain('→ −2.3 dB');
    expect(loud).toContain('lands on −14.0 LUFS');

    const onTarget = renderPage({ ...done, output: { ...measured, integratedLufs: -14.1, truePeakDb: -1.4 } });
    expect(onTarget).toContain('Meets −14.0 LUFS, −1.0 dBTP');
    expect(onTarget).not.toContain('LU loud');

    // Before a render there is nothing to pass, and the page says so rather than
    // showing a green tick over a dash.
    expect(renderPage()).toContain('Render to check a target');
  });

  it('says where to start when nothing has been scanned', () => {
    const empty = { ...libraryStub, result: { root: '', tracks: [], skipped: 0, truncated: false } };
    const markup = renderPage(idleRender, empty);
    expect(markup).toContain('No tracks yet');
    expect(markup).toContain('Scan a folder on the Library page first');
  });

  it('writes at the rate the page chooses, not only at 48 kHz', () => {
    const markup = renderPage();
    expect(markup).toContain('aria-label="Sample rate"');
    expect(markup).toContain('>44.1 kHz<');
    expect(markup).toContain('>48 kHz<');
    expect(markup).toContain('>96 kHz<');
    // The header names the rate the master will carry, so it is never a surprise.
    expect(markup).toContain('24-bit PCM WAV · 48 kHz');
  });

  it('offers an album when two tracks belong to one', () => {
    const second = { ...track, path: '/music/Second.flac', fileName: 'Second.flac', title: 'Second' };
    const two = { ...libraryStub, result: { root: '/music', tracks: [track, second], skipped: 0, truncated: false } };
    const markup = renderPage(idleRender, two);
    expect(markup).toContain('Artist · Album');
    expect(markup).toContain('2 tracks');
    expect(markup).toContain('Master album');
    // The album is a sequence written by the rack as it stands, and its button says so.
    expect(markup).toContain('in order, as one programme');

    // One track is not an album, so the card stays away rather than offering a queue of one.
    expect(renderPage()).not.toContain('Master album');
  });

  it('measures a reference and draws the whole file', () => {
    const markup = renderToStaticMarkup(<MasterPage
      library={libraryStub}
      player={playerStub}
      chain={chain}
      render={{
        ...renderStub(),
        scan: { ...idleScan, frames: 144_000, seconds: 3, envelope: [0.5, 0.4], loudness: [-9, -10], columns: 2, bands: 2, spectrum: [0.2, 0.5, 0.3, 0.6] },
        nullReport: { frames: 144_000, differing: 0, maxDifferenceDb: -120 },
      }}
    />);
    expect(markup).toContain('aria-label="Reference track"');
    expect(markup).toContain('>Whole file<');
    expect(markup).toContain('Read the file');
    // A bypassed rack that changed nothing says so, and a difference would say which.
    expect(markup).toContain('Bypass test');
    expect(markup).toContain('Identical · 144,000 frames');
    expect(markup).toContain('Peak envelope over 3.0 seconds');
    expect(markup).toContain('Loudness over time, in LUFS');
    expect(markup).toContain('Spectrogram, 2 columns of 2 bands');
    // The timeline is drawn wider than the card and moved sideways: one column is
    // four pixels, so two columns are an 8 px drawing the track can pan.
    expect(markup).toContain('file-scan-track');
    expect(markup).toContain('width:8px');
    expect(markup).toContain('Whole file: peak envelope, loudness and spectrogram over 3.0 seconds');
  });
});
