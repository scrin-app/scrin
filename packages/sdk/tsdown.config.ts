import { defineConfig } from 'tsdown';

export default defineConfig({
  entry: ['src/index.ts'],
  platform: 'neutral',
  target: 'es2024',
  format: 'esm',
  dts: true,
  clean: true,
});
