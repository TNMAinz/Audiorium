import { describe, expect, it } from 'vitest';
import { themes, workspaces } from './model';

describe('workspace and scene configuration', () => {
  it('defines accessible copy for every navigation destination', () => {
    expect(Object.keys(workspaces)).toEqual(['Orbit', 'Spectrum', 'Flow', 'Processors', 'Eq', 'Spatial', 'Library', 'Devices']);
    for (const item of Object.values(workspaces)) {
      expect(item.title.length).toBeGreaterThan(0);
      expect(item.description.length).toBeGreaterThan(0);
    }
  });
  it('defines valid primary and secondary colors for each scene theme', () => {
    for (const theme of Object.values(themes)) {
      expect(theme.primary).toMatch(/^#[0-9a-f]{6}$/i);
      expect(theme.secondary).toMatch(/^#[0-9a-f]{6}$/i);
    }
  });
});
