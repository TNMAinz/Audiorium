import { Canvas, useFrame, useThree } from '@react-three/fiber';
import { Component, memo, useEffect, useMemo, useRef, useState, type ReactNode, type RefObject } from 'react';
import * as THREE from 'three';
import type { AnalysisStatus } from '../useAnalysis';
import { coverLayout, easeCursor, shelfMoving, type CoverNode, type GlobeMotion } from './globeLibrary';
import { themes, type Theme } from '../model';

interface Props {
  theme: Theme;
  reducedMotion: boolean;
  active: boolean;
  /** The window is in the tray: no frame of this scene can be seen. */
  suspended: boolean;
  /** Sound is coming out, so the scene is at its liveliest and is drawn every frame. */
  playing: boolean;
  levels: RefObject<AnalysisStatus>;
  motion: RefObject<GlobeMotion>;
  nodes: RefObject<Map<number, CoverNode>>;
  count: number;
  selected: number | null;
  requestFrame: RefObject<(() => void) | null>;
}

const pointVertex = `
  attribute float aSize;
  attribute vec3 color;
  uniform float uTime;
  uniform float uTravel;
  uniform float uEnergy;
  varying vec3 vColor;
  varying float vAlpha;
  void main() {
    float a = uTime * uTravel;
    vec3 p = position;
    p.xz = mat2(cos(a), -sin(a), sin(a), cos(a)) * p.xz;
    float twinkle = 0.82 + 0.18 * sin(uTime * 2.1 + p.x * 2.7 + p.z * 1.9);
    vColor = color;
    vAlpha = (0.55 + uEnergy * 0.45) * (0.88 + 0.12 * twinkle);
    vec4 view = modelViewMatrix * vec4(p, 1.0);
    gl_PointSize = clamp(aSize * 780.0 * twinkle / max(1.0, -view.z), 0.9, 12.0);
    gl_Position = projectionMatrix * view;
  }
`;
const pointFragment = `
  varying vec3 vColor;
  varying float vAlpha;
  void main() {
    float d = length(gl_PointCoord - 0.5) * 2.0;
    if (d > 1.0) discard;
    float light = pow(1.0 - d, 1.7);
    gl_FragColor = vec4(vColor * 1.9, light * vAlpha);
  }
`;
const discVertex = `
  varying vec2 vUv;
  void main() {
    vUv = uv;
    gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
  }
`;
// A radial spectrum: the glow brightens per angle, so the disc plays with the music.
const discFragment = `
  uniform float uTime;
  uniform float uEnergy;
  uniform sampler2D uSpectrum;
  uniform vec3 uInner;
  uniform vec3 uOuter;
  varying vec2 vUv;
  void main() {
    vec2 p = vUv * 2.0 - 1.0;
    float r = length(p);
    if (r > 1.0) discard;
    float sector = fract((atan(p.y, p.x) + 3.14159265) / 6.2831853);
    // Bass at the top, treble at the bottom, mirrored so the disc has no seam.
    float folded = sector < 0.5 ? sector * 2.0 : (1.0 - sector) * 2.0;
    float band = texture2D(uSpectrum, vec2(folded, 0.5)).r;
    // A resting shimmer keeps the disc alive when nothing is playing.
    float idle = 0.24 + 0.06 * sin(uTime * 1.2 + folded * 12.566);
    float level = max(band * 1.35, idle) + uEnergy * 0.4;

    float base = smoothstep(1.0, 0.0, r) * (0.26 + level * 0.62) * smoothstep(0.02, 0.85, r);
    float sun = pow(max(0.0, 0.28 - r) / 0.28, 2.0) * (0.55 + level * 1.0);

    float alpha = (base + sun) * smoothstep(1.0, 0.9, r);
    vec3 color = mix(uInner, uOuter, smoothstep(0.08, 0.96, r));
    gl_FragColor = vec4(color * (1.0 + level * 0.9), alpha);
  }
`;
const filamentVertex = `
  attribute float aSeed;
  attribute float aAlong;
  uniform float uTime;
  uniform float uEnergy;
  uniform float uLow;
  uniform float uRadius;
  uniform sampler2D uSpectrum;
  uniform vec3 uWarm;
  uniform vec3 uCool;
  varying vec3 vColor;
  varying float vFade;
  vec3 spin(vec3 p, vec3 axis, float angle) {
    float c = cos(angle);
    float s = sin(angle);
    return p * c + cross(axis, p) * s + axis * dot(axis, p) * (1.0 - c);
  }
  void main() {
    float sa = fract(aSeed * 97.13);
    float sb = fract(aSeed * 41.71);
    float sc = fract(aSeed * 13.37);
    vec3 axis = normalize(vec3(sin(sa * 6.283) + 1e-4, cos(sb * 6.283), sin(sc * 6.283)));
    vec3 start = normalize(vec3(sin(sb * 12.9), cos(sc * 7.3) + 1e-4, sin(sa * 9.7)));
    float sweep = (0.6 + sc * 1.5) * 3.14159265;
    // The strands flow faster as the track gets louder, so the shell never sits still mid-song.
    float travel = aAlong * sweep + uTime * (0.05 + sa * 0.1) * (1.0 + uEnergy * 1.8);
    vec3 dir = spin(start, axis, travel);
    float wobble = 0.014 + uEnergy * 0.06;
    dir += wobble * vec3(
      sin(aAlong * 9.0 + uTime * 0.7 + sa * 6.283),
      sin(aAlong * 7.0 - uTime * 0.9 + sb * 6.283),
      sin(aAlong * 11.0 + uTime * 0.5 + sc * 6.283));
    dir = normalize(dir);
    // Which band of the live spectrum this point of the shell sits on. The direction
    // is warped before it is projected onto a slowly turning axis, so a low note
    // throws a torn, striated front across the shell instead of one round cap, and
    // each stitch walks through its own slice of the spectrum along its length, so no
    // two neighbours answer in lockstep.
    vec3 warped = normalize(dir + vec3(
      sin(dir.y * 3.1 + uTime * 0.37 + sa * 6.283),
      sin(dir.z * 2.7 - uTime * 0.29 + sb * 6.283),
      sin(dir.x * 3.3 + uTime * 0.33 + sc * 6.283)) * 0.55);
    vec3 tuning = normalize(vec3(sin(uTime * 0.16 + 0.7), 0.42, cos(uTime * 0.16 + 0.7)));
    float scatter = (fract(sa * 53.7) - 0.5) * 0.36 + (fract(aAlong * 7.7 + sa * 13.1) - 0.5) * 0.26;
    float spread = clamp(dot(warped, tuning) * 0.5 + 0.5 + scatter, 0.0, 1.0);
    float band = texture2D(uSpectrum, vec2(spread, 0.5)).r;
    // Stretched across the spectrum's own range: a quiet band pulls the strands in,
    // a loud one throws them out, so the lobes are visible rather than a uniform glow.
    float shaped = smoothstep(0.30, 0.84, band);
    // Crests run along each strand; the exponent sharpens them into the cusps the
    // reference field has, so a loud band reads as a fold rather than a fat blob.
    float crest = pow(max(0.0, sin(aAlong * 9.4248 - uTime * (2.4 + uEnergy * 3.0) + sa * 6.2831853)), 6.0);
    // Outward room is scarce — the shell has to stay inside the globe — so a loud
    // band pushes a little and a quiet one pulls in more.
    float push = shaped - 0.42;
    float lift = push * (push > 0.0 ? 0.2 : 0.34) + crest * shaped * 0.03 + uEnergy * 0.02;
    float radius = uRadius * (0.94 + sa * 0.1) * (1.0 + 0.02 * sin(aAlong * 3.14159265) + lift);
    radius = min(radius, 1.78);
    vec3 tint = mix(uWarm, uCool, clamp(dir.y * 0.5 + 0.5 + (sa - 0.5) * 0.5, 0.0, 1.0));
    vColor = mix(tint, vec3(1.0), crest * shaped * 0.34);
    vFade = (0.085 + shaped * 0.2 + crest * shaped * 0.42 + uEnergy * 0.09) * (1.0 - abs(aAlong - 0.5) * 0.8);
    gl_Position = projectionMatrix * modelViewMatrix * vec4(dir * radius, 1.0);
  }
`;
const filamentFragment = `
  varying vec3 vColor;
  varying float vFade;
  void main() {
    gl_FragColor = vec4(vColor, vFade);
  }
`;
// A rim-lit shell keeps the sphere luminous even where its grid is faint.
const rimVertex = `
  varying vec3 vNormal;
  varying vec3 vView;
  void main() {
    vec4 view = modelViewMatrix * vec4(position, 1.0);
    vNormal = normalize(normalMatrix * normal);
    vView = normalize(-view.xyz);
    gl_Position = projectionMatrix * view;
  }
`;
const rimFragment = `
  varying vec3 vNormal;
  varying vec3 vView;
  uniform vec3 uTint;
  uniform float uStrength;
  void main() {
    float rim = pow(1.0 - abs(dot(normalize(vNormal), normalize(vView))), 2.4);
    gl_FragColor = vec4(uTint * rim * 1.4, rim * uStrength);
  }
`;

