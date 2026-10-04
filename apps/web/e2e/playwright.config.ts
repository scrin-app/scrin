import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: '.',
  testMatch: '*.spec.ts',
  outputDir: '../test-results',
  reporter: [['list']],
  fullyParallel: true,
  use: {
    baseURL: 'http://127.0.0.1:5181',
    trace: 'retain-on-failure',
  },
  projects: [
    { name: 'phone', use: { ...devices['Desktop Chrome'], viewport: { width: 390, height: 844 } } },
    {
      name: 'ultrawide',
      use: { ...devices['Desktop Chrome'], viewport: { width: 3440, height: 1440 } },
    },
  ],
  webServer: {
    command: 'pnpm run preview --host 127.0.0.1',
    cwd: '..',
    url: 'http://127.0.0.1:5181',
    reuseExistingServer: true,
    timeout: 60_000,
  },
});
