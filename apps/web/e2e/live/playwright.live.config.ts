import { existsSync } from 'node:fs';

import { defineConfig, devices } from '@playwright/test';

/**
 * Live WB-002 run against a real host + gateway (see README.md). Separate
 * from `e2e/playwright.config.ts`: it needs native binaries and drives this
 * machine's desktop, so it is never part of the default e2e lane.
 *
 * Uses installed Google Chrome when present (H.264 in WebCodecs and
 * WebTransport with `serverCertificateHashes`), else Playwright's Chromium.
 */
const chrome = 'C:/Program Files/Google/Chrome/Application/chrome.exe';

export default defineConfig({
  testDir: '.',
  testMatch: '*.live.ts',
  outputDir: '../../test-results/live',
  reporter: [['list']],
  workers: 1,
  use: {
    ...devices['Desktop Chrome'],
    viewport: { width: 1280, height: 800 },
    trace: 'retain-on-failure',
    ...(existsSync(chrome) ? { channel: 'chrome' } : {}),
  },
});