function random(seed: number) {
  let value = seed;
  return () => (value = value * 16807 % 2147483647) / 2147483647;
}

function paint(width: number, height: number, draw: (context: CanvasRenderingContext2D, width: number, height: number) => void) {
  const canvas = document.createElement('canvas');
  canvas.width = width; canvas.height = height;
  const context = canvas.getContext('2d');
  if (context) draw(context, width, height);
  const map = new THREE.CanvasTexture(canvas);
  map.colorSpace = THREE.SRGBColorSpace;
  map.anisotropy = 4;
  return map;
}

/** Equirectangular latitude/longitude web with stippled nodes, drawn once. */
function gridTexture() {
  return paint(2048, 1024, (context, width, height) => {
    context.fillStyle = '#000';
    context.fillRect(0, 0, width, height);
    context.globalCompositeOperation = 'lighter';
    // Violet at the crown, cyan through the body, deep blue at the base.
    const tint = context.createLinearGradient(0, 0, 0, height);
    tint.addColorStop(0, 'rgba(172,126,255,.62)');
    tint.addColorStop(0.34, 'rgba(112,168,255,.5)');
    tint.addColorStop(0.56, 'rgba(78,214,255,.55)');
    tint.addColorStop(1, 'rgba(40,120,235,.34)');
    for (let lon = 0; lon < 360; lon += 5) {
      const major = lon % 30 === 0;
      context.globalAlpha = major ? 1 : 0.34;
      context.strokeStyle = tint;
      context.lineWidth = major ? 2.2 : 1;
      const x = lon / 360 * width;
      context.beginPath(); context.moveTo(x, 0); context.lineTo(x, height); context.stroke();
    }
    // Parallels stay evenly faint: a brighter one reads as a ring cutting across the
    // globe rather than as part of its web.
    for (let lat = 0; lat <= 180; lat += 5) {
      const major = lat % 30 === 0;
      context.globalAlpha = major ? 0.42 : 0.26;
      context.strokeStyle = tint;
      context.lineWidth = major ? 1.4 : 1;
      const y = lat / 180 * height;
      context.beginPath(); context.moveTo(0, y); context.lineTo(width, y); context.stroke();
    }
    context.globalAlpha = 1;
    const rand = random(9137);
    for (let at = 0; at < 5600; at++) {
      const x = rand() * width, y = rand() * height;
      const violet = y < height * 0.45;
      context.fillStyle = violet
        ? `rgba(${150 + rand() * 90 | 0},${120 + rand() * 70 | 0},255,${0.1 + rand() * 0.32})`
        : `rgba(${90 + rand() * 90 | 0},${180 + rand() * 70 | 0},255,${0.1 + rand() * 0.32})`;
      context.fillRect(x, y, 1.5, 1.5);
    }
    for (let at = 0; at < 340; at++) {
      const x = Math.floor(rand() * 72) / 72 * width;
      const y = Math.floor(rand() * 36) / 36 * height;
      context.fillStyle = 'rgba(186,232,255,.7)';
      context.beginPath(); context.arc(x, y, 1.6 + rand() * 2.2, 0, Math.PI * 2); context.fill();
    }
  });
}

