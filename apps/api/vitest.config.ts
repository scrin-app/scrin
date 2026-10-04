import { defineProject } from 'vitest/config';

export default defineProject({
  test: {
    name: 'api',
    environment: 'node',
    include: ['src/**/*.test.ts'],
    // Each test file boots an in-process Postgres (PGlite) and applies the migrations.
    testTimeout: 30_000,
    hookTimeout: 60_000,
  },
});
