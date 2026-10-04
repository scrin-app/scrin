// The scrin mark, as numbers. Everything (SVG masters, Android vectors, motion, sheet) reads this.
//
// Construction (24-unit keyline grid, stroke-centreline coordinates):
//   The mark is a lowercase "s" drawn from screen corners (radius 3 = the UI's radius-md feel).
//   It is 180-degree rotationally symmetric about the grid centre (12,12) and is split there:
//   the ivory half ("you") and the lagoon half ("them") are the same shape, turned to face each
//   other, meeting in the middle bar. scrin = two screens becoming one.
//
// Optical sizes (logo-craft.md): master >= 64 px (stroke 2.75), small 24-48 px (stroke 3),
// micro 16-20 px (16-unit pixel grid, stroke 2, whole-pixel bars).

export const GRID = 24;
export const CENTER = 12;

/** Half paths in the 24 grid. The ivory half overlaps 0.35 under the lagoon half to hide the seam. */
export function halves({ stroke = 3, r = 3, overlap = 0.35, gap = 0 } = {}) {
  // Bars at y = 5.5 / 12 / 18.5 and stems at x = 7.5 / 16.5 keep the 12x16 outer box for any stroke
  // by moving centrelines inward with the stroke: outer edge stays at x 6..18, y 4..20.
  const h = stroke / 2;
  const x0 = 6 + h;
  const x1 = 18 - h;
  const y0 = 4 + h;
  const y2 = 20 - h;
  const y1 = 12;
  const g = gap / 2;
  const ivory =
    `M${x1} ${y0}H${x0 + r}A${r} ${r} 0 0 0 ${x0} ${y0 + r}` +
    `V${y1 - r}A${r} ${r} 0 0 0 ${x0 + r} ${y1}H${CENTER + overlap - g}`;
  const lagoon =
    `M${CENTER + g} ${y1}H${x1 - r}A${r} ${r} 0 0 1 ${x1} ${y1 + r}` +
    `V${y2 - r}A${r} ${r} 0 0 1 ${x1 - r} ${y2}H${x0}`;
  return { ivory, lagoon, stroke };
}

/** Pixel-snapped 16 px variant: 16-unit grid, 2 px stroke, bars on rows 2-4 / 7-9 / 12-14. */
export function microHalves({ gap = 0 } = {}) {
  const g = gap / 2;
  return {
    ivory: `M12 3H7A2 2 0 0 0 5 5V6A2 2 0 0 0 7 8H${8.3 - g}`,
    lagoon: `M${8 + g} 8H9A2 2 0 0 1 11 10V11A2 2 0 0 1 9 13H4`,
    stroke: 2,
  };
}

/** Lettered wordmark "scrin" (not typed: drawn from the mark's own strokes). x-height 16 units. */
export function wordmark({ stroke = 3 } = {}) {
  const h = stroke / 2;
  const r = 3;
  const top = h;
  const bot = 16 - h;
  const mid = 8;
  const s = (x) =>
    `M${x + 12 - h} ${top}H${x + h + r}A${r} ${r} 0 0 0 ${x + h} ${top + r}V${mid - r}` +
    `A${r} ${r} 0 0 0 ${x + h + r} ${mid}H${x + 12 - h - r}A${r} ${r} 0 0 1 ${x + 12 - h} ${mid + r}` +
    `V${bot - r}A${r} ${r} 0 0 1 ${x + 12 - h - r} ${bot}H${x + h}`;
  const c = (x) =>
    `M${x + 12 - h} ${top}H${x + h + r}A${r} ${r} 0 0 0 ${x + h} ${top + r}V${bot - r}` +
    `A${r} ${r} 0 0 0 ${x + h + r} ${bot}H${x + 12 - h}`;
  const rr = (x) => `M${x + h} 16V${top + r}A${r} ${r} 0 0 1 ${x + h + r} ${top}H${x + 9.5}`;
  const i = (x) => `M${x + h} 16V0`;
  const n = (x) =>
    `M${x + h} 16V${top + r}A${r} ${r} 0 0 1 ${x + h + r} ${top}H${x + 12 - h - r}` +
    `A${r} ${r} 0 0 1 ${x + 12 - h} ${top + r}V16`;
  // Hand-kerned advances: open right side of c and r's short arm pull the next letter in.
  const xs = { s: 0, c: 15.5, r: 30.5, i: 43, n: 49.5 };
  return {
    paths: [s(xs.s), c(xs.c), rr(xs.r), i(xs.i), n(xs.n)],
    // i-dot = a tiny screen, in the accent colour.
    dot: { x: xs.i, y: -5.5, w: stroke, h: stroke, rx: 0.75 },
    width: xs.n + 12,
    top: -5.5,
    height: 21.5,
    stroke,
  };
}