/**
 * The whole sky as one wrap-around map. Drawn on a sphere that surrounds the camera
 * it has no edge to find in any direction, so the universe keeps going whichever way
 * the library is turned.
 */
function skyTexture() {
  const width = 2048;
  const height = 1024;
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext('2d') as CanvasRenderingContext2D;
  const paintSky = () => {
    context.fillStyle = '#000';
    context.fillRect(0, 0, width, height);
    const rand = random(4127);
    // The galactic band is a sine of longitude, so it meets itself across the seam.
    const bandAt = (x: number) => height * 0.54 + Math.sin(x / width * Math.PI * 2) * height * 0.13;
    // Anything drawn near the seam is drawn on both sides of it, as well.
    const wrapped = (x: number, draw: (cx: number) => void) => { draw(x); draw(x - width); draw(x + width); };
    const cloud = (x: number, y: number, size: number, tint: string, alpha: number) => wrapped(x, cx => {
      const gradient = context.createRadialGradient(cx, y, 0, cx, y, size);
      gradient.addColorStop(0, tint + alpha.toFixed(3) + ')');
      gradient.addColorStop(1, 'rgba(0,0,0,0)');
      context.fillStyle = gradient;
      context.fillRect(cx - size, y - size, size * 2, size * 2);
    });
    context.globalCompositeOperation = 'lighter';

    // Unresolved starlight hugging the band.
    for (let at = 0; at < 2600; at++) {
      const x = rand() * width;
      const y = bandAt(x) + (rand() + rand() + rand() - 1.5) * height * 0.09;
      const size = 26 + rand() * 140;
      const tint = rand() > 0.86 ? `rgba(255,${196 + rand() * 40 | 0},${168 + rand() * 40 | 0},`
        : rand() > 0.5 ? `rgba(${186 + rand() * 50 | 0},${206 + rand() * 40 | 0},255,`
          : `rgba(${150 + rand() * 70 | 0},${176 + rand() * 60 | 0},255,`;
      cloud(x, y, size, tint, 0.01 + rand() * 0.018);
    }

    // Colour clouds at four longitudes, so the sky is not one uniform wash.
    // Enough colour clouds that whichever way the library is turned, three or four of
    // them are in view: an even ring of them, each at its own latitude and temperature.
    const tints = ['rgba(86,140,255,', 'rgba(166,110,255,', 'rgba(72,178,255,', 'rgba(214,132,255,', 'rgba(255,156,120,', 'rgba(96,164,255,'];
    for (let cluster = 0; cluster < 18; cluster++) {
      const centre = (cluster / 18 + (rand() - 0.5) * 0.02) * width;
      const latitude = height * (0.2 + rand() * 0.6);
      const reach = 90 + rand() * 50;
      const tint = tints[cluster % tints.length];
      const spread = (value: number) => Math.pow(value, 0.45);
      for (let blob = 0; blob < 300; blob++) {
        const angle = rand() * Math.PI * 2;
        const radius = spread(rand()) * reach;
        cloud(centre + Math.cos(angle) * radius * 1.5,
          latitude + Math.sin(angle) * radius * 0.75 + Math.sin(radius * 0.01) * 30,
          10 + rand() * 56, tint, 0.016 + rand() * 0.018);
      }
    }

    // Two distant galaxies, one on each side of the sky.
    for (const [at, y, size, tint] of [[0.19, 0.2, 90, 'rgba(214,224,255,'], [0.71, 0.76, 64, 'rgba(255,222,196,']] as const) {
      const centre = at * width;
      for (let arm = 0; arm < 2; arm++) {
        for (let step = 0; step < 260; step++) {
          const turn = step / 260;
          const angle = turn * Math.PI * 3.1 + arm * Math.PI;
          const radius = size * (0.08 + turn * 0.85);
          cloud(centre + Math.cos(angle) * radius, y * height + Math.sin(angle) * radius * 0.4,
            1.6 + rand() * 3.4, tint, 0.18 - turn * 0.12);
        }
      }
      cloud(centre, y * height, size * 0.24, 'rgba(255,250,238,', 0.5);
    }

    // A broad, very faint haze underneath everything gives the sky depth instead of
    // a flat black with bright patches on it.
    for (let at = 0; at < 260; at++) {
      cloud(rand() * width, rand() * height, 200 + rand() * 260,
        rand() > 0.5 ? 'rgba(70,120,230,' : 'rgba(150,110,255,', 0.003 + rand() * 0.005);
    }

    // The galactic bulge glows warmer and denser.
    const bulge = context.createRadialGradient(width * 0.52, height * 0.54, 0, width * 0.52, height * 0.54, height * 0.3);
    bulge.addColorStop(0, 'rgba(255,228,192,.12)');
    bulge.addColorStop(0.5, 'rgba(255,192,152,.045)');
    bulge.addColorStop(1, 'rgba(0,0,0,0)');
    context.fillStyle = bulge;
    context.fillRect(0, 0, width, height);

    // No individual suns in this map: at this resolution a painted star becomes a
    // soft blob several pixels of sky wide. The scene's own star field supplies those.

    // The poles of an equirectangular map are squeezed to a point, and the sampling
    // there turns into a woven moiré. Fade the top and bottom rows out, so the sky
    // reads as continuous in every direction without that veil over it.
    context.globalCompositeOperation = 'destination-in';
    const poles = context.createLinearGradient(0, 0, 0, height);
    poles.addColorStop(0, 'rgba(0,0,0,0)');
    poles.addColorStop(0.16, 'rgba(0,0,0,1)');
    poles.addColorStop(0.84, 'rgba(0,0,0,1)');
    poles.addColorStop(1, 'rgba(0,0,0,0)');
    context.fillStyle = poles;
    context.fillRect(0, 0, width, height);

    // Dust lanes bite dark rifts out of the band, as in deep sky photographs.
    context.globalCompositeOperation = 'destination-out';
    for (let at = 0; at < 220; at++) {
      const x = rand() * width;
      const y = bandAt(x) + (rand() - 0.5) * height * 0.06;
      const size = 18 + rand() * 80;
      wrapped(x, cx => {
        const rift = context.createRadialGradient(cx, y, 0, cx, y, size);
        rift.addColorStop(0, `rgba(0,0,0,${0.35 + rand() * 0.45})`);
        rift.addColorStop(1, 'rgba(0,0,0,0)');
        context.fillStyle = rift;
        context.fillRect(cx - size, y - size, size * 2, size * 2);
      });
    }
  };
  paintSky();
  // The 2D engine dithers its gradients pixel by pixel, and magnified over the whole
  // sky that dither reads as a woven veil. Averaging it down to half resolution, with
  // a touch of blur, keeps the clouds and drops the veil.
  const half = document.createElement('canvas');
  half.width = width / 2;
  half.height = height / 2;
  const smoothed = half.getContext('2d') as CanvasRenderingContext2D;
  smoothed.imageSmoothingEnabled = true;
  smoothed.imageSmoothingQuality = 'high';
  smoothed.filter = 'blur(1px)';
  smoothed.drawImage(canvas, 0, 0, half.width, half.height);
  const map = new THREE.CanvasTexture(half);
  map.colorSpace = THREE.SRGBColorSpace;
  map.anisotropy = 4;
  map.minFilter = THREE.LinearFilter;
  map.generateMipmaps = false;
  return map;
}

