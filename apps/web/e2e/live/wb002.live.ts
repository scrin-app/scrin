/**
 * WB-002 live: the built SPA in real Chrome connects through a real
 * scrin-server gateway to a real Windows host with the one-time code, shows
 * the SAS, decodes at least one H.264 frame onto the canvas and sends a key
 * that the host injects with SendInput.
 *
 * Run: `pwsh -NoProfile -File apps/web/e2e/live/run-live.ps1` (builds what is
 * missing through the queue). Windows only: the host captures this desktop.
 */
import { execFileSync } from 'node:child_process';

import { expect, test, type Page } from '@playwright/test';

import { startHarness, type Harness } from './harness';

let h: Harness;

test.beforeAll(async () => {
  h = await startHarness();
});

test.afterEach(async ({ page: _page }, info) => {
  if (info.status !== info.expectedStatus) {
    await info.attach('processes.log', { body: h.logs(), contentType: 'text/plain' });
  }
});

test.afterAll(async () => {
  await h.stop();
});

/** Scroll Lock state of this Windows session (the host injects into it). */
function scrollLockOn(): boolean {
  const out = execFileSync(
    'powershell.exe',
    [
      '-NoProfile',
      '-Command',
      'Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.Control]::IsKeyLocked(145)',
    ],
    { encoding: 'utf8' },
  );
  return out.trim() === 'True';
}

/** Distinct colours in a screenshot of the canvas (blank = 1). */
async function distinctColours(page: Page): Promise<number> {
  const png = await page.locator('canvas').screenshot();
  return page.evaluate(async (b64: string) => {
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    const bmp = await createImageBitmap(new Blob([bytes], { type: 'image/png' }));
    const c = new OffscreenCanvas(bmp.width, bmp.height);
    const ctx = c.getContext('2d');
    if (!ctx) return 0;
    ctx.drawImage(bmp, 0, 0);
    const px = ctx.getImageData(0, 0, bmp.width, bmp.height).data;
    const seen = new Set<number>();
    for (let i = 0; i < px.length && seen.size < 64; i += 4 * 37) {
      seen.add(((px[i] ?? 0) << 16) | ((px[i + 1] ?? 0) << 8) | (px[i + 2] ?? 0));
    }
    return seen.size;
  }, png.toString('base64'));
}

test('browser to gateway to Windows host: code, SAS, video, key', async ({ page }) => {
  test.setTimeout(180_000);
  const host = h.host();
  expect(host?.online).toBe(true);
  if (!host) return;

  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));

  // `SCRIN_LIVE_TRANSPORT=ws` hides WebTransport so the client takes the WebSocket fallback.
  if (process.env.SCRIN_LIVE_TRANSPORT === 'ws') {
    await page.addInitScript(() => {
      Reflect.deleteProperty(globalThis, 'WebTransport');
    });
  }
  await page.emulateMedia({ reducedMotion: 'reduce' });
  // `?server=` points the SPA at the harness origin (front door + gateway).
  await page.goto(`${h.web}/?server=${encodeURIComponent(h.web)}`);
  await page.getByLabel('Partner ID').fill(host.id);
  await page.getByLabel('One-time code', { exact: true }).last().fill(host.code);
  await page.getByRole('button', { name: 'Connect', exact: true }).click();

  // SAS: five emoji from the real SPAKE2 transcript.
  await expect(page.getByRole('heading', { name: 'Verify these emoji' })).toBeVisible({
    timeout: 45_000,
  });
  const sas = page.getByRole('list', { name: /^Verification emoji: / });
  await expect(sas.getByRole('listitem')).toHaveCount(5);
  await page.getByRole('button', { name: 'They match' }).click();

  // The host accepts after its anti-scam delay (5 s); then video flows.
  await expect(page.getByRole('toolbar', { name: 'Session toolbar' })).toBeVisible({
    timeout: 45_000,
  });
  await expect(page.getByText('Waiting for the first frame')).toBeHidden({ timeout: 45_000 });
  const size = await page
    .locator('canvas')
    .evaluate((c: HTMLCanvasElement) => ({ w: c.width, h: c.height }));
  expect(size.w).toBeGreaterThan(300);
  expect(size.h).toBeGreaterThan(150);
  await expect.poll(() => distinctColours(page), { timeout: 15_000 }).toBeGreaterThan(4);

  // Key: Scroll Lock (HID 0x47) toggles host state we can read back, and a
  // second press restores it.
  const before = scrollLockOn();
  await page.locator('canvas').focus();
  await page.keyboard.press('ScrollLock');
  await expect.poll(scrollLockOn, { timeout: 10_000 }).toBe(!before);
  await page.keyboard.press('ScrollLock');
  await expect.poll(scrollLockOn, { timeout: 10_000 }).toBe(before);

  // Which transport carried the session: the front door counts WebSocket upgrades.
  const transport =
    h.wsUpgrades() === 0 ? 'webtransport' : `websocket (${h.wsUpgrades()} upgrades)`;
  test.info().annotations.push({ type: 'transport', description: transport });
  process.stdout.write(`LIVE transport=${transport}\n`);
  if (process.env.SCRIN_LIVE_TRANSPORT === 'ws') expect(h.wsUpgrades()).toBeGreaterThan(0);
  else expect(h.wsUpgrades()).toBe(0);
  expect(errors).toEqual([]);
});
