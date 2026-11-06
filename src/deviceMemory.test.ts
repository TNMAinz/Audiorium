import { describe, expect, it } from 'vitest';
import {
  INPUT_DEVICE_KEY, MONITOR_KEY, OUTPUT_DEVICE_KEY, readInputDevice, readMonitor, readOutputDevice,
  usableInputDevice, usableOutputDevice, writeInputDevice, writeMonitor, writeOutputDevice,
} from './deviceMemory';

function memory(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value); },
    removeItem: (key: string) => { values.delete(key); },
    read: (key: string) => values.get(key) ?? null,
  };
}

const locked = {
  getItem: () => { throw new Error('storage blocked'); },
  setItem: () => { throw new Error('quota exceeded'); },
  removeItem: () => { throw new Error('storage blocked'); },
};

const dac = { id: '{0.0.0.00000000}.{dac}', name: 'Speakers (USB DAC)' };
const hdmi = { id: '{0.0.0.00000000}.{hdmi}', name: '1 - Monitor' };

describe('remembered output device', () => {
  it('starts on the Windows default when nothing was chosen', () => {
    expect(readOutputDevice(memory())).toBe('');
    expect(readOutputDevice(null)).toBe('');
    expect(readOutputDevice(undefined)).toBe('');
  });
  it('returns the device chosen last time', () => {
    expect(readOutputDevice(memory({ [OUTPUT_DEVICE_KEY]: dac.id }))).toBe(dac.id);
  });
  it('survives a storage that throws on read or write', () => {
    expect(readOutputDevice(locked)).toBe('');
    expect(() => writeOutputDevice(locked, dac.id)).not.toThrow();
    expect(() => writeOutputDevice(null, dac.id)).not.toThrow();
  });
  it('round-trips the device and forgets it when the default is chosen', () => {
    const storage = memory();
    writeOutputDevice(storage, dac.id);
    expect(storage.read(OUTPUT_DEVICE_KEY)).toBe(dac.id);
    expect(readOutputDevice(storage)).toBe(dac.id);
    writeOutputDevice(storage, '  ');
    expect(storage.read(OUTPUT_DEVICE_KEY)).toBeNull();
    expect(readOutputDevice(storage)).toBe('');
  });
});

describe('remembered input device', () => {
  const mic = { id: '{0.0.1.00000000}.{mic}', name: 'Microphone (USB)' };
  it('is remembered under its own key, so the two sides never overwrite each other', () => {
    const storage = memory();
    writeInputDevice(storage, mic.id);
    writeOutputDevice(storage, dac.id);
    expect(storage.read(INPUT_DEVICE_KEY)).toBe(mic.id);
    expect(storage.read(OUTPUT_DEVICE_KEY)).toBe(dac.id);
    expect(readInputDevice(storage)).toBe(mic.id);
    expect(readOutputDevice(storage)).toBe(dac.id);
  });
  it('starts on the Windows default and survives blocked storage', () => {
    expect(readInputDevice(memory())).toBe('');
    expect(readInputDevice(locked)).toBe('');
    expect(() => writeInputDevice(locked, mic.id)).not.toThrow();
  });
  it('is dropped when the endpoint is no longer present', () => {
    expect(usableInputDevice(mic.id, [mic])).toBe(mic.id);
    expect(usableInputDevice(mic.id, [])).toBe('');
  });
});

describe('remembered listening preference', () => {
  it('listens by default, so pressing Capture is heard', () => {
    expect(readMonitor(memory())).toBe(true);
    expect(readMonitor(null)).toBe(true);
    expect(readMonitor(locked)).toBe(true);
  });
  it('remembers listening being turned off, and turning it back on again', () => {
    const storage = memory();
    writeMonitor(storage, false);
    expect(storage.read(MONITOR_KEY)).toBe('off');
    expect(readMonitor(storage)).toBe(false);
    // On is the default, so it is stored as no key at all rather than as a value.
    writeMonitor(storage, true);
    expect(storage.read(MONITOR_KEY)).toBeNull();
    expect(readMonitor(storage)).toBe(true);
  });
  it('survives a storage that throws on read or write', () => {
    expect(() => writeMonitor(locked, false)).not.toThrow();
    expect(() => writeMonitor(undefined, false)).not.toThrow();
  });
});

describe('using a remembered device that is not here', () => {
  it('keeps a device that is present', () => {
    expect(usableOutputDevice(dac.id, [hdmi, dac])).toBe(dac.id);
  });
  it('falls back to the Windows default when the device is unplugged', () => {
    expect(usableOutputDevice(dac.id, [hdmi])).toBe('');
    expect(usableOutputDevice(dac.id, [])).toBe('');
  });
  it('treats no remembered device as the default, never as a match', () => {
    expect(usableOutputDevice('', [dac])).toBe('');
  });
});
