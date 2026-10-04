// Writes the OpenAPI 3.1 document to stdout or to the path in argv[2]. No server,
// no network: an in-memory PGlite backs the app just long enough to build routes.
import { writeFile } from 'node:fs/promises';
import { bootstrap } from '../src/bootstrap.ts';
import { loadEnv } from '../src/env.ts';
import { openApiDocument } from '../src/app.ts';
import { createLogger } from '../src/logger.ts';

const env = loadEnv({
  DATABASE_URL: 'pglite:memory',
  BETTER_AUTH_SECRET: 'openapi-export-only-not-a-real-secret-000000',
  LOG_LEVEL: 'silent',
});
const { app, handle } = await bootstrap(env, { logger: createLogger('silent') });
const doc = `${JSON.stringify(openApiDocument(app), null, 2)}\n`;
await handle.close();
const out = process.argv[2];
if (out === undefined) process.stdout.write(doc);
else await writeFile(out, doc, 'utf8');