function glowTexture(inner: string, middle: string) {
  return paint(256, 256, (context, size) => {
    const gradient = context.createRadialGradient(size / 2, size / 2, 0, size / 2, size / 2, size / 2);
    gradient.addColorStop(0, inner);
    gradient.addColorStop(0.22, middle);
    gradient.addColorStop(1, 'rgba(0,0,0,0)');
    context.fillStyle = gradient;
    context.fillRect(0, 0, size, size);
  });
}

/** Spectral colours from hot blue-white stars through white, yellow and orange to red dwarfs. */
const STARS: number[][] = [
  [0.64, 0.77, 1], [0.76, 0.85, 1], [0.88, 0.93, 1], [1, 0.98, 0.92],
  [1, 0.92, 0.76], [1, 0.82, 0.6], [1, 0.68, 0.46], [0.86, 0.72, 1],
];

function particles(kind: 'stars' | 'globe' | 'orbit', count: number) {
  const positions = new Float32Array(count * 3);
  const colors = new Float32Array(count * 3);
  const sizes = new Float32Array(count);
  const rand = random(kind === 'stars' ? 71 : kind === 'globe' ? 872 : 413);
  for (let at = 0; at < count; at++) {
    const angle = rand() * Math.PI * 2;
    if (kind === 'stars') {
      const radius = 16 + rand() * 22;
      const tilt = (rand() - 0.5) * Math.PI;
      positions.set([Math.cos(angle) * Math.cos(tilt) * radius, Math.sin(tilt) * radius, Math.sin(angle) * Math.cos(tilt) * radius - 6], at * 3);
    } else if (kind === 'globe') {
      const y = 1 - 2 * (at + 0.5) / count;
      const radius = 2.43 + rand() * 0.05;
      const ring = Math.sqrt(Math.max(0, 1 - y * y));
      positions.set([Math.cos(angle) * ring * radius, y * radius, Math.sin(angle) * ring * radius], at * 3);
    } else {
      // Scattered motes, not a bead of light on a wire: the spread is what keeps
      // these from reading as a drawn ring once the scene is seen edge-on.
      const radius = 3.0 + rand() * 0.95;
      const y = Math.sin(angle * 3) * 0.24 + (rand() - 0.5) * 0.62;
      positions.set([Math.cos(angle) * radius, y, Math.sin(angle) * radius], at * 3);
    }
    const warm = rand() > 0.8;
    const violet = Math.cos(angle) > 0.2;
    if (kind === 'stars') {
      // Skewed towards dim stars, as the night sky is; a few blazing bright ones remain.
      colors.set(STARS[Math.min(STARS.length - 1, Math.floor(Math.pow(rand(), 2.1) * STARS.length))], at * 3);
      sizes[at] = rand() > 0.965 ? 0.085 + rand() * 0.07 : 0.02 + rand() * 0.05;
    } else {
      colors.set(warm ? [1, 0.62, 0.24] : violet ? [0.58, 0.34, 1] : [0.15, 0.72, 1], at * 3);
      sizes[at] = 0.007 + rand() * 0.026;
    }
  }
  return { positions, colors, sizes };
}

