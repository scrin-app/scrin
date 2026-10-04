#!/usr/bin/env node
// Icon legibility gate (numbers, not eyeballing). For every concept tile and the shipped tray /
// favicon rasters at 16/24/32 px it measures, on the real background:
//   ink   - share of the icon box that is mark (not plate/background)
//   runs  - stroke runs crossed by the centre column and by a column through each stem; the
//           split-S must keep 3 separate bars (top / middle / bottom) = counters stay open
//   minC  - WCAG contrast of the median mark pixel against the plate (>= 3 = non-text AA)
// It also checks lockup / wordmark geometry with getBBox (no overlaps, inside the viewBox).
// Exit 1 if the chosen mark (A) loses a counter or drops below 3:1 at any size.
//
//   node brand/scripts/legibility.mjs
/* global document, Image -- page.evaluate callbacks run in Chromium */
import { readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(join(ROOT, '..', 'apps', 'web', 'package.json'));
const { chromium } = require('@playwright/test');
const svg = (rel) => readFileSync(join(ROOT, rel), 'utf8');

const CASES = [
  { id: 'A tile', pick: (s) => (s <= 20 ? 'logo/app-icon-micro.svg' : 'logo/app-icon-small.svg'), bg: '#202020', gate: true },
  { id: 'A tile on light', pick: (s) => (s <= 20 ? 'logo/app-icon-micro.svg' : 'logo/app-icon-small.svg'), bg: '#f3f3f3', gate: true },
  { id: 'A tray dark', pick: (s) => (s <= 20 ? 'logo/symbol-micro-on-dark.svg' : 'logo/symbol-small-on-dark.svg'), bg: '#202020', gate: true, noPlate: true },
  { id: 'A tray light', pick: (s) => (s <= 20 ? 'logo/symbol-micro-on-light.svg' : 'logo/symbol-small-on-light.svg'), bg: '#f3f3f3', gate: true, noPlate: true },
  { id: 'A mono black', pick: (s) => (s <= 20 ? 'logo/symbol-micro-mono-black.svg' : 'logo/symbol-small-mono-black.svg'), bg: '#ffffff', gate: true, noPlate: true },
  { id: 'B twin panes', pick: () => 'concepts/B-twin-panes.svg', bg: '#202020' },
  { id: 'C corner brackets', pick: () => 'concepts/C-corner-brackets.svg', bg: '#202020' },
  { id: 'D screen + code', pick: () => 'concepts/D-screen-code.svg', bg: '#202020' },
];
const SIZES = [16, 24, 32];

const browser = await chromium.launch();
const page = await browser.newPage();
await page.setContent('<html><body></body></html>');

const results = [];
for (const c of CASES) {
  for (const size of SIZES) {
    const r = await page.evaluate(
      async ({ s, size: dim, bg, noPlate }) => {
        const sized = s.replace(/<svg[^>]*>/, (t) => t.replace(/ (width|height)="[^"]*"/g, '').replace('<svg', `<svg width="${dim}" height="${dim}"`));
        const img = new Image();
        img.src = 'data:image/svg+xml;base64,' + btoa(unescape(encodeURIComponent(sized)));
        await img.decode();
        const cv = document.createElement('canvas');
        cv.width = cv.height = dim;
        const x = cv.getContext('2d');
        x.fillStyle = bg;
        x.fillRect(0, 0, dim, dim);
        x.drawImage(img, 0, 0);
        const d = x.getImageData(0, 0, dim, dim).data;
        // Serialised into the browser by page.evaluate: helpers must live inside the callback.
        // oxlint-disable-next-line unicorn/consistent-function-scoping
        const lin = (v) => ((v /= 255) <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
        const Y = (i) => 0.2126 * lin(d[i]) + 0.7152 * lin(d[i + 1]) + 0.0722 * lin(d[i + 2]);
        const at = (px, py) => Y((py * dim + px) * 4);
        // Plate luminance = the pixel 25 % in from the top-left (inside the plate, outside the mark).
        const plateY = noPlate ? at(0, 0) : at(Math.round(dim * 0.2), Math.round(dim * 0.82));
        // oxlint-disable-next-line unicorn/consistent-function-scoping -- serialised into the browser
        const cr = (a, b) => (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
        const isMark = (px, py) => cr(at(px, py), plateY) >= 1.8;
        let markPx = 0;
        const cs = [];
        for (let py = 0; py < dim; py++)
          for (let px = 0; px < dim; px++)
            if (isMark(px, py)) {
              markPx++;
              cs.push(cr(at(px, py), plateY));
            }
        cs.sort((a, b) => a - b);
        const runs = (px) => {
          let n = 0;
          let prev = false;
          // Plate cases: scan rows inside the plate only (1/24 inset + corner radius), so the
          // page background outside a dark tile is not counted as mark.
          const y0 = noPlate ? 0 : Math.ceil(dim * 0.12);
          const y1 = noPlate ? dim : Math.floor(dim * 0.88);
          for (let py = y0; py < y1; py++) {
            const m = isMark(px, py);
            if (m && !prev) n++;
            prev = m;
          }
          return n;
        };
        const mid = Math.floor(dim / 2) - (dim % 2 === 0 ? 1 : 0);
        return {
          ink: +(markPx / (dim * dim)).toFixed(3),
          runsCentre: Math.max(runs(mid), runs(mid + 1)),
          medianC: cs.length ? +cs[Math.floor(cs.length / 2)].toFixed(2) : 0,
        };
      },
      { s: svg(c.pick(size)), size, bg: c.bg, noPlate: !!c.noPlate },
    );
    results.push({ case: c.id, size, ...r, gate: !!c.gate });
  }
}

// Geometry checks (lockup + wordmark): bounding boxes in user units.
const geo = await page.evaluate(
  ({ lockup, wm }) => {
    const host = document.createElement('div');
    document.body.append(host);
    const box = (s) => {
      host.innerHTML = s;
      const root = host.querySelector('svg');
      const vb = root.viewBox.baseVal;
      const kids = [...root.querySelectorAll('path, rect')].map((el) => {
        const b = el.getBBox();
        const m = el.getCTM();
        const sx = m ? m.a : 1;
        const tx = m ? m.e : 0;
        const ty = m ? m.f : 0;
        const rootM = root.getCTM();
        const rs = rootM ? rootM.a : 1;
        return { x: (b.x * sx + tx - (rootM ? rootM.e : 0)) / rs, y: (b.y * sx + ty - (rootM ? rootM.f : 0)) / rs, w: (b.width * sx) / rs, h: (b.height * sx) / rs };
      });
      return { vb: { x: vb.x, y: vb.y, w: vb.width, h: vb.height }, kids };
    };
    return { lockup: box(lockup), wm: box(wm) };
  },
  { lockup: svg('logo/lockup-horizontal-on-dark.svg'), wm: svg('logo/wordmark-on-dark.svg') },
);
// Wordmark raster: at 20 px x-height (the smallest lockup size in BRAND.md) the 5 letters must stay
// 5 separate ink islands along x (4 empty column runs between them).
const wmCols = await page.evaluate(
  async ({ s }) => {
    const h = 27; // viewBox height 21.5 units -> x-height 16 units = 20 px
    const sized = s.replace(/<svg[^>]*>/, (t) => t.replace(/ (width|height)="[^"]*"/g, '').replace('<svg', `<svg height="${h}"`));
    const img = new Image();
    img.src = 'data:image/svg+xml;base64,' + btoa(unescape(encodeURIComponent(sized)));
    await img.decode();
    const cv = document.createElement('canvas');
    cv.width = img.width;
    cv.height = h;
    const x = cv.getContext('2d');
    x.drawImage(img, 0, 0);
    const d = x.getImageData(0, 0, cv.width, h).data;
    const colInk = [];
    for (let px = 0; px < cv.width; px++) {
      let a = 0;
      for (let py = 0; py < h; py++) a = Math.max(a, d[(py * cv.width + px) * 4 + 3]);
      colInk.push(a > 40);
    }
    let islands = 0;
    let prev = false;
    const gaps = [];
    let gap = 0;
    for (const c of colInk) {
      if (c && !prev) {
        islands++;
        if (islands > 1) gaps.push(gap);
      }
      gap = c ? 0 : gap + 1;
      prev = c;
    }
    return { width: cv.width, islands, gaps };
  },
  { s: svg('logo/wordmark-on-dark.svg') },
);
await browser.close();

const lines = [];
let fail = 0;
lines.push('case                 size  ink    runs(centre)  medianC  verdict');
for (const r of results) {
  const needRuns = r.case.startsWith('A') ? 3 : null;
  const bad = r.gate && ((needRuns && r.runsCentre !== needRuns) || r.medianC < 3);
  if (bad) fail++;
  lines.push(
    `${r.case.padEnd(20)} ${String(r.size).padStart(4)}  ${r.ink.toFixed(3)}  ${String(r.runsCentre).padStart(12)}  ${r.medianC.toFixed(2).padStart(7)}  ${r.gate ? (bad ? 'FAIL' : 'PASS') : 'info'}`,
  );
}
// Wordmark/lockup sanity: everything inside the viewBox (0.5 unit stroke tolerance).
for (const [name, g] of Object.entries(geo)) {
  const out = g.kids.filter((k) => k.x < g.vb.x - 2 || k.y < g.vb.y - 2 || k.x + k.w > g.vb.x + g.vb.w + 2 || k.y + k.h > g.vb.y + g.vb.h + 2);
  lines.push(`${name}: ${g.kids.length} shapes, ${out.length} outside viewBox ${JSON.stringify(g.vb)} -> ${out.length ? 'FAIL' : 'PASS'}`);
  if (out.length) fail++;
  // Letter overlap (fill-box intersections between different letters, ignoring stroke half-width).
  if (name === 'wm') {
    const ov = [];
    for (let i = 0; i < 5; i++)
      for (let j = i + 1; j < 5; j++) {
        const a = g.kids[i];
        const b = g.kids[j];
        const gap = b.x - (a.x + a.w);
        if (j === i + 1) ov.push(`${'scrin'[i]}${'scrin'[j]} gap ${gap.toFixed(2)}`);
      }
    lines.push(`wordmark letter gaps (centreline units; visible gap = minus stroke half-widths): ${ov.join(' / ')}`);
  }
}
{
  const okWm = wmCols.islands === 5 && wmCols.gaps.every((g) => g >= 2);
  lines.push(`wordmark raster @ 20 px x-height: ${wmCols.islands} letter islands, visible gaps ${wmCols.gaps.join('/')} px -> ${okWm ? 'PASS' : 'FAIL'}`);
  if (!okWm) fail++;
}
lines.push(fail ? `RESULT: FAIL (${fail})` : 'RESULT: PASS - chosen mark keeps 3 bars (both counters open) and >= 3:1 at 16/24/32 px');
const text = lines.join('\n');
writeFileSync(join(ROOT, 'sheet', 'legibility.txt'), text + '\n');
console.log(text);
process.exit(fail ? 1 : 0);
