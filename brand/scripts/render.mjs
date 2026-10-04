#!/usr/bin/env node
// Rasterises the SVG masters into every platform export (PNG / WebP / ICO) and screenshots the
// comparison sheet + motion demo. Uses Chromium from the repo's Playwright install (apps/web).
//
//   node brand/scripts/render.mjs            (run build.mjs and sheet.mjs first)
/* global document, Image -- page.evaluate callbacks run in Chromium */
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { halves, microHalves } from './geometry.mjs';
import { brandColors as B } from './palette.mjs';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(join(ROOT, '..', 'apps', 'web', 'package.json'));
const { chromium } = require('@playwright/test');

const svg = (rel) => readFileSync(join(ROOT, rel), 'utf8');
const written = [];
const fontLog = [];
function out(rel, buf) {
  const p = join(ROOT, rel);
  mkdirSync(dirname(p), { recursive: true });
  writeFileSync(p, buf);
  written.push(`${rel} (${buf.length} B)`);
}

/** ICO with PNG-compressed entries (Vista+). Order: as given (Tauri wants 32 first). */
function ico(pngs) {
  const head = Buffer.alloc(6 + 16 * pngs.length);
  head.writeUInt16LE(0, 0);
  head.writeUInt16LE(1, 2);
  head.writeUInt16LE(pngs.length, 4);
  let offset = head.length;
  pngs.forEach(({ size, buf }, i) => {
    const e = 6 + i * 16;
    head.writeUInt8(size >= 256 ? 0 : size, e);
    head.writeUInt8(size >= 256 ? 0 : size, e + 1);
    head.writeUInt16LE(1, e + 4); // planes
    head.writeUInt16LE(32, e + 6); // bpp
    head.writeUInt32LE(buf.length, e + 8);
    head.writeUInt32LE(offset, e + 12);
    offset += buf.length;
  });
  return Buffer.concat([head, ...pngs.map((p) => p.buf)]);
}

// ---------------------------------------------------------------- SVG compositions
const DARK = [B.ivory.hex, B['lagoon-bright'].hex];
const LIGHT = [B['ink-text'].hex, B['lagoon-deep'].hex];
const markPaths = (src, colors) =>
  `<path d="${src.ivory}" stroke="${colors[0]}"/><path d="${src.lagoon}" stroke="${colors[1]}"/>`;
const plate = (w, h) =>
  `<defs><linearGradient id="t" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${B['ink-hi'].hex}"/><stop offset="0.7" stop-color="${B.ink.hex}"/></linearGradient></defs><rect width="${w}" height="${h}" fill="url(#t)"/>`;

/** Full-bleed square with the mark at `art` fraction of the side (for maskable / apple / Play). */
function fullBleed(art, { bg = 'ink', colors = DARK } = {}) {
  const s = halves({ stroke: 3 });
  const k = (art * 24) / 16; // the s is 16 units tall
  const bgEl = bg === 'ink' ? plate(24, 24) : `<rect width="24" height="24" fill="${bg}"/>`;
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">${bgEl}<g fill="none" stroke-width="3" stroke-linejoin="round" transform="translate(12 12) scale(${k}) translate(-12 -12)">${markPaths(s, colors)}</g></svg>`;
}
/** Transparent symbol, optional status dot (tray). */
function traySvg(size, colors, state) {
  const micro = size <= 20;
  const src = micro ? microHalves() : halves({ stroke: 3 });
  const g = micro ? 16 : 24;
  const scale = micro ? 1 : 1.3; // fill the tray box (Windows tray glyphs are ~full bleed)
  const c = g / 2;
  let dot = '';
  if (state === 'active') {
    const r = g * 0.17;
    dot = `<circle cx="${g - r}" cy="${g - r}" r="${r + g * 0.06}" fill="${colors === DARK ? '#202020' : '#f3f3f3'}"/><circle cx="${g - r}" cy="${g - r}" r="${r}" fill="${colors[1]}"/>`;
  }
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${g} ${g}"><g fill="none" stroke-width="${src.stroke}" stroke-linejoin="round" transform="translate(${c} ${c}) scale(${scale}) translate(${-c} ${-c})">${markPaths(src, colors)}</g>${dot}</svg>`;
}
const tile = (size) => svg(size <= 20 ? 'logo/app-icon-micro.svg' : size < 64 ? 'logo/app-icon-small.svg' : 'logo/app-icon-master.svg');

