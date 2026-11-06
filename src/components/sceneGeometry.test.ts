import { describe, expect, it } from 'vitest';
import configText from '../../src-tauri/tauri.conf.json?raw';
import capabilityText from '../../src-tauri/capabilities/default.json?raw';
import { createSoundField } from './sceneGeometry';

describe('sound observatory geometry', () => {
  it('produces deterministic finite positions and colors with a fixed population', () => {
    for (const layout of ['globe', 'disc', 'drift'] as const) {
      const data = createSoundField(100, layout);
      const bound = layout === 'drift' ? 22 : 3.5;
      expect(data.positions).toHaveLength(300);
      expect(data.colors).toHaveLength(300);
      expect(data).toEqual(createSoundField(100, layout));
      expect([...data.positions].every(value => Number.isFinite(value) && Math.abs(value) < bound)).toBe(true);
      expect([...data.colors].every(value => value >= 0 && value <= 1)).toBe(true);
    }
  });
  it('sends the outer drift past the inner disc so the field reaches the window edges', () => {
    const drift = createSoundField(600, 'drift').positions;
    const radii = Array.from({ length: drift.length / 3 }, (_, i) => Math.hypot(drift[i * 3], drift[i * 3 + 2]));
    expect(Math.min(...radii)).toBeGreaterThan(3.2);
    expect(Math.max(...radii)).toBeGreaterThan(8);
  });
  it('places globe particles on a bounded spherical shell', () => {
    const { positions } = createSoundField(400, 'globe');
    for (let i = 0; i < positions.length; i += 3) {
      const radius = Math.hypot(positions[i], positions[i + 1], positions[i + 2]);
      expect(radius).toBeGreaterThanOrEqual(2.04999);
      expect(radius).toBeLessThan(2.14);
    }
  });
});

describe('native title bar configuration', () => {
  it('disables native decorations while retaining a resizable desktop-sized window', () => {
    const config = JSON.parse(configText);
    expect(config.app.windows[0].decorations).toBe(false);
    expect(config.app.windows[0].minWidth).toBeGreaterThanOrEqual(760);
  });
  it('grants only main-window chrome actions and event subscription', () => {
    const capability = JSON.parse(capabilityText);
    expect(capability.windows).toEqual(['main']);
    expect(capability.permissions).toEqual([
      'core:window:allow-minimize', 'core:window:allow-toggle-maximize',
      'core:window:allow-is-maximized', 'core:window:allow-close',
      'core:window:allow-start-dragging',
      'core:event:allow-listen', 'core:event:allow-unlisten',
    ]);
  });
});
