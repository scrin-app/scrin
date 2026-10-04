#!/usr/bin/env node
// Writes brand/sheet/comparison.html (concept comparison + legibility) and brand/sheet/motion-demo.html.
// render.mjs screenshots both. Paths are relative to brand/sheet/.
import { mkdirSync, readFileSync, writeFileSync, existsSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { halves } from './geometry.mjs';
import { brandColors as B } from './palette.mjs';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
mkdirSync(join(ROOT, 'sheet'), { recursive: true });
const svg = (rel) => readFileSync(join(ROOT, rel), 'utf8');
const dataUrl = (s) => `data:image/svg+xml;base64,${Buffer.from(s).toString('base64')}`;

const CONCEPTS = [
  { key: 'A-split-s', title: 'A · Split S', idea: 'two halves of one “s” (you + them) meet in the middle bar', tile: 'logo/app-icon-small.svg', micro: 'logo/app-icon-micro.svg', sym: 'logo/symbol-small-on-light.svg', mono: 'logo/symbol-small-mono-black.svg', inv: 'logo/app-icon-inverse.svg' },
  { key: 'B-twin-panes', title: 'B · Twin panes', idea: 'two overlapping screens (local + remote)', tile: 'concepts/B-twin-panes.svg', sym: 'concepts/B-twin-panes-symbol-light.svg' },
  { key: 'C-corner-brackets', title: 'C · Corner brackets', idea: 'capture-frame corners + a cursor block', tile: 'concepts/C-corner-brackets.svg', sym: 'concepts/C-corner-brackets-symbol-light.svg' },
  { key: 'D-screen-code', title: 'D · Screen + code', idea: 'monitor with one-time-code dots (category default)', tile: 'concepts/D-screen-code.svg', sym: 'concepts/D-screen-code-symbol-light.svg' },
];
const SIZES = [16, 24, 32, 48, 128];

// Real neighbours for the taskbar strip (extracted from this machine by .copilot-tmp/taskbar-icons.ps1).
const tbDir = join(ROOT, '..', '.copilot-tmp', 'taskbar');
const neighbours = existsSync(tbDir)
  ? readdirSync(tbDir).filter((f) => f.endsWith('.png')).map((f) => `data:image/png;base64,${readFileSync(join(tbDir, f)).toString('base64')}`)
  : [];

function sizedTile(c, s) {
  const src = s <= 20 && c.micro ? c.micro : c.tile;
  return `<img src="${dataUrl(svg(src))}" width="${s}" height="${s}" alt="">`;
}
function pix(c, s, bg) {
  // Canvas raster at true size, shown 5x nearest-neighbour: what the pixels really are.
  const src = s <= 20 && c.micro ? c.micro : c.tile;
  return `<canvas class="pix" data-src="${dataUrl(svg(src))}" data-size="${s}" data-bg="${bg}" width="${s}" height="${s}" style="width:${s * 5}px;height:${s * 5}px"></canvas>`;
}
function maskCircle(c) {
  // Android adaptive: 108 canvas shown at 144 px; circle mask = 72 dp visible; dashed 66 dp safe zone.
  const inner = c.key === 'A-split-s' ? svg('logo/app-icon-small.svg') : svg(c.tile);
  return `<div class="mask"><img src="${dataUrl(inner)}" width="190" height="190" alt=""><svg class="safe" viewBox="0 0 108 108"><circle cx="54" cy="54" r="33" fill="none" stroke="#e8c76a" stroke-width="0.8" stroke-dasharray="2 2"/></svg></div>`;
}
function taskbar(c, s, dark) {
  const items = [...neighbours.slice(0, 4), dataUrl(svg(s <= 20 && c.micro ? c.micro : c.tile)), ...neighbours.slice(4, 8)];
  return `<div class="taskbar ${dark ? 'tb-dark' : 'tb-light'}">${items.map((u, i) => `<span class="tbi ${i === 4 ? 'me' : ''}"><img src="${u}" width="${s}" height="${s}" alt=""></span>`).join('')}</div>`;
}

const rows = CONCEPTS.map(
  (c) => `
<section class="concept">
  <h2>${c.title}<small>${c.idea}</small></h2>
  <div class="grid">
    <div class="cell dark"><div class="lbl">dark · 16 24 32 48 128</div><div class="sizes">${SIZES.map((s) => sizedTile(c, s)).join('')}</div></div>
    <div class="cell light"><div class="lbl">light · 16 24 32 48 128</div><div class="sizes">${SIZES.map((s) => sizedTile(c, s)).join('')}</div></div>
    <div class="cell dark"><div class="lbl">512 (shown 160)</div><img src="${dataUrl(svg(c.tile))}" width="160" height="160" alt=""></div>
    <div class="cell mid"><div class="lbl">Android circle mask · 66 dp safe</div>${maskCircle(c)}</div>
    <div class="cell"><div class="lbl">taskbar 24 px (100%) · 36 px (150%)</div>${taskbar(c, 24, true)}${taskbar(c, 36, true)}${taskbar(c, 24, false)}</div>
    <div class="cell light"><div class="lbl">symbol on light · one colour</div><img src="${dataUrl(svg(c.sym))}" width="72" height="72" alt="">${c.mono ? `<img src="${dataUrl(svg(c.mono))}" width="72" height="72" alt="">` : `<img class="onecol" src="${dataUrl(svg(c.sym))}" width="72" height="72" alt="">`}</div>
    <div class="cell mid"><div class="lbl">inverse · squint (blur 2px)</div>${c.inv ? `<img src="${dataUrl(svg(c.inv))}" width="72" height="72" alt="">` : `<img class="invert" src="${dataUrl(svg(c.tile))}" width="72" height="72" alt="">`}<img class="squint" src="${dataUrl(svg(c.tile))}" width="72" height="72" alt=""></div>
    <div class="cell"><div class="lbl">true pixels ×5 · 16 / 24 / 32</div>${[16, 24, 32].map((s) => pix(c, s, '#202020')).join('')}</div>
  </div>
</section>`,
).join('');

const legibility = `
<section class="concept">
  <h2>Chosen · A · legibility check (true pixels ×5)<small>micro (16, 20) · small (24–48) · on Windows dark #202020, light #f3f3f3, browser tab #dee1e6, and tray symbol only</small></h2>
  <div class="leg">
    ${['#202020', '#f3f3f3', '#dee1e6'].map((bg) => `<div class="legrow"><span class="lbl">${bg}</span>${[16, 20, 24, 32].map((s) => pix(CONCEPTS[0], s, bg)).join('')}</div>`).join('')}
    <div class="legrow"><span class="lbl">tray dark</span>${[16, 20, 24, 32].map((s) => `<canvas class="pix" data-src="${dataUrl(svg(s <= 20 ? 'logo/symbol-micro-on-dark.svg' : 'logo/symbol-small-on-dark.svg'))}" data-size="${s}" data-bg="#202020" width="${s}" height="${s}" style="width:${s * 5}px;height:${s * 5}px"></canvas>`).join('')}</div>
    <div class="legrow"><span class="lbl">tray light</span>${[16, 20, 24, 32].map((s) => `<canvas class="pix" data-src="${dataUrl(svg(s <= 20 ? 'logo/symbol-micro-on-light.svg' : 'logo/symbol-small-on-light.svg'))}" data-size="${s}" data-bg="#f3f3f3" width="${s}" height="${s}" style="width:${s * 5}px;height:${s * 5}px"></canvas>`).join('')}</div>
  </div>
  <div class="wm">
    <img src="${dataUrl(svg('logo/lockup-horizontal-on-dark.svg'))}" height="64" alt="scrin lockup on dark" style="background:${B.ink.hex};padding:16px;border-radius:12px">
    <img src="${dataUrl(svg('logo/lockup-horizontal-on-light.svg'))}" height="64" alt="scrin lockup on light" style="background:#fff;padding:16px;border-radius:12px">
    <img src="${dataUrl(svg('logo/wordmark-on-light.svg'))}" height="40" alt="" style="background:#fff;padding:16px;border-radius:12px">
    <p class="ro">Conectare la orice ecran, în câteva secunde. <span>ȘȚ șț Ăă Ââ Îî</span><br><em>Connect to any screen, in seconds.</em></p>
  </div>
</section>`;

const PIX_SCRIPT = `
document.fonts.ready.then(async () => {
  for (const c of document.querySelectorAll('canvas.pix')) {
    const s = +c.dataset.size; const img = new Image();
    const raw = atob(c.dataset.src.split(',')[1]).replace(/width="\\d+" height="\\d+"/, 'width="' + s + '" height="' + s + '"');
    img.src = 'data:image/svg+xml;base64,' + btoa(raw); await img.decode();
    const x = c.getContext('2d'); x.fillStyle = c.dataset.bg; x.fillRect(0, 0, s, s); x.drawImage(img, 0, 0, s, s);
  }
  document.body.dataset.ready = '1';
});`;

const FONT_FACE = `
@font-face { font-family: 'Inter Variable'; font-weight: 100 900; src: url('../fonts/Inter-latin.woff2') format('woff2'); unicode-range: U+0000-00FF, U+2000-206F; }
@font-face { font-family: 'Inter Variable'; font-weight: 100 900; src: url('../fonts/Inter-latin-ext.woff2') format('woff2'); unicode-range: U+0100-02BA, U+02BD-02FF; }`;

writeFileSync(
  join(ROOT, 'sheet/comparison.html'),
  `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>scrin — concept comparison sheet</title><style>${FONT_FACE}
body{margin:0;padding:32px;background:#e9eceb;font:14px/1.4 'Inter Variable',system-ui,sans-serif;color:#111816;width:1680px}
h1{margin:0 0 4px;font-size:28px;font-weight:650;letter-spacing:-.01em} .sub{color:#545d5b;margin:0 0 24px}
.concept{background:#fff;border-radius:16px;padding:20px;margin-bottom:20px;box-shadow:0 1px 2px #0001}
h2{margin:0 0 12px;font-size:18px;font-weight:600} h2 small{font-weight:400;color:#545d5b;margin-left:12px;font-size:13px}
.grid{display:grid;grid-template-columns:repeat(4,1fr);gap:12px}
.cell{border-radius:12px;padding:12px;background:#f6fbfa;min-height:120px;display:flex;flex-wrap:wrap;gap:10px;align-items:center;align-content:flex-start}
.cell.dark{background:#0a110f;color:#a4adab} .cell.light{background:#fff;outline:1px solid #e3e8e6} .cell.mid{background:#7d8986;color:#fff}
.lbl{width:100%;font-size:11px;text-transform:uppercase;letter-spacing:.06em;opacity:.8}
.sizes{display:flex;align-items:flex-end;gap:14px}
.mask{position:relative;width:144px;height:144px;border-radius:50%;overflow:hidden;margin:auto}
.mask img{position:absolute;left:-23px;top:-23px} .mask .safe{position:absolute;inset:0;width:144px;height:144px}
.taskbar{display:flex;gap:2px;padding:4px 6px;border-radius:6px;width:100%} .tb-dark{background:#202020} .tb-light{background:#f3f3f3}
.tbi{display:inline-flex;padding:6px;border-radius:4px} .tbi.me{background:#ffffff14;box-shadow:inset 0 -2px #37e1c4}
.tb-light .tbi.me{background:#0000000d;box-shadow:inset 0 -2px #007463}
.onecol{filter:brightness(0)} .invert{filter:invert(1) hue-rotate(180deg)} .squint{filter:blur(2px)}
canvas.pix{image-rendering:pixelated;margin-right:10px;border-radius:2px}
.leg{display:flex;flex-direction:column;gap:10px} .legrow{display:flex;align-items:flex-end;gap:6px} .legrow .lbl{width:90px}
.wm{display:flex;gap:24px;align-items:center;margin-top:16px;flex-wrap:wrap}
.ro{font-size:22px;font-weight:550;margin:0} .ro span{color:#006455} .ro em{font-size:15px;font-weight:400;color:#545d5b}
</style></head><body>
<h1>scrin — logo concept comparison</h1><p class="sub">brand pack 0.1.0 · 2026-10-04 · tiles on the ink plate, lagoon accent (oklch 0.82 0.14 178) · A uses its micro optical size at 16–20 px; B–D have no micro variant (as drawn)</p>
${rows}${legibility}
<script>${PIX_SCRIPT}</script></body></html>`,
);

// ---------------------------------------------------------------- motion demo + storyboard
const s = halves({ stroke: 3 });
const markInline = (state, extra = '') =>
  `<svg viewBox="0 0 24 24" width="120" height="120" ${extra}><g transform="translate(12 12) scale(0.86) translate(-12 -12)"><g class="scrin-mark" data-state="${state}" fill="none" stroke-width="3" stroke-linejoin="round"><path class="scrin-mark__ivory" pathLength="1" d="${s.ivory}" stroke="${B.ivory.hex}"/><path class="scrin-mark__lagoon" pathLength="1" d="${s.lagoon}" stroke="${B['lagoon-bright'].hex}"/></g></g></svg>`;
// Static reference (no class, no animation) for the final-frame == static check.
const markStatic = `<svg id="static-ref" viewBox="0 0 24 24" width="120" height="120"><g transform="translate(12 12) scale(0.86) translate(-12 -12)"><g fill="none" stroke-width="3" stroke-linejoin="round"><path d="${s.ivory}" stroke="${B.ivory.hex}"/><path d="${s.lagoon}" stroke="${B['lagoon-bright'].hex}"/></g></g></svg>`;
const motion = JSON.parse(readFileSync(join(ROOT, 'tokens/motion.tokens.json'), 'utf8')).logo.$extensions['app.scrin.logo'];
const totalIntro = motion.intro.draw + motion.intro.stagger;
const storyboard = (state, total, n) =>
  Array.from({ length: n }, (_, i) => {
    const t = Math.round((total * i) / (n - 1));
    return `<figure>${markInline(state, `style="--t:-${t}ms" class="frame"`)}<figcaption>${state} ${t} ms</figcaption></figure>`;
  }).join('');
writeFileSync(
  join(ROOT, 'sheet/motion-demo.html'),
  `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>scrin — logo motion demo</title>
<link rel="stylesheet" href="../dist/motion/logo.css"><style>${FONT_FACE}
body{margin:0;padding:32px;background:${B.ink.hex};color:${B.ivory.hex};font:14px/1.4 'Inter Variable',system-ui,sans-serif;width:1500px}
h1{font-size:26px;margin:0 0 4px;font-weight:650} p{color:#a4adab;margin:0 0 20px}
.live{display:flex;gap:24px;margin-bottom:28px} .live figure{margin:0;text-align:center} .live button{margin-top:8px;font:inherit;background:#0b221d;color:#71e2ca;border:1px solid #00cdb1;border-radius:8px;padding:6px 14px;cursor:pointer}
.live button:focus-visible{outline:2px solid #37e1c4;outline-offset:2px}
.row{display:flex;gap:10px;margin-bottom:18px;align-items:center} .row h2{width:110px;font-size:14px;margin:0;color:#71e2ca}
.row figure{margin:0;text-align:center;background:#0a110f;border-radius:10px;padding:6px} figcaption{font-size:11px;color:#a4adab}
.frame .scrin-mark, .frame .scrin-mark path{animation-play-state:paused !important;animation-delay:var(--t) !important}
.hover-demo{display:inline-block;border-radius:16px;padding:4px} .hover-demo:focus-visible{outline:2px solid #37e1c4}
</style></head><body>
<h1>scrin — logo motion (Gate E demo)</h1>
<p>One source: tokens/motion.tokens.json → logo.* → dist/motion/logo.css, logo-motion.ts (Motion 14), logo-intro.svg, logo-thinking.svg, android avd_scrin_splash.xml. Reduced motion: final frame, no transforms; thinking = 2-step opacity pulse.</p>
<div class="live">
${['intro', 'thinking', 'success', 'error'].map((st) => `<figure id="live-${st}">${markInline(st)}<br><button type="button" data-replay="${st}">Replay ${st}</button></figure>`).join('')}
<figure><a class="hover-demo scrin-mark-hover" href="#" aria-label="hover and press demo">${markInline('idle')}</a><br><span>hover / press</span></figure>
<figure>${markStatic}<br><span>static mark</span></figure>
</div>
<div class="row"><h2>intro ${totalIntro} ms</h2>${storyboard('intro', totalIntro, 7)}</div>
<div class="row"><h2>thinking ${motion.thinking.period} ms</h2>${storyboard('thinking', motion.thinking.period, 7)}</div>
<div class="row"><h2>success ${motion.success.duration} ms</h2>${storyboard('success', motion.success.duration, 5)}</div>
<div class="row"><h2>error ${motion.error.duration} ms</h2>${storyboard('error', motion.error.duration, 5)}</div>
<script>
for (const b of document.querySelectorAll('[data-replay]')) b.addEventListener('click', () => {
  const g = document.querySelector('#live-' + b.dataset.replay + ' .scrin-mark');
  const st = g.dataset.state; g.dataset.state = 'idle'; void g.getBoundingClientRect(); g.dataset.state = st;
});
document.body.dataset.ready = '1';
</script></body></html>`,
);
console.log('sheet.mjs: wrote sheet/comparison.html, sheet/motion-demo.html');
