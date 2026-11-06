/**
 * The devices last chosen here, remembered between launches: one for playback out
 * and one for the input side.
 *
 * An empty value is a real choice — Windows' default — and is stored as no key at
 * all, so a device that was once picked cannot come back by accident. Device ids
 * are the endpoint ids Windows reports, so a remembered id only means something on
 * the machine that made it; a machine that no longer has that endpoint falls back
 * to the default instead of playing to a device that is not there.
 */
export const OUTPUT_DEVICE_KEY = 'audiorium.output-device';
export const INPUT_DEVICE_KEY = 'audiorium.input-device';
/** Whether a capture is played back to an output. Listening is the default, because
 * pressing Capture is a request to hear the input; choosing off meters silently. */
export const MONITOR_KEY = 'audiorium.monitor';

interface Readable { getItem(key: string): string | null }
interface Writable {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

/** An unreadable value means no device was remembered, never a guess. */
export function readOutputDevice(storage: Readable | null | undefined): string {
  return readChoice(storage, OUTPUT_DEVICE_KEY);
}
export function readInputDevice(storage: Readable | null | undefined): string {
  return readChoice(storage, INPUT_DEVICE_KEY);
}

function readChoice(storage: Readable | null | undefined, key: string): string {
  try {
    return (storage?.getItem(key) ?? '').trim();
  } catch {
    // A locked-down WebView can throw on access; the default is the safe reading.
    return '';
  }
}

export function writeOutputDevice(storage: Writable | null | undefined, id: string): void {
  writeChoice(storage, OUTPUT_DEVICE_KEY, id);
}
export function writeInputDevice(storage: Writable | null | undefined, id: string): void {
  writeChoice(storage, INPUT_DEVICE_KEY, id);
}

export function readMonitor(storage: Readable | null | undefined): boolean {
  return readChoice(storage, MONITOR_KEY) !== 'off';
}
export function writeMonitor(storage: Writable | null | undefined, listening: boolean): void {
  // On is the default, so it is stored as no key at all.
  writeChoice(storage, MONITOR_KEY, listening ? '' : 'off');
}

function writeChoice(storage: Writable | null | undefined, key: string, id: string): void {
  try {
    const value = id.trim();
    if (value) storage?.setItem(key, value);
    else storage?.removeItem?.(key);
  } catch {
    // Storage can be blocked or full; the session keeps working either way.
  }
}

/**
 * The remembered id when that endpoint is present right now, otherwise Windows'
 * default. A device that is merely unplugged keeps its memory, so reconnecting it
 * brings the choice back on the next launch.
 */
export function usableOutputDevice(remembered: string, present: readonly { id: string }[]): string {
  if (!remembered) return '';
  return present.some(device => device.id === remembered) ? remembered : '';
}

/** The same reading for the input side: an endpoint that is not here cannot be chosen. */
export function usableInputDevice(remembered: string, present: readonly { id: string }[]): string {
  return usableOutputDevice(remembered, present);
}

/** The WebView's own storage, or null when it is unavailable. */
export function localOutputStorage(): Storage | null {
  return localChoiceStorage();
}
export function localInputStorage(): Storage | null {
  return localChoiceStorage();
}

function localChoiceStorage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}
