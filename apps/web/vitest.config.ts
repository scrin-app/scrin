import { defineProject } from 'vitest/config';

export default defineProject({
  define: { __APP_VERSION__: JSON.stringify('0.0.0-test') },
  test: {
    name: 'web',
    environment: 'happy-dom',
    include: ['src/**/*.test.{ts,tsx}'],
    setupFiles: ['./src/test/setup.ts'],
  },
});