// Windows MSIX unplated: symbol only (no plate), light/dark versions.
const unplated = (size, colors) => {
  const micro = size <= 20;
  const src = micro ? microHalves() : halves({ stroke: 3 });
  const g = micro ? 16 : 24;
  const c = g / 2;
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${g} ${g}"><g fill="none" stroke-width="${src.stroke}" stroke-linejoin="round" transform="translate(${c} ${c}) scale(${micro ? 1 : 1.2}) translate(${-c} ${-c})">${markPaths(src, colors)}</g></svg>`;
};

// Favicon SVG: micro geometry on the plate; dark media query swaps nothing on the plate but
// lifts the rim so the tile separates from dark tab strips.
const faviconSvg = svg('logo/app-icon-micro.svg')
  .replace(/ width="512" height="512"/, '')
  .replace('<defs>', `<style>.rim{stroke-opacity:0}@media (prefers-color-scheme: dark){.rim{stroke-opacity:1}}</style><defs>`);
const stagingSvg = svg('logo/app-icon-staging.svg').replace(/ width="512" height="512"/, '');

const browser = await chromium.launch();
const page = await browser.newPage();
await page.setContent('<html><body></body></html>');

async function raster(svgText, w, h = w, type = 'image/png', quality = 0.92) {
  const b64 = await page.evaluate(
    async ({ s, w: cw, h: ch, type: mime, quality: q }) => {
      const sized = s.replace(/<svg[^>]*>/, (tag) => tag.replace(/ (width|height)="[^"]*"/g, '').replace('<svg', `<svg width="${cw}" height="${ch}"`));
      const img = new Image();
      img.src = 'data:image/svg+xml;base64,' + btoa(unescape(encodeURIComponent(sized)));
      await img.decode();
      const c = document.createElement('canvas');
      c.width = cw;
      c.height = ch;
      const x = c.getContext('2d');
      x.imageSmoothingQuality = 'high';
      x.drawImage(img, 0, 0, cw, ch);
      return c.toDataURL(mime, q).split(',')[1];
    },
    { s: svgText, w, h, type, quality },
  );
  return Buffer.from(b64, 'base64');
}

// ---------------------------------------------------------------- web
out('dist/web/icon.svg', Buffer.from(faviconSvg));
out('dist/web/icon-staging.svg', Buffer.from(stagingSvg));
out('dist/web/favicon.ico', ico([{ size: 32, buf: await raster(tile(32), 32) }, { size: 16, buf: await raster(tile(16), 16) }]));
out('dist/web/favicon-staging.ico', ico([{ size: 32, buf: await raster(stagingSvg, 32) }, { size: 16, buf: await raster(stagingSvg, 16) }]));
out('dist/web/apple-touch-icon.png', await raster(fullBleed(0.5), 180)); // opaque, s = 90 px tall, ~140 px safe art box
out('dist/web/icon-192.png', await raster(tile(192), 192));
out('dist/web/icon-512.png', await raster(tile(512), 512));
out('dist/web/icon-maskable-512.png', await raster(fullBleed(0.5), 512)); // s diag within 40 % radius circle
out('dist/web/icon-maskable-192.png', await raster(fullBleed(0.5), 192));

// ---------------------------------------------------------------- windows / tauri
const icoSizes = [32, 16, 20, 24, 40, 48, 64, 256];
const icoEntries = [];
for (const s of icoSizes) icoEntries.push({ size: s, buf: await raster(tile(s), s) });
out('dist/windows/icon.ico', ico(icoEntries));
for (const [name, s] of [['32x32.png', 32], ['64x64.png', 64], ['128x128.png', 128], ['128x128@2x.png', 256], ['icon.png', 512], ['icon-1024.png', 1024]]) {
  out(`dist/windows/tauri/${name}`, await raster(tile(s), s));
}
for (const s of [30, 44, 71, 89, 107, 142, 150, 284, 310]) out(`dist/windows/tauri/Square${s}x${s}Logo.png`, await raster(tile(s), s));
out('dist/windows/tauri/StoreLogo.png', await raster(tile(50), 50));
for (const s of [16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 256]) {
  out(`dist/windows/msix/AppList.targetsize-${s}.png`, await raster(tile(s), s));
  out(`dist/windows/msix/AppList.targetsize-${s}_altform-unplated.png`, await raster(unplated(s, DARK), s));
  out(`dist/windows/msix/AppList.targetsize-${s}_altform-lightunplated.png`, await raster(unplated(s, LIGHT), s));
}

// ---------------------------------------------------------------- tray (dark + light taskbar, idle + active)
for (const s of [16, 20, 24, 32, 40, 48]) {
  for (const { theme, colors } of [{ theme: 'dark', colors: DARK }, { theme: 'light', colors: LIGHT }]) {
    out(`dist/tray/tray-${theme}-${s}.png`, await raster(traySvg(s, colors, 'idle'), s));
    out(`dist/tray/tray-${theme}-active-${s}.png`, await raster(traySvg(s, colors, 'active'), s));
  }
}
const trayIco = async (theme, colors, state) =>
  ico(await Promise.all([16, 20, 24, 32, 40, 48].map(async (s) => ({ size: s, buf: await raster(traySvg(s, colors, state), s) }))));
out('dist/tray/tray-dark.ico', await trayIco('dark', DARK, 'idle'));
out('dist/tray/tray-light.ico', await trayIco('light', LIGHT, 'idle'));
out('dist/tray/tray-dark-active.ico', await trayIco('dark', DARK, 'active'));
out('dist/tray/tray-light-active.ico', await trayIco('light', LIGHT, 'active'));

// ---------------------------------------------------------------- android
for (const [d, s] of [['mdpi', 48], ['hdpi', 72], ['xhdpi', 96], ['xxhdpi', 144], ['xxxhdpi', 192]]) {
  // Legacy launcher (API < 26 never used with minSdk 26, kept for launchers that ignore adaptive).
  out(`android/mipmap-${d}/ic_launcher.webp`, await raster(tile(s), s, s, 'image/webp', 0.95));
  out(`android/mipmap-${d}/ic_launcher_round.webp`, await raster(tile(s).replace(/rx="[\d.]+"/g, 'rx="11"'), s, s, 'image/webp', 0.95));
}
out('android/play/ic_launcher-playstore-512.png', await raster(fullBleed(0.62), 512));
// Splash icon (no bg): 288 dp canvas, mark within 192 dp circle -> xxxhdpi 1152 px; ship 4x density source.
out('android/drawable-nodpi/splash_icon_288dp_xxxhdpi.png', await raster(svg('logo/symbol-small-on-dark.svg'), 1152));

// ---------------------------------------------------------------- HTML templates (text): OG + TV banner
const fontsUrl = pathToFileURL(join(ROOT, 'fonts')).href;
const FONT = `@font-face{font-family:'Inter Variable';font-weight:100 900;src:url('${fontsUrl}/Inter-latin.woff2') format('woff2');unicode-range:U+0000-00FF,U+2000-206F}
@font-face{font-family:'Inter Variable';font-weight:100 900;src:url('${fontsUrl}/Inter-latin-ext.woff2') format('woff2');unicode-range:U+0100-02BA,U+02BD-02FF}`;
const lockDark = svg('logo/lockup-horizontal-on-dark.svg');
const dataUrl = (s) => `data:image/svg+xml;base64,${Buffer.from(s).toString('base64')}`;
async function shot(html, w, h, rel, extra = []) {
  const p = await browser.newPage({ viewport: { width: w, height: h }, deviceScaleFactor: 1 });
  // Load from a file:// URL (not about:blank) so the file:// @font-face sources are same-origin.
  const tmp = join(ROOT, 'sheet', `.tmp-${rel.replace(/[\\/]/g, '_')}.html`);
  writeFileSync(tmp, html);
  await p.goto(pathToFileURL(tmp).href, { waitUntil: 'load' });
  const fontState = await p.evaluate(async () => {
    await document.fonts.ready;
    const text = document.body.innerText;
    await document.fonts.load("650 64px 'Inter Variable'", text);
    return [...document.fonts].filter((f) => f.status === 'loaded').length;
  });
  if (/[ăâîșțĂÂÎȘȚ]/.test(html) && fontState < 2) throw new Error(`${rel}: Inter latin + latin-ext not both loaded (${fontState})`);
  fontLog.push(`${rel}: ${fontState} Inter subset(s) loaded`);
  const buf = await p.screenshot({ type: 'png' });
  rmSync(tmp);
  out(rel, buf);
  for (const [r, type] of extra) {
    const b64 = await page.evaluate(
      async ({ b, type: mime }) => {
        const img = new Image();
        img.src = 'data:image/png;base64,' + b;
        await img.decode();
        const c = document.createElement('canvas');
        c.width = img.width;
        c.height = img.height;
        c.getContext('2d').drawImage(img, 0, 0);
        return c.toDataURL(mime, 0.9).split(',')[1];
      },
      { b: buf.toString('base64'), type },
    );
    out(r, Buffer.from(b64, 'base64'));
  }
  await p.close();
}
const ogHtml = (lang, title, sub) => `<!doctype html><html lang="${lang}"><head><meta charset="utf-8"><style>${FONT}
body{margin:0;width:1200px;height:630px;background:linear-gradient(135deg,${B['ink-hi'].hex} 0%,${B.ink.hex} 65%);font-family:'Inter Variable',sans-serif;color:${B.ivory.hex};position:relative;overflow:hidden}
.lock{position:absolute;left:96px;top:92px;height:96px}
h1{position:absolute;left:96px;top:250px;margin:0;font-size:64px;line-height:1.08;font-weight:650;letter-spacing:-.02em;width:860px}
p{position:absolute;left:96px;top:430px;margin:0;font-size:28px;color:#a4adab;width:900px}
.ghost{position:absolute;right:-120px;bottom:-150px;width:620px;opacity:.10}
</style></head><body><img class="lock" src="${dataUrl(lockDark)}"><h1>${title}</h1><p>${sub}</p><img class="ghost" src="${dataUrl(svg('logo/symbol-master-on-dark.svg'))}"></body></html>`;
await shot(ogHtml('en', 'Connect to any screen, in seconds.', 'Open-source remote desktop · end-to-end encrypted · AGPL-3.0'), 1200, 630, 'dist/og/og-en.png', [['dist/og/og-en.webp', 'image/webp']]);
await shot(ogHtml('ro', 'Conectare la orice ecran, în câteva secunde.', 'Desktop la distanță open-source · criptat cap-coadă · AGPL-3.0'), 1200, 630, 'dist/og/og-ro.png', [['dist/og/og-ro.webp', 'image/webp']]);
const tvHtml = `<!doctype html><html><head><meta charset="utf-8"><style>${FONT}
body{margin:0;width:320px;height:180px;background:linear-gradient(135deg,${B['ink-hi'].hex},${B.ink.hex} 70%);display:flex;align-items:center;justify-content:center}
img{height:64px}</style></head><body><img src="${dataUrl(lockDark)}"></body></html>`;
await shot(tvHtml, 320, 180, 'android/drawable-xhdpi/tv_banner.png');
const tvHtml2 = tvHtml.replace('width:320px;height:180px', 'width:640px;height:360px').replace('height:64px', 'height:128px');
await shot(tvHtml2, 640, 360, 'android/drawable-xxxhdpi/tv_banner.png');

// ---------------------------------------------------------------- sheets
async function shotFile(rel, png, width) {
  const p = await browser.newPage({ viewport: { width, height: 900 }, deviceScaleFactor: 1 });
  await p.goto(pathToFileURL(join(ROOT, rel)).href);
  await p.waitForFunction(() => document.body.dataset.ready === '1');
  await p.waitForTimeout(300);
  out(png, await p.screenshot({ fullPage: true, type: 'png' }));
  await p.close();
}
await shotFile('sheet/comparison.html', 'sheet/comparison.png', 1744);
await shotFile('sheet/motion-demo.html', 'sheet/motion-storyboard.png', 1564);

await browser.close();
writeFileSync(join(ROOT, 'dist/render-manifest.txt'), written.join('\n') + '\n');
console.log(`render.mjs: wrote ${written.length} files\n${fontLog.join('\n')}`);
