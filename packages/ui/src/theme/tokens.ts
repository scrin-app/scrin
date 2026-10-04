import { formatOklch, type Oklch } from './contrast';

export const MODES = ['light', 'dark', 'system'] as const;
export type Mode = (typeof MODES)[number];
export type ResolvedMode = 'light' | 'dark';

export const SURFACES = ['solid', 'glass', 'amoled'] as const;
export type Surface = (typeof SURFACES)[number];

export const DENSITIES = ['compact', 'comfortable', 'spacious'] as const;
export type Density = (typeof DENSITIES)[number];

export const MOTIONS = ['off', 'subtle', 'full'] as const;
export type MotionLevel = (typeof MOTIONS)[number];

export interface Accent {
  hue: number;
  chroma: number;
}

export interface ThemeSettings {
  mode: Mode;
  accent: Accent;
  surface: Surface;
  density: Density;
  motion: MotionLevel;
}

export const DEFAULT_THEME: ThemeSettings = {
  mode: 'system',
  accent: { hue: 264, chroma: 0.17 },
  surface: 'solid',
  density: 'comfortable',
  motion: 'full',
};

/** Every colour slot the stylesheet reads. */
export const TOKEN_NAMES = [
  'bg',
  'surface',
  'surface-2',
  'fg',
  'muted',
  'outline',
  'accent',
  'accent-fg',
  'success',
  'warning',
  'danger',
] as const;
export type TokenName = (typeof TOKEN_NAMES)[number];
export type Palette = Record<TokenName, Oklch>;

/**
 * Derives the full palette from mode + surface + accent. A pure function so
 * the contrast tests can sweep every preset × mode × surface, and so the same
 * numbers can be exported to the Android Compose theme.
 *
 * Accent lightness is chosen per mode so `accent-fg` on `accent` and `accent`
 * on `bg` both stay at WCAG AA for every hue: hues whose sRGB gamut is narrow
 * (yellow-green) get a lower chroma cap through the gamut clip in contrast.ts,
 * and the test enforces the result rather than trusting the curve.
 */
export function derivePalette(mode: ResolvedMode, surface: Surface, accent: Accent): Palette {
  const h = accent.hue;
  const c = accent.chroma;
  // Neutrals carry a whisper of the accent hue so the UI feels of a piece.
  const tint = Math.min(c, 0.2) * 0.08;

  if (mode === 'light') {
    // Accent L is lowered for yellow/green hues, which are perceptually light.
    const yellowish = h > 70 && h < 160;
    const accentL = yellowish ? 0.47 : 0.52;
    return {
      bg: { l: 0.985, c: tint * 0.5, h },
      surface: { l: 1, c: 0, h },
      'surface-2': { l: 0.955, c: tint, h },
      fg: { l: 0.2, c: tint, h },
      muted: { l: 0.47, c: tint, h },
      outline: { l: 0.86, c: tint, h },
      accent: { l: accentL, c: Math.min(c, 0.2), h },
      'accent-fg': { l: 0.99, c: 0, h },
      success: { l: 0.5, c: 0.13, h: 150 },
      warning: { l: 0.55, c: 0.14, h: 70 },
      danger: { l: 0.53, c: 0.19, h: 27 },
    };
  }

  const amoled = surface === 'amoled';
  return {
    bg: amoled ? { l: 0, c: 0, h } : { l: 0.17, c: tint, h },
    surface: amoled ? { l: 0.13, c: tint, h } : { l: 0.215, c: tint, h },
    'surface-2': amoled ? { l: 0.18, c: tint, h } : { l: 0.26, c: tint, h },
    fg: { l: 0.96, c: tint * 0.5, h },
    muted: { l: 0.74, c: tint, h },
    outline: amoled ? { l: 0.3, c: tint, h } : { l: 0.36, c: tint, h },
    accent: { l: 0.76, c: Math.min(c, 0.17), h },
    'accent-fg': { l: 0.18, c: tint, h },
    success: { l: 0.77, c: 0.15, h: 150 },
    warning: { l: 0.82, c: 0.14, h: 80 },
    danger: { l: 0.72, c: 0.17, h: 25 },
  };
}

const DENSITY_SCALE: Record<Density, number> = { compact: 0.8, comfortable: 1, spacious: 1.25 };
const MOTION_SCALE: Record<MotionLevel, number> = { off: 0, subtle: 0.6, full: 1 };

export function motionScale(level: MotionLevel, reducedMotion: boolean): number {
  return reducedMotion ? 0 : MOTION_SCALE[level];
}

/** CSS custom properties written on `<html>`; `theme.css` maps them into Tailwind. */
export function themeToCssVars(
  settings: ThemeSettings,
  env: { systemPrefersDark: boolean; reducedMotion: boolean },
): Record<string, string> {
  const mode = resolveMode(settings.mode, env.systemPrefersDark);
  const palette = derivePalette(mode, settings.surface, settings.accent);
  const vars: Record<string, string> = {
    '--scrin-accent-h': String(settings.accent.hue),
    '--scrin-accent-c': String(settings.accent.chroma),
    '--scrin-density': String(DENSITY_SCALE[settings.density]),
    '--scrin-motion': String(motionScale(settings.motion, env.reducedMotion)),
  };
  for (const name of TOKEN_NAMES) vars[`--scrin-${name}`] = formatOklch(palette[name]);
  return vars;
}

export function themeToDataAttrs(
  settings: ThemeSettings,
  env: { systemPrefersDark: boolean; reducedMotion: boolean },
): Record<string, string> {
  return {
    'data-mode': resolveMode(settings.mode, env.systemPrefersDark),
    'data-surface': settings.surface,
    'data-density': settings.density,
    'data-motion': env.reducedMotion ? 'off' : settings.motion,
  };
}

export function resolveMode(mode: Mode, systemPrefersDark: boolean): ResolvedMode {
  if (mode === 'system') return systemPrefersDark ? 'dark' : 'light';
  return mode;
}

/** Validates a stored value, falling back field-by-field to the defaults. */
export function parseThemeSettings(raw: unknown): ThemeSettings {
  if (typeof raw !== 'object' || raw === null) return DEFAULT_THEME;
  const field = (k: string): unknown => Reflect.get(raw, k);
  const a = field('accent');
  const num = (k: string): unknown =>
    typeof a === 'object' && a !== null ? Reflect.get(a, k) : undefined;
  const hueRaw = num('hue');
  const chromaRaw = num('chroma');
  const hue =
    typeof hueRaw === 'number' && hueRaw >= 0 && hueRaw <= 360 ? hueRaw : DEFAULT_THEME.accent.hue;
  const chroma =
    typeof chromaRaw === 'number' && chromaRaw >= 0 && chromaRaw <= 0.4
      ? chromaRaw
      : DEFAULT_THEME.accent.chroma;
  return {
    mode: pick(MODES, field('mode'), DEFAULT_THEME.mode),
    accent: { hue, chroma },
    surface: pick(SURFACES, field('surface'), DEFAULT_THEME.surface),
    density: pick(DENSITIES, field('density'), DEFAULT_THEME.density),
    motion: pick(MOTIONS, field('motion'), DEFAULT_THEME.motion),
  };
}

function pick<T extends string>(list: readonly T[], v: unknown, fallback: T): T {
  return list.find((x) => x === v) ?? fallback;
}