function Dust({ kind, count, travel, energy }: { kind: 'stars' | 'globe' | 'orbit'; count: number; travel: number; energy: RefObject<number> }) {
  const data = useMemo(() => particles(kind, count), [kind, count]);
  const uniforms = useMemo(() => ({ uTime: { value: 0 }, uTravel: { value: travel }, uEnergy: { value: 0 } }), [travel]);
  useFrame(({ clock }) => { uniforms.uTime.value = clock.elapsedTime; uniforms.uEnergy.value = energy.current; });
  return <points>
    <bufferGeometry>
      <bufferAttribute attach="attributes-position" args={[data.positions, 3]} />
      <bufferAttribute attach="attributes-color" args={[data.colors, 3]} />
      <bufferAttribute attach="attributes-aSize" args={[data.sizes, 1]} />
    </bufferGeometry>
    <shaderMaterial vertexShader={pointVertex} fragmentShader={pointFragment} uniforms={uniforms} transparent depthWrite={false} blending={THREE.AdditiveBlending} />
  </points>;
}

function Observatory({ theme, reducedMotion, levels, motion, nodes, count, selected, requestFrame, paced }: Props & { paced: boolean }) {
  const { camera, size, invalidate } = useThree();
  // The scene canvas spans the window, but the library strip must sit between the rails.
  const band = useRef({ left: 0, right: size.width });
  const globe = useRef<THREE.Group>(null);
  const sky = useRef<THREE.Group>(null);
  const energy = useRef(0);
  // Browsing is eased: a wheel notch or an arrow key glides the shelf into place instead
  // of teleporting one sleeve, while a drag stays locked to the pointer.
  const eased = useRef(0);
  const grid = useMemo(gridTexture, []);
  const goldGlow = useMemo(() => glowTexture('rgba(255,250,232,1)', 'rgba(255,176,86,.55)'), []);
  const coolGlow = useMemo(() => glowTexture('rgba(232,248,255,1)', 'rgba(86,170,255,.5)'), []);
  const skyMap = useMemo(skyTexture, []);
  // The core disc reads the live spectrum through a fine texture, one pixel per angle.
  const disc = useMemo(() => {
    const bins = 256;
    const data = new Uint8Array(bins * 4);
    for (let at = 0; at < bins; at++) data[at * 4 + 3] = 255;
    const texture = new THREE.DataTexture(data, bins, 1, THREE.RGBAFormat);
    texture.minFilter = THREE.LinearFilter;
    texture.magFilter = THREE.LinearFilter;
    texture.needsUpdate = true;
    return {
      bins, data, texture, smoothed: new Float32Array(bins),
      uniforms: {
        uTime: { value: 0 },
        uEnergy: { value: 0 },
        uSpectrum: { value: texture },
        uInner: { value: new THREE.Color('#ffd7a0') },
        uOuter: { value: new THREE.Color('#79b7ff') },
      },
    };
  }, []);
  // Filaments: thousands of connected 1 px segments swept around a sphere, positioned
  // entirely in the vertex shader from their own seed and position along the strand.
  const filaments = useMemo(() => {
    const strands = 300, steps = 28;
    const vertices = strands * (steps - 1) * 2;
    const seeds = new Float32Array(vertices);
    const along = new Float32Array(vertices);
    let at = 0;
    for (let strand = 0; strand < strands; strand++) {
      const id = strand / strands;
      for (let step = 0; step < steps - 1; step++) {
        seeds[at] = id; along[at] = step / (steps - 1); at++;
        seeds[at] = id; along[at] = (step + 1) / (steps - 1); at++;
      }
    }
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.BufferAttribute(new Float32Array(vertices * 3), 3));
    geometry.setAttribute('aSeed', new THREE.BufferAttribute(seeds, 1));
    geometry.setAttribute('aAlong', new THREE.BufferAttribute(along, 1));
    return {
      geometry,
      uniforms: {
        uTime: { value: 0 },
        uEnergy: { value: 0 },
        uLow: { value: 0 },
        uRadius: { value: 1.44 },
        // The disc's own 256-bin spectrum, already eased: one live reading per band.
        uSpectrum: { value: disc.texture },
        uWarm: { value: new THREE.Color('#ffd06a') },
        uCool: { value: new THREE.Color('#e46bff') },
      },
    };
  }, []);
  useEffect(() => () => {
    filaments.geometry.dispose();
    for (const texture of [grid, skyMap, goldGlow, coolGlow, disc.texture]) texture.dispose();
  }, [grid, skyMap, goldGlow, coolGlow, disc, filaments]);
  useEffect(() => { requestFrame.current = invalidate; return () => { requestFrame.current = null; }; }, [invalidate, requestFrame]);
  useEffect(() => {
    const read = () => {
      band.current = {
        left: document.querySelector('.sidebar')?.getBoundingClientRect().right ?? 0,
        right: document.querySelector('.inspector')?.getBoundingClientRect().left ?? size.width,
      };
      invalidate();
    };
    read();
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(read);
    observer?.observe(document.documentElement);
    return () => observer?.disconnect();
  }, [invalidate, size.width]);
  const rim = useMemo(() => ({
    uTint: { value: new THREE.Color(themes[theme].primary) },
    uStrength: { value: 1.5 },
  }), [theme]);
  const outerRim = useMemo(() => ({
    uTint: { value: new THREE.Color('#b073ff') },
    uStrength: { value: 0.7 },
  }), []);
  useEffect(() => { camera.position.set(0, 1.72, 12.9); camera.lookAt(0, 0.18, 0); }, [camera]);
  // Nothing playing: the field only drifts, so it does not need every frame the display
  // has. A hand on the shelf, a throw still coasting and a step still gliding into place
  // ask for the full rate, so browsing moves exactly as it did before.
  useEffect(() => {
    if (!paced) return;
    let frame = 0;
    let last = 0;
    const step = (now: number) => {
      frame = requestAnimationFrame(step);
      if (!shelfMoving(motion.current, eased.current) && now - last < 1000 / IDLE_FPS) return;
      last = now;
      invalidate();
    };
    frame = requestAnimationFrame(step);
    return () => cancelAnimationFrame(frame);
  }, [paced, invalidate, motion, eased]);

  useFrame(({ clock }) => {
    const status = levels.current;
    const shown = status.playing || status.paused;
    energy.current = shown ? Math.min(1, Math.max(0, status.rms * 3)) : 0.12;
    // Dragging and a thrown shelf follow the hand exactly; a wheel notch glides.
    eased.current = easeCursor(eased.current, motion.current.cursor, reducedMotion || motion.current.dragging || motion.current.gliding);
    const turn = eased.current;
    if (globe.current) {
      globe.current.rotation.y = turn * 0.16 + (reducedMotion ? 0 : clock.elapsedTime * 0.03);
      globe.current.rotation.x = motion.current.tilt * 0.6;
    }
    // The sky turns more slowly than the globe, so browsing carries real parallax.
    if (sky.current) {
      sky.current.rotation.y = turn * 0.045 + (reducedMotion ? 0 : clock.elapsedTime * 0.004);
      sky.current.rotation.x = motion.current.tilt * 0.3;
    }
    // Feed the disc one spectrum pixel per angle, eased so bars glide instead of flicker.
    disc.uniforms.uTime.value = clock.elapsedTime;
    disc.uniforms.uEnergy.value = energy.current;
    const spectrum = status.spectrum;
    for (let at = 0; at < disc.bins; at++) {
      // Interpolate between the engine's own bins, so 32 readings do not become 32 steps.
      let raw = 0;
      if (spectrum.length > 1) {
        const place = at / disc.bins * (spectrum.length - 1);
        const low = Math.floor(place);
        const next = Math.min(spectrum.length - 1, low + 1);
        raw = spectrum[low] + (spectrum[next] - spectrum[low]) * (place - low);
      }
      const target = Number.isFinite(raw) ? Math.max(0, Math.min(1, raw)) : 0;
      disc.smoothed[at] += (target - disc.smoothed[at]) * 0.28;
      disc.data[at * 4] = Math.round(disc.smoothed[at] * 255);
    }
    disc.texture.needsUpdate = true;
    filaments.uniforms.uTime.value = clock.elapsedTime;
    filaments.uniforms.uEnergy.value = energy.current;
    filaments.uniforms.uLow.value = shown ? Math.min(1, Math.max(0, status.low)) : 0.12;
    filaments.uniforms.uRadius.value = 1.44 * (1 + energy.current * 0.04);

    // One shelf of sleeves along the bottom of the scene: songs slide sideways with the
    // library and the shelf is browsed by dragging, the wheel or the arrow keys.
    for (const [index, { card }] of nodes.current) {
      const layout = coverLayout(index, turn, count);
      const x = (band.current.left + band.current.right) / 2 + layout.x * size.height * 0.94;
      // The shelf is level and stays level: a vertical drag tilts the sky around it, it
      // never tilts the sleeves. Only horizontal travel moves them.
      // The shelf sits low in the scene, with the browse controls gone underneath it.
      const y = size.height * (0.8 + layout.y);
      const width = Math.max(46, Math.min(128, size.height * (index === selected ? 0.108 : 0.1) * layout.scale));
      // Sleeves turn away towards the ends of the shelf, so browsing reads as a shelf
      // pulled past the camera rather than a grid sliding sideways.
      const offset = Math.max(-1, Math.min(1, layout.x * 3.2));
      const yaw = offset * -22;
      card.style.transform = `translate3d(${x}px,${y}px,0) translate(-50%,-50%) rotateY(${yaw.toFixed(2)}deg) rotateX(-2.5deg)`;
      card.style.width = `${width}px`;
      card.style.zIndex = String(1000 + Math.round(layout.scale * 100));
      card.style.opacity = String(index === selected ? 1 : Math.max(0, Math.min(1, layout.opacity)));
    }
  });

  return <>
    <color attach="background" args={['#02040c']} />

    {/* deep space, held in one group so the whole sky turns with the library */}
    {/* the sky, painted once on a sphere around the camera so there is no edge to find */}
    <group ref={sky}>
    <mesh rotation={[0.34, 0, -0.5]}>
      <sphereGeometry args={[80, 64, 48]} />
      <meshBasicMaterial map={skyMap} side={THREE.BackSide} transparent opacity={0.92} depthWrite={false} blending={THREE.AdditiveBlending} />
    </mesh>
    <Dust kind="stars" count={9000} travel={0} energy={energy} />
    </group>

    {/* The orb sits a little above the middle so it and the shelf below share the scene as
        one composition rather than the orb hogging the centre. */}
    <group scale={1.32} position={[0, 0.3, 0]}>
    {/* the globe */}
    <group ref={globe} position={[0, 0.05, 0]}>
      <mesh>
        <sphereGeometry args={[1.86, 96, 64]} />
        <meshBasicMaterial map={grid} transparent opacity={0.78} depthWrite={false} blending={THREE.AdditiveBlending} side={THREE.DoubleSide} />
      </mesh>
      <mesh>
        <sphereGeometry args={[1.95, 64, 48]} />
        <shaderMaterial vertexShader={rimVertex} fragmentShader={rimFragment} uniforms={rim} transparent depthWrite={false} side={THREE.BackSide} blending={THREE.AdditiveBlending} />
      </mesh>
      <mesh>
        <sphereGeometry args={[2.14, 64, 48]} />
        <shaderMaterial vertexShader={rimVertex} fragmentShader={rimFragment} uniforms={outerRim} transparent depthWrite={false} side={THREE.BackSide} blending={THREE.AdditiveBlending} />
      </mesh>
      <Dust kind="globe" count={2400} travel={0} energy={energy} />
      <lineSegments geometry={filaments.geometry} frustumCulled={false}>
        <shaderMaterial vertexShader={filamentVertex} fragmentShader={filamentFragment} uniforms={filaments.uniforms} transparent depthWrite={false} blending={THREE.AdditiveBlending} />
      </lineSegments>
    </group>
    <sprite position={[0, 0.2, -3]} scale={[4.4, 4.4, 1]}><spriteMaterial map={coolGlow} transparent opacity={0.16} depthWrite={false} blending={THREE.AdditiveBlending} /></sprite>
    {/* orbital dust: the bands' own motes, kept so the globe still has rings of light
        without the drawn circles that cut across the orb */}
    {[{ tilt: 0.2, roll: -0.18 }, { tilt: -0.42, roll: 0.24 }, { tilt: 0.62, roll: 0.34 }].map((band, at) => (
      <group key={at} rotation={[band.tilt, 0, band.roll]} position={[0, 0.05, 0]}>
        <Dust kind="orbit" count={1900} travel={reducedMotion ? 0 : (at % 2 ? -0.05 : 0.04)} energy={energy} />
      </group>
    ))}

    {/* the golden core, held at its old size so the larger globe still reads */}
    <group position={[0, 0, 0.05]} scale={0.72}>
      <mesh rotation={[-0.62, 0, 0]}>
        <planeGeometry args={[4.8, 4.8]} />
        <shaderMaterial vertexShader={discVertex} fragmentShader={discFragment} uniforms={disc.uniforms} transparent depthWrite={false} side={THREE.DoubleSide} blending={THREE.AdditiveBlending} />
      </mesh>
      <sprite scale={[1.1, 1.1, 1]}><spriteMaterial map={goldGlow} transparent opacity={0.45} depthWrite={false} blending={THREE.AdditiveBlending} /></sprite>
    </group>

    </group>
  </>;
}

class SceneBoundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() { return { failed: true }; }
  render() {
    return this.state.failed ? <div className="scene-fallback" role="status">The globe needs WebGL.<span>Your library and playback controls remain available.</span></div> : this.props.children;
  }
}

/**
 * Frames per second for a scene with nothing playing.
 *
 * Idle, everything on the field drifts — the globe turns a few hundredths of a
 * radian a second, the dust drifts with it — so a frame is a picture of almost the
 * same thing as the frame before it. Thirty of those a second is indistinguishable
 * from sixty for motion that slow, and it halves what the scene costs while nothing
 * is playing. Browsing keeps the display's own rate: a drag, a throw or a gliding step
 * asks for every frame until the shelf is still again.
 */
const IDLE_FPS = 30;

export const OrbitScene = memo(function OrbitScene(props: Props) {
  const [visible, setVisible] = useState(() => !document.hidden);
  useEffect(() => {
    const changed = () => setVisible(!document.hidden);
    document.addEventListener('visibilitychange', changed);
    return () => document.removeEventListener('visibilitychange', changed);
  }, [setVisible]);
  // The scene only has a frame rate to spend when it can be seen and is not a still
  // image: reduced motion and another workspace want frames only when something asks.
  const animated = visible && props.active && !props.reducedMotion && !props.suspended;
  return <SceneBoundary><Canvas
    frameloop={props.suspended ? 'never' : animated && props.playing ? 'always' : 'demand'}
    dpr={[1, 1.6]}
    camera={{ position: [0, 1.72, 12.9], fov: 38 }}
    gl={{ antialias: true, powerPreference: 'high-performance' }}
  ><Observatory {...props} paced={animated && !props.playing} /></Canvas></SceneBoundary>;
});
