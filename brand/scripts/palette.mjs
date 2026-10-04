// Single source of the scrin palette. Every token file, the contrast pairs, the icon colours and
// the dist outputs are generated from this module by build.mjs. Edit here, then rebuild.
import { clampChroma, toHex } from './oklch.mjs';

/** Brand hue: "lagoon" — aqua-green, unowned in the remote-desktop category. */
export const HUE = 178;
/** Accent chroma requested from packages/ui derivePalette (it caps / gamut-maps per mode). */
export const CHROMA = 0.14;
/** Neutral tint = what packages/ui derivePalette applies: min(c, 0.2) * 0.08. */
export const TINT = +(Math.min(CHROMA, 0.2) * 0.08).toFixed(4);

const L = (l, c, h = HUE) => {
  const cc = +clampChroma(l, c, h).toFixed(4);
  return { l, c: cc, h, hex: toHex(l, cc, h) };
};

// 12-step ramps (Radix step jobs). Dark is designed first; light derived.
const lagoonDarkL = [0.17, 0.195, 0.23, 0.26, 0.295, 0.34, 0.4, 0.48, 0.76, 0.8, 0.84, 0.94];
const lagoonDarkC = [0.012, 0.018, 0.03, 0.04, 0.05, 0.06, 0.075, 0.09, 0.14, 0.13, 0.11, 0.05];
const lagoonLightL = [0.99, 0.975, 0.95, 0.92, 0.885, 0.84, 0.78, 0.7, 0.52, 0.48, 0.45, 0.28];
const lagoonLightC = [0.008, 0.016, 0.03, 0.045, 0.06, 0.075, 0.09, 0.11, 0.14, 0.13, 0.11, 0.06];
const neutralLightL = [0.985, 0.97, 0.955, 0.935, 0.91, 0.885, 0.86, 0.78, 0.6, 0.55, 0.47, 0.2];
const neutralDarkL = [0.17, 0.195, 0.215, 0.26, 0.29, 0.32, 0.36, 0.45, 0.6, 0.66, 0.74, 0.96];

export const ramps = {
  lagoon: lagoonLightL.map((l, i) => L(l, lagoonLightC[i])),
  'lagoon-dark': lagoonDarkL.map((l, i) => L(l, lagoonDarkC[i])),
  neutral: neutralLightL.map((l, i) => L(l, i === 0 ? TINT * 0.5 : TINT)),
  'neutral-dark': neutralDarkL.map((l, i) => L(l, i === 11 ? TINT * 0.5 : TINT)),
};

// Brand-only colours (icon tile, wordmark). Not part of the app UI ramps.
export const brandColors = {
  ink: L(0.235, 0.04, 205), // icon tile, banner background
  'ink-hi': L(0.315, 0.05, 200), // tile gradient top-left (lit 120deg, Fluent)
  'ink-rim': L(0.42, 0.05, 200), // 1-unit inner rim so the tile reads on dark taskbars
  ivory: L(0.97, 0.012, 178), // "you" half on dark
  'lagoon-bright': L(0.82, 0.14, 178), // "them" half on dark
  'lagoon-deep': L(0.5, 0.1, 178), // "them" half on light
  'ink-text': L(0.24, 0.035, 205), // "you" half / wordmark on light
};

// Status colours: exactly what packages/ui derivePalette uses (step 9), plus bg/fg steps.
export const status = {
  light: {
    success: { bg: L(0.95, 0.04, 150), solid: L(0.5, 0.13, 150), fg: L(0.42, 0.11, 150) },
    warning: { bg: L(0.95, 0.05, 75), solid: L(0.55, 0.14, 70), fg: L(0.45, 0.11, 70) },
    danger: { bg: L(0.95, 0.03, 27), solid: L(0.53, 0.19, 27), fg: L(0.46, 0.17, 27) },
  },
  dark: {
    success: { bg: L(0.25, 0.05, 150), solid: L(0.77, 0.15, 150), fg: L(0.86, 0.11, 150) },
    warning: { bg: L(0.26, 0.05, 80), solid: L(0.82, 0.14, 80), fg: L(0.88, 0.1, 80) },
    danger: { bg: L(0.25, 0.06, 25), solid: L(0.72, 0.17, 25), fg: L(0.85, 0.09, 25) },
  },
};

/** Semantic layer: theme -> slot -> [rampName, step(1-based)] or a literal colour. */
export const semantic = {
  light: {
    'bg.canvas': ['neutral', 1],
    'bg.surface': { l: 1, c: 0, h: HUE, hex: '#ffffff' },
    'bg.subtle': ['neutral', 3],
    'fg.default': ['neutral', 12],
    'fg.muted': ['neutral', 11],
    'border.default': ['neutral', 7],
    'border.focus': ['lagoon', 9],
    'accent.solid': ['lagoon', 9],
    'accent.solid-hover': ['lagoon', 10],
    'accent.text': ['lagoon', 11],
    'accent.subtle': ['lagoon', 3],
    'fg.on-accent': L(0.99, 0, HUE),
  },
  dark: {
    'bg.canvas': ['neutral-dark', 1],
    'bg.surface': ['neutral-dark', 3],
    'bg.subtle': ['neutral-dark', 4],
    'fg.default': ['neutral-dark', 12],
    'fg.muted': ['neutral-dark', 11],
    'border.default': ['neutral-dark', 7],
    'border.focus': ['lagoon-dark', 9],
    'accent.solid': ['lagoon-dark', 9],
    'accent.solid-hover': ['lagoon-dark', 10],
    'accent.text': ['lagoon-dark', 11],
    'accent.subtle': ['lagoon-dark', 3],
    'fg.on-accent': L(0.18, TINT, HUE),
  },
};

export function resolve(theme, slot) {
  const v = semantic[theme][slot];
  if (Array.isArray(v)) return ramps[v[0]][v[1] - 1];
  return v;
}

export const amoledBg = { l: 0, c: 0, h: HUE, hex: '#000000' };
