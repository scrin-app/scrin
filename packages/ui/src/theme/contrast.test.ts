import { describe, expect, it } from 'vitest';

import { contrastRatio, parseColor } from './contrast';
import { ACCENT_PRESETS } from './presets';
import { derivePalette, SURFACES, type ResolvedMode, type TokenName } from './tokens';

const AA_TEXT = 4.5;
const AA_UI = 3;

/** [foreground, background, minimum ratio] pairs that real UI paints. */
const PAIRS: [TokenName, TokenName, number][] = [
  ['fg', 'bg', AA_TEXT],
  ['fg', 'surface', AA_TEXT],
  ['fg', 'surface-2', AA_TEXT],
  ['muted', 'bg', AA_TEXT],
  ['muted', 'surface', AA_TEXT],
  ['muted', 'surface-2', AA_TEXT],
  ['accent-fg', 'accent', AA_TEXT],
  // Accent as link text / focus ring / icon on the page background.
  ['accent', 'bg', AA_TEXT],
  ['accent', 'surface', AA_TEXT],
  ['accent-fg', 'danger', AA_UI],
  ['success', 'surface', AA_UI],
  ['warning', 'surface', AA_UI],
  ['danger', 'surface', AA_UI],
  ['danger', 'bg', AA_TEXT],
];

const MODES: ResolvedMode[] = ['light', 'dark'];

describe('contrast maths', () => {
  it('matches the WCAG reference points', () => {
    expect(contrastRatio('#000', '#fff')).toBeCloseTo(21, 0);
    expect(contrastRatio('#777', '#fff')).toBeCloseTo(4.48, 1);
  });

  it('parses oklch strings', () => {
    expect(parseColor('oklch(0.5 0.1 200)')).toEqual({ l: 0.5, c: 0.1, h: 200 });
    expect(parseColor('oklch(50% 0.1 200)').l).toBeCloseTo(0.5);
  });
});

describe('WCAG AA over every preset × mode × surface', () => {
  expect(ACCENT_PRESETS.length).toBeGreaterThanOrEqual(12);

  for (const preset of ACCENT_PRESETS) {
    for (const mode of MODES) {
      for (const surface of SURFACES) {
        if (mode === 'light' && surface === 'amoled') continue;
        it(`${preset.id} / ${mode} / ${surface}`, () => {
          const p = derivePalette(mode, surface, preset);
          const failures = PAIRS.flatMap(([fg, bg, min]) => {
            const ratio = contrastRatio(p[fg], p[bg]);
            return ratio >= min ? [] : [`${fg} on ${bg}: ${ratio.toFixed(2)} < ${min}`];
          });
          expect(failures).toEqual([]);
        });
      }
    }
  }
});

describe('surfaces', () => {
  it('AMOLED background is true black', () => {
    expect(derivePalette('dark', 'amoled', ACCENT_PRESETS[0]!).bg.l).toBe(0);
  });
});
