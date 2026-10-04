import { defineConfig } from 'tsdown';

export default defineConfig({
  entry: ['src/server.ts', 'src/migrate.ts'],
  platform: 'node',
  target: 'node24',
  format: 'esm',
  // An app, not a library: no declarations.
  dts: false,
  clean: true,
  sourcemap: true,
  deps: {
    // Dev-only in-process Postgres (DATABASE_URL=pglite:...). Never bundle its wasm.
    neverBundle: [/^@electric-sql\/pglite(\/|$)/, /^drizzle-orm\/pglite(\/|$)/],
  },
});
