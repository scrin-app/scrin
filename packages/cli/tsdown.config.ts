import { defineConfig } from 'tsdown';

export default defineConfig({
  entry: ['src/bin.ts'],
  platform: 'node',
  target: 'node24',
  format: 'esm',
  dts: false,
  clean: true,
  // Self-contained binary: bundle the workspace SDK (openapi-fetch is tiny).
  deps: { alwaysBundle: [/^@scrin\/sdk$/], onlyBundle: ['openapi-fetch'] },
});
