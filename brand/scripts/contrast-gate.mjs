#!/usr/bin/env node
// contrast-gate.mjs — WCAG 2.x gate + APCA advisory for brand colour pairs. Node 22, no deps.
//
//   node contrast-gate.mjs brand/contrast-pairs.json [--json]
//   node contrast-gate.mjs --self-test
//
// Input: { "pairs": [ { "fg": "oklch(0.97 0.01 212)", "bg": "#0b1a20", "use": "body|large|ui", "name": "..." } ] }
// Colours: `#rgb`, `#rrggbb`, `oklch(L C H)` with L as 0..1 or `NN%`, optional `/ alpha` (ignored, pairs are opaque).
// Thresholds (WCAG 2.2 AA): body 4.5, large 3, ui 3 (SC 1.4.3 / 1.4.11) -> exit 1 on any failure.
// APCA 0.0.98G-4g Lc advisory: body 75, large 60, ui 45 -> warnings only (APCA is not a conformance standard).
// Out-of-gamut OKLCH is chroma-reduced (hue and lightness kept) into sRGB and reported as "clipped".
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const WCAG_MIN = { body: 4.5, large: 3, ui: 3 };
const APCA_MIN = { body: 75, large: 60, ui: 45 };

// ---------- parsing & conversion ----------
function parseHex(s) {
  let h = s.slice(1);
  if (h.length === 3 || h.length === 4) h = Array.from(h.slice(0, 3), (c) => c + c).join('');
  if (h.length === 8) h = h.slice(0, 6);
  if (!/^[0-9a-f]{6}$/i.test(h)) throw new Error(`bad hex colour: ${s}`);
  return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16) / 255);
}

function oklabToLinearSrgb(L, a, b) {
  const l_ = L + 0.3963377774 * a + 0.2158037573 * b;
  const m_ = L - 0.1055613458 * a - 0.0638541728 * b;
  const s_ = L - 0.0894841775 * a - 1.291485548 * b;
  const l = l_ ** 3, m = m_ ** 3, s = s_ ** 3;
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ];
}

