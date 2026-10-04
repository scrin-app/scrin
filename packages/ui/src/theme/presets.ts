import type { TranslationKey } from '@scrin/i18n';

import type { Accent } from './tokens';

export interface AccentPreset extends Accent {
  id: string;
  /** i18n key for the swatch name. */
  labelKey: Extract<TranslationKey, `theme.preset${string}`>;
}

/** 13 accent presets spread around the OKLCH hue wheel, plus a neutral. */
export const ACCENT_PRESETS: readonly AccentPreset[] = [
  { id: 'iris', hue: 264, chroma: 0.17, labelKey: 'theme.presetIris' },
  { id: 'ocean', hue: 245, chroma: 0.15, labelKey: 'theme.presetOcean' },
  { id: 'sky', hue: 225, chroma: 0.13, labelKey: 'theme.presetSky' },
  { id: 'teal', hue: 190, chroma: 0.12, labelKey: 'theme.presetTeal' },
  { id: 'mint', hue: 160, chroma: 0.13, labelKey: 'theme.presetMint' },
  { id: 'lime', hue: 130, chroma: 0.15, labelKey: 'theme.presetLime' },
  { id: 'amber', hue: 75, chroma: 0.15, labelKey: 'theme.presetAmber' },
  { id: 'tangerine', hue: 50, chroma: 0.17, labelKey: 'theme.presetTangerine' },
  { id: 'coral', hue: 30, chroma: 0.17, labelKey: 'theme.presetCoral' },
  { id: 'rose', hue: 5, chroma: 0.18, labelKey: 'theme.presetRose' },
  { id: 'orchid', hue: 330, chroma: 0.17, labelKey: 'theme.presetOrchid' },
  { id: 'violet', hue: 295, chroma: 0.18, labelKey: 'theme.presetViolet' },
  { id: 'graphite', hue: 260, chroma: 0.02, labelKey: 'theme.presetGraphite' },
];

export function presetFor(accent: Accent): AccentPreset | undefined {
  return ACCENT_PRESETS.find((p) => p.hue === accent.hue && p.chroma === accent.chroma);
}
