import { describe, expect, it } from 'vitest';
import { ANALYSIS_POLL_MS, ANALYSIS_REDUCED_POLL_MS, SURFACE_SPAN_MS, TRACE_SPAN_MS, historyFrames, normalizeCapture, pollInterval, reopensCapture, surfaceFrames } from './useAnalysis';

const request = { input: '{0.0.1}.{mic}', output: '{0.0.0}.{dac}', listen: true };

describe('a capture status from any build', () => {
  it('reads an engine that predates listening as not listening', () => {
    const older = { active: true, device: 'd', deviceName: 'Microphone (USB)', loopback: false, error: null };
    expect(normalizeCapture(older)).toEqual({ ...older, monitor: { active: false, deviceName: '', error: null } });
  });
  it('keeps what the engine says about listening, and fills the rest', () => {
    const listening = {
      active: true, device: 'd', deviceName: 'Microphone (USB)', loopback: false, error: null,
      monitor: { active: true, deviceName: 'Speakers (USB)' },
    };
    expect(normalizeCapture(listening).monitor).toEqual({ active: true, deviceName: 'Speakers (USB)', error: null });
  });
  it('is an idle capture when nothing answered at all', () => {
    expect(normalizeCapture(null)).toEqual({
      active: false, device: '', deviceName: '', loopback: false, error: null,
      monitor: { active: false, deviceName: '', error: null },
    });
  });
});

describe('choosing a device while a capture runs', () => {
  it('re-opens the capture when the endpoint changes', () => {
    expect(reopensCapture(true, request, { ...request, input: '{0.0.1}.{other}' })).toBe(true);
  });
  it('re-opens it when the output it is heard on changes', () => {
    expect(reopensCapture(true, request, { ...request, output: '{0.0.0}.{headphones}' })).toBe(true);
  });
  it('re-opens it when listening is turned on or off', () => {
    expect(reopensCapture(true, request, { ...request, listen: false })).toBe(true);
  });
  it('leaves a choice alone when nothing asks for a different stream', () => {
    expect(reopensCapture(true, request, { ...request })).toBe(false);
  });
  it('does nothing while no capture runs: the choice is simply the next one used', () => {
    expect(reopensCapture(false, request, { input: '', output: '', listen: false })).toBe(false);
  });
});

describe('how often the panels ask', () => {
  it('asks at the live rate, and a fifth of it when motion is reduced', () => {
    expect(pollInterval(false)).toBe(ANALYSIS_POLL_MS);
    expect(pollInterval(true)).toBe(ANALYSIS_REDUCED_POLL_MS);
    // Still live: ten readings a second, not a held frame.
    expect(pollInterval(true)).toBeGreaterThan(0);
    expect(1000 / pollInterval(true)).toBeGreaterThanOrEqual(8);
  });
  it('keeps the trace and the spectrum tail the same stretch of time at either rate', () => {
    for (const interval of [ANALYSIS_POLL_MS, ANALYSIS_REDUCED_POLL_MS]) {
      // Whole points can only land on a whole interval, so the span is within one of them.
      expect(Math.abs(historyFrames(interval) * interval - TRACE_SPAN_MS)).toBeLessThan(interval);
      expect(Math.abs(surfaceFrames(interval) * interval - SURFACE_SPAN_MS)).toBeLessThanOrEqual(interval);
    }
    // The live rate is the span the panels were built at: 600 points, 32 layers.
    expect(historyFrames(ANALYSIS_POLL_MS)).toBe(600);
    expect(surfaceFrames(ANALYSIS_POLL_MS)).toBe(32);
    // A slower poll draws fewer layers rather than a longer tail.
    expect(surfaceFrames(ANALYSIS_REDUCED_POLL_MS)).toBeLessThan(surfaceFrames(ANALYSIS_POLL_MS));
    // A nonsense interval still leaves something to draw.
    expect(historyFrames(0)).toBeGreaterThan(1);
    expect(surfaceFrames(0)).toBeGreaterThan(1);
  });
});
