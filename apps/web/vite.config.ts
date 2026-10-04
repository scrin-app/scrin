import tailwindcss from '@tailwindcss/vite';
import { tanstackRouter } from '@tanstack/router-plugin/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

import pkg from './package.json' with { type: 'json' };

export default defineConfig({
  plugins: [
    // Must run before the React plugin so generated route code is transformed.
    tanstackRouter({ target: 'react', autoCodeSplitting: true }),
    react(),
    tailwindcss(),
  ],
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },
  server: { port: 5180, strictPort: true },
  preview: { port: 5181, strictPort: true },
  build: {
    target: 'es2024',
    sourcemap: true,
    reportCompressedSize: false,
  },
});
