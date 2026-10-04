/**
 * OKLCH → sRGB → WCAG 2.x contrast, by hand (CSS Color 4 §10.3). Out-of-gamut
 * channels are clipped the way a browser paints them. Ported from dashy.
 */
export interface Oklch {
  l: number;
  c: number;
  h: number;
}

/** `oklch(0.6 0.1 250)` or `#rrggbb` → `{ l, c, h }`. */
export function parseColor(input: string): Oklch {
  const s = input.trim();
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(s);
  if (hex) return rgbToOklch(hexToRgb(hex[1] ?? '000'));
  const m = /^oklch\(\s*([\d.]+)(%?)\s+([\d.]+)\s+([\d.]+)/i.exec(s);
  if (!m) throw new Error(`Unsupported colour: ${input}`);
  const l = Number(m[1]);
  return { l: m[2] === '%' ? l / 100 : l, c: Number(m[3]), h: Number(m[4]) };
}

function hexToRgb(h: string): [number, number, number] {
  const full = h.length === 3 ? h.replace(/./g, '$&$&') : h;
  const n = Number.parseInt(full, 16);
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
}

const clamp01 = (v: number) => (v < 0 ? 0 : v > 1 ? 1 : v);

function oklchToLinearSrgb({ l, c, h }: Oklch): [number, number, number] {
  const rad = (h * Math.PI) / 180;
  const a = c * Math.cos(rad);
  const b = c * Math.sin(rad);
  const l_ = l + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
  const m_ = l - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
  const s_ = l - 0.089_484_177_5 * a - 1.291_485_548 * b;
  const L = l_ ** 3;
  const M = m_ ** 3;
  const S = s_ ** 3;
  return [
    clamp01(4.076_741_662_1 * L - 3.307_711_591_3 * M + 0.230_969_929_2 * S),
    clamp01(-1.268_438_004_6 * L + 2.609_757_401_1 * M - 0.341_319_396_5 * S),
    clamp01(-0.004_196_086_3 * L - 0.703_418_614_8 * M + 1.707_614_701 * S),
  ];
}

function luminance(color: Oklch): number {
  const [r, g, b] = oklchToLinearSrgb(color);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

export function contrastRatio(a: Oklch | string, b: Oklch | string): number {
  const la = luminance(typeof a === 'string' ? parseColor(a) : a);
  const lb = luminance(typeof b === 'string' ? parseColor(b) : b);
  const [hi, lo] = la > lb ? [la, lb] : [lb, la];
  return (hi + 0.05) / (lo + 0.05);
}

const lin = (v: number) => (v <= 0.040_45 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);

function rgbToOklch([r, g, b]: [number, number, number]): Oklch {
  const [R, G, B] = [lin(r), lin(g), lin(b)];
  const l_ = Math.cbrt(0.412_221_470_8 * R + 0.536_332_536_3 * G + 0.051_445_992_9 * B);
  const m_ = Math.cbrt(0.211_903_498_2 * R + 0.680_699_545_1 * G + 0.107_396_956_6 * B);
  const s_ = Math.cbrt(0.088_302_461_9 * R + 0.281_718_837_6 * G + 0.629_978_700_5 * B);
  const L = 0.210_454_255_3 * l_ + 0.793_617_785 * m_ - 0.004_072_046_8 * s_;
  const a = 1.977_998_495_1 * l_ - 2.428_592_205 * m_ + 0.450_593_709_9 * s_;
  const bb = 0.025_904_037_1 * l_ + 0.782_771_766_2 * m_ - 0.808_675_766 * s_;
  const c = Math.hypot(a, bb);
  const h = c < 1e-6 ? 0 : ((Math.atan2(bb, a) * 180) / Math.PI + 360) % 360;
  return { l: L, c, h };
}

export function formatOklch({ l, c, h }: Oklch): string {
  return `oklch(${l.toFixed(3)} ${c.toFixed(3)} ${h.toFixed(1)})`;
}
