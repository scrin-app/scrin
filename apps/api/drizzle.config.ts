import { defineConfig } from 'drizzle-kit';

// `drizzle-kit generate` needs no database; migrations are committed in ./drizzle.
export default defineConfig({
  dialect: 'postgresql',
  schema: './src/db/schema/index.ts',
  out: './drizzle',
  strict: true,
  verbose: true,
});
