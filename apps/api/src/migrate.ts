// Migration runner: `node dist/migrate.mjs` (image) or `pnpm --filter @scrin/api db:migrate`.
// Run BEFORE deploying an image that needs the new schema.
import { openDb } from './db/client.ts';
import { createLogger } from './logger.ts';

const log = createLogger(process.env.LOG_LEVEL ?? 'info');
const url = process.env.DATABASE_URL;
if (url === undefined || url === '') {
  log.fatal('DATABASE_URL is not set');
  process.exit(1);
}

const handle = await openDb(url);
try {
  await handle.migrate();
  log.info({ kind: handle.kind }, 'migrations applied');
} catch (err) {
  log.fatal({ err }, 'migration failed');
  process.exitCode = 1;
} finally {
  await handle.close();
}
