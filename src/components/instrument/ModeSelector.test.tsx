import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { ModeSelector } from './ModeSelector';
import { spatialModes } from '../../model';

const render = (value: 'stereo' | 'atmos', compact = false) =>
  renderToStaticMarkup(<ModeSelector value={value} onChange={() => {}} compact={compact} />);

describe('spatial mode selector', () => {
  it('is a radio bank, not a dropdown', () => {
    const markup = render('stereo');
    expect(markup).not.toContain('<select');
    expect(markup).toContain('role="radiogroup" aria-label="Spatial mode"');
    for (const mode of spatialModes) expect(markup).toContain(`aria-label="${mode.label}"`);
    expect((markup.match(/role="radio"/g) ?? []).length).toBe(spatialModes.length);
    expect(markup).toContain('aria-checked="true" aria-label="Stereo 2.0"');
    // One position in the tab order; the arrow keys walk the rest.
    expect((markup.match(/tabindex="0"/g) ?? []).length).toBe(1);
  });
  it('shows short labels in the compact bank and keeps the full name for assistive tech', () => {
    const markup = render('atmos', true);
    expect(markup).toContain('>Atmos<');
    expect(markup).not.toContain('>Dolby Atmos (Windows spatial)<');
    expect(markup).toContain('aria-label="Dolby Atmos (Windows spatial)"');
    expect(markup).toContain('mode-selector compact');
  });
});
