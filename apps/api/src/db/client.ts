import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { sql } from 'drizzle-orm';
import type { PgDatabase, PgQueryResultHKT } from 'drizzle-orm/pg-core';
import * as schema from './schema/index.ts';

type Schema = typeof schema;
/** Driver-agnostic handle: postgres-js in production, PGlite in tests and local dev. */
export type Db = PgDatabase<PgQueryResultHKT, Schema>;

export interface DbHandle {
  db: Db;
  kind: 'postgres' | 'pglite';
  migrate(): Promise<void>;
  ping(): Promise<void>;
  close(): Promise<void>;
}

/**
 * `apps/api/drizzle`: two levels up from `src/db/client.ts`, one level up from
 * the bundled `dist/*.mjs`. `MIGRATIONS_DIR` overrides (the image sets it).
 */
const MIGRATIONS_DIR =
  process.env.MIGRATIONS_DIR ??
  [new URL('../drizzle', import.meta.url), new URL('../../drizzle', import.meta.url)]
    .map((u) => fileURLToPath(u))
    .find((dir) => existsSync(`${dir}/meta/_journal.json`)) ??
  fileURLToPath(new URL('../../drizzle', import.meta.url));

/**
 * `postgres://…` → postgres-js pool. `pglite:memory` / `pglite:<dir>` → in-process
 * Postgres (WASM), loaded lazily so production images never ship it.
 */
export async function openDb(url: string, migrationsDir = MIGRATIONS_DIR): Promise<DbHandle> {
  if (url.startsWith('pglite:')) {
    const target = url.slice('pglite:'.length);
    const { PGlite } = await import('@electric-sql/pglite');
    const { drizzle } = await import('drizzle-orm/pglite');
    const { migrate } = await import('drizzle-orm/pglite/migrator');
    const client = target === 'memory' || target === '' ? new PGlite() : new PGlite(target);
    const db = drizzle({ client, schema });
    return {
      db,
      kind: 'pglite',
      migrate: () => migrate(db, { migrationsFolder: migrationsDir }),
      ping: async () => {
        await db.execute(sql`select 1`);
      },
      close: () => client.close(),
    };
  }
  const { default: postgres } = await import('postgres');
  const { drizzle } = await import('drizzle-orm/postgres-js');
  const { migrate } = await import('drizzle-orm/postgres-js/migrator');
  const client = postgres(url, { max: 10, idle_timeout: 30, connect_timeout: 10 });
  const db = drizzle({ client, schema });
  return {
    db,
    kind: 'postgres',
    migrate: () => migrate(db, { migrationsFolder: migrationsDir }),
    ping: async () => {
      await db.execute(sql`select 1`);
    },
    close: () => client.end({ timeout: 5 }),
  };
}