const inGamut = (rgb) => rgb.every((v) => v >= -1e-6 && v <= 1 + 1e-6);
const encode = (v) => (v <= 0.0031308 ? 12.92 * v : 1.055 * v ** (1 / 2.4) - 0.055);
const decode = (v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
const clamp01 = (v) => Math.min(1, Math.max(0, v));

function oklchToSrgb(L, C, H) {
  const lin = (c) => {
    const hr = (H * Math.PI) / 180;
    return oklabToLinearSrgb(L, c * Math.cos(hr), c * Math.sin(hr));
  };
  let rgb = lin(C);
  let clipped = false;
  if (!inGamut(rgb)) {
    clipped = true;
    let lo = 0, hi = C;
    for (let i = 0; i < 40; i++) {
      const mid = (lo + hi) / 2;
      if (inGamut(lin(mid))) lo = mid; else hi = mid;
    }
    rgb = lin(lo);
  }
  return { rgb: rgb.map((v) => clamp01(encode(clamp01(v)))), clipped };
}

export function parseColor(input) {
  const s = String(input).trim().toLowerCase();
  if (s.startsWith('#')) return { rgb: parseHex(s), clipped: false };
  const m = s.match(/^oklch\(\s*([\d.]+)(%?)\s+([\d.]+)\s+([\d.]+)(?:deg)?\s*(?:\/\s*[\d.%]+\s*)?\)$/);
  if (!m) throw new Error(`unsupported colour (use #hex or oklch(L C H)): ${input}`);
  let L = Number(m[1]);
  if (m[2] === '%' || L > 1) L /= 100;
  return oklchToSrgb(L, Number(m[3]), Number(m[4]));
}

export const toHex = (rgb) => '#' + rgb.map((v) => Math.round(v * 255).toString(16).padStart(2, '0')).join('');

// ---------- WCAG 2.x ----------
export function relativeLuminance([r, g, b]) {
  return 0.2126 * decode(r) + 0.7152 * decode(g) + 0.0722 * decode(b);
}
export function wcagRatio(fg, bg) {
  const a = relativeLuminance(fg), b = relativeLuminance(bg);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

// ---------- APCA 0.0.98G-4g ----------
const A = {
  mainTRC: 2.4, sRco: 0.2126729, sGco: 0.7151522, sBco: 0.072175,
  normBG: 0.56, normTXT: 0.57, revTXT: 0.62, revBG: 0.65,
  // APCA 0.0.98G defines blkClmp as exactly 1.414 (not √2); Math.SQRT2 would shift every Lc.
  // oxlint-disable-next-line approx-constant
  blkThrs: 0.022, blkClmp: 1.414, scaleBoW: 1.14, scaleWoB: 1.14,
  loBoWoffset: 0.027, loWoBoffset: 0.027, deltaYmin: 0.0005, loClip: 0.1,
};
function apcaY([r, g, b]) {
  return A.sRco * r ** A.mainTRC + A.sGco * g ** A.mainTRC + A.sBco * b ** A.mainTRC;
}
export function apcaLc(fg, bg) {
  let yt = apcaY(fg), yb = apcaY(bg);
  if (yt <= A.blkThrs) yt += (A.blkThrs - yt) ** A.blkClmp;
  if (yb <= A.blkThrs) yb += (A.blkThrs - yb) ** A.blkClmp;
  if (Math.abs(yb - yt) < A.deltaYmin) return 0;
  let out;
  if (yb > yt) {
    const sapc = (yb ** A.normBG - yt ** A.normTXT) * A.scaleBoW;
    out = sapc < A.loClip ? 0 : sapc - A.loBoWoffset;
  } else {
    const sapc = (yb ** A.revBG - yt ** A.revTXT) * A.scaleWoB;
    out = sapc > -A.loClip ? 0 : sapc + A.loWoBoffset;
  }
  return out * 100;
}

// ---------- gate ----------
export function evaluate(pairs) {
  return pairs.map((p, i) => {
    const use = p.use ?? 'body';
    if (!(use in WCAG_MIN)) throw new Error(`pair ${i}: use must be body|large|ui, got ${use}`);
    const fg = parseColor(p.fg), bg = parseColor(p.bg);
    const ratio = wcagRatio(fg.rgb, bg.rgb);
    const lc = apcaLc(fg.rgb, bg.rgb);
    return {
      name: p.name ?? `${p.fg} on ${p.bg}`, use, fg: p.fg, bg: p.bg,
      fgHex: toHex(fg.rgb), bgHex: toHex(bg.rgb), clipped: fg.clipped || bg.clipped,
      ratio, lc, wcagPass: ratio + 1e-9 >= WCAG_MIN[use], apcaPass: Math.abs(lc) >= APCA_MIN[use],
    };
  });
}

function selfTest() {
  const cases = [
    ['#777 on #fff WCAG ~4.48', wcagRatio(parseColor('#777').rgb, parseColor('#fff').rgb), 4.48, 0.01],
    ['black on white WCAG 21', wcagRatio(parseColor('#000').rgb, parseColor('#fff').rgb), 21, 1e-9],
    ['white on black WCAG 21 (symmetric)', wcagRatio(parseColor('#fff').rgb, parseColor('#000').rgb), 21, 1e-9],
    ['APCA black on white ~Lc 106', apcaLc(parseColor('#000').rgb, parseColor('#fff').rgb), 106.04, 0.1],
    ['APCA white on black ~Lc -107.9', apcaLc(parseColor('#fff').rgb, parseColor('#000').rgb), -107.88, 0.1],
    ['APCA #888 on #fff ~Lc 63.1', apcaLc(parseColor('#888').rgb, parseColor('#fff').rgb), 63.06, 0.2],
    ['oklch(1 0 0) = white (L 0..1)', relativeLuminance(parseColor('oklch(1 0 0)').rgb), 1, 1e-4],
    ['oklch(100% 0 0) = white (L %)', relativeLuminance(parseColor('oklch(100% 0 0)').rgb), 1, 1e-4],
    ['oklch(0 0 0) = black', relativeLuminance(parseColor('oklch(0 0 0)').rgb), 0, 1e-6],
  ];
  let bad = 0;
  for (const [label, got, want, tol] of cases) {
    const ok = Math.abs(got - want) <= tol;
    if (!ok) bad++;
    console.log(`${ok ? 'PASS' : 'FAIL'}  ${label}: got ${got.toFixed(3)} want ${want} +/-${tol}`);
  }
  const red = toHex(parseColor('oklch(0.6279554 0.2576833 29.2338851)').rgb);
  const redOk = red === '#ff0000';
  if (!redOk) bad++;
  console.log(`${redOk ? 'PASS' : 'FAIL'}  oklch(0.628 0.258 29.23) -> ${red} want #ff0000`);
  const clip = parseColor('oklch(0.7 0.4 145)');
  const clipOk = clip.clipped && inGamut(clip.rgb);
  if (!clipOk) bad++;
  console.log(`${clipOk ? 'PASS' : 'FAIL'}  out-of-gamut oklch(0.7 0.4 145) clipped -> ${toHex(clip.rgb)}`);
  console.log(bad ? `self-test: ${bad} FAILED` : 'self-test: all passed');
  return bad ? 1 : 0;
}

function main(argv) {
  if (argv.includes('--self-test')) return selfTest();
  const file = argv.find((a) => !a.startsWith('--'));
  if (!file) {
    console.error('usage: contrast-gate.mjs <contrast-pairs.json> [--json] | --self-test');
    return 2;
  }
  const { pairs } = JSON.parse(readFileSync(file, 'utf8'));
  if (!Array.isArray(pairs) || pairs.length === 0) { console.error('no "pairs" array in input'); return 2; }
  const res = evaluate(pairs);
  if (argv.includes('--json')) console.log(JSON.stringify(res, null, 2));
  else {
    for (const r of res) {
      const tag = !r.wcagPass ? 'FAIL' : !r.apcaPass ? 'WARN' : 'PASS';
      console.log(
        `${tag}  ${r.name.padEnd(34)} ${r.use.padEnd(5)} WCAG ${r.ratio.toFixed(2).padStart(5)} (min ${WCAG_MIN[r.use]})` +
        `  APCA Lc ${r.lc.toFixed(1).padStart(6)} (adv ${APCA_MIN[r.use]})  ${r.fgHex} on ${r.bgHex}${r.clipped ? '  [clipped to sRGB]' : ''}`,
      );
    }
  }
  const fails = res.filter((r) => !r.wcagPass).length;
  const warns = res.filter((r) => r.wcagPass && !r.apcaPass).length;
  console.log(`\n${res.length} pairs: ${res.length - fails - warns} pass, ${warns} APCA advisory warnings, ${fails} WCAG failures`);
  return fails ? 1 : 0;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  try { process.exitCode = main(process.argv.slice(2)); }
  catch (e) { console.error(`error: ${e.message}`); process.exitCode = 2; }
}
