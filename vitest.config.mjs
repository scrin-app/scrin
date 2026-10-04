import { defineConfig } from 'vitest/config';

// One `vitest run` at the root runs every package as its own project.
export default defineConfig({
  test: {
    projects: ['packages/*', 'apps/web', 'apps/api'],
  },
});
