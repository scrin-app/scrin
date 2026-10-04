// OKLCH <-> sRGB helpers (CSS Color 4). Same matrices as packages/ui/src/theme/contrast.ts,
// plus gamut mapping (chroma reduction, hue + lightness kept) and hex output. No deps.

const lin = (v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
const gam = (v) => (v <= 0.0031308 ? 12.92 * v : 1.055 * v ** (1 / 2.4) - 0.055);

/** Unclamped linear sRGB for an OKLCH colour. */
export function oklchToLinear(l, c, h) {
  const rad = (h * Math.PI) / 180;
  const a = c * Math.cos(rad);
  const b = c * Math.sin(rad);
  const l_ = (l + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m_ = (l - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s_ = (l - 0.0894841775 * a - 1.291485548 * b) ** 3;
  return [
    4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_,
    -1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_,
    -0.0041960863 * l_ - 0.7034186148 * m_ + 1.707614701 * s_,
  ];
}

export function inGamut(l, c, h, eps = 1e-4) {
  return oklchToLinear(l, c, h).every((v) => v >= -eps && v <= 1 + eps);
}

/** Largest chroma <= c that is inside sRGB at this L/h. */
export function clampChroma(l, c, h) {
  if (inGamut(l, c, h)) return c;
  let lo = 0;
  let hi = c;
  for (let i = 0; i < 30; i++) {
    const mid = (lo + hi) / 2;
    if (inGamut(l, mid, h)) lo = mid;
    else hi = mid;
  }
  return lo;
}

export function maxChroma(l, h) {
  return clampChroma(l, 0.4, h);
}

/** Hex as a browser paints it (gamut-clipped by channel, like contrast.ts). */
export function toHex(l, c, h) {
  return (
    '#' +
    oklchToLinear(l, c, h)
      .map((v) => Math.round(Math.min(1, Math.max(0, gam(Math.min(1, Math.max(0, v))))) * 255))
      .map((v) => v.toString(16).padStart(2, '0'))
      .join('')
  );
}

export function luminance(l, c, h) {
  const [r, g, b] = oklchToLinear(l, c, h).map((v) => Math.min(1, Math.max(0, v)));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

export function contrast(a, b) {
  const la = luminance(...a);
  const lb = luminance(...b);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

export const css = ([l, c, h]) => `oklch(${+l.toFixed(3)} ${+c.toFixed(3)} ${+h.toFixed(1)})`;
export { lin };
