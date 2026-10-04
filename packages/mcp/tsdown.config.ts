import { defineConfig } from 'tsdown';

export default defineConfig({
  entry: ['src/bin.ts'],
  platform: 'node',
  target: 'node24',
  format: 'esm',
  dts: false,
  clean: true,
  deps: { alwaysBundle: [/^@scrin\/sdk$/], onlyBundle: ['openapi-fetch'] },
});
