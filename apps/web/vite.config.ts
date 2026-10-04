import tailwindcss from '@tailwindcss/vite';
import { tanstackRouter } from '@tanstack/router-plugin/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

import pkg from './package.json' with { type: 'json' };

export default defineConfig(({ mode }) => {
  // The Tauri shell builds this SPA with `--mode desktop` (apps/desktop).
  const desktop = mode === 'desktop' || process.env.VITE_SCRIN_HOST === 'desktop';
  return {
    plugins: [
      // Must run before the React plugin so generated route code is transformed.
      tanstackRouter({ target: 'react', autoCodeSplitting: true }),
      react(),
      tailwindcss(),
    ],
    define: {
      __APP_VERSION__: JSON.stringify(pkg.version),
      'import.meta.env.VITE_SCRIN_HOST': JSON.stringify(desktop ? 'desktop' : 'web'),
    },
    server: { port: desktop ? 5182 : 5180, strictPort: true },
    preview: { port: 5181, strictPort: true },
    build: {
      target: 'es2024',
      outDir: desktop ? 'dist-desktop' : 'dist',
      sourcemap: !desktop,
      reportCompressedSize: false,
    },
  };
});
