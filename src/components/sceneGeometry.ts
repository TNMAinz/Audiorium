export type SoundFieldLayout = 'globe' | 'disc' | 'drift';

export function createSoundField(count: number, layout: SoundFieldLayout) {
  const positions = new Float32Array(count * 3);
  const colors = new Float32Array(count * 3);
  const sizes = new Float32Array(count);
  for (let i = 0; i < count; i++) {
    const angle = i * 2.3999632297;
    const fraction = (i + 0.5) / count;
    const variation = ((i * 73) % 101) / 101;
    if (layout === 'globe') {
      const height = 1 - 2 * fraction;
      const radius = 2.05 + variation * 0.08;
      const ring = Math.sqrt(1 - height * height);
      positions.set([Math.cos(angle) * ring * radius, height * radius, Math.sin(angle) * ring * radius], i * 3);
    } else if (layout === 'drift') {
      // Sparse outer dust that carries the field past the panels to every window edge.
      const radius = 3.3 + Math.pow(fraction, 0.55) * 17;
      const swirl = angle + radius * 0.42;
      positions.set([Math.cos(swirl) * radius, -1.08 + Math.sin(radius * 0.8 + swirl * 0.05) * 0.18 - fraction * 0.1, Math.sin(swirl) * radius], i * 3);
    } else {
      const band = (i % 18) / 18;
      const radius = 0.48 + Math.pow(band, 0.85) * 2.96 + variation * 0.025;
      const spiral = i * 2.3999632297 + radius * 0.9;
      positions.set([Math.cos(spiral) * radius, -1.1 + Math.sin(radius * 1.9 + spiral * 0.06) * 0.035, Math.sin(spiral) * radius], i * 3);
    }
    const glow = layout === 'globe' ? variation > 0.88 : variation > 0.82;
    const violet = !glow && variation > 0.64;
    colors.set(glow ? [1, 0.72, 0.45] : violet ? [0.62, 0.54, 1] : [0.4, 0.72 + variation * 0.18, 1], i * 3);
    sizes[i] = layout === 'globe'
      ? 0.05 + variation * 0.1
      : layout === 'drift'
        ? 0.022 + variation * 0.046
        : (0.026 + variation * 0.05) * (i % 18 < 6 ? 0.55 : 1);
  }
  return { positions, colors, sizes };
}
