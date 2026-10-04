import { serve } from '@hono/node-server';
import { bootstrap } from './bootstrap.ts';
import { loadEnv } from './env.ts';
import { startWebhookWorker } from './services/webhooks.ts';

const env = loadEnv(process.env);
const { app, deps, handle } = await bootstrap(env);
const stopWorker = env.WEBHOOK_WORKER ? startWebhookWorker(deps.db, deps.logger) : () => undefined;

const server = serve({ fetch: app.fetch, port: env.PORT }, (info) => {
  deps.logger.info(
    { port: info.port, db: handle.kind, email: deps.mailer.enabled ? 'brivio' : 'disabled' },
    'scrin-api listening',
  );
});

function shutdown(signal: string): void {
  deps.logger.info({ signal }, 'shutting down');
  stopWorker();
  server.close(() => {
    handle
      .close()
      .catch((err: unknown) => {
        deps.logger.error({ err }, 'db close failed');
      })
      .finally(() => process.exit(0));
  });
  setTimeout(() => process.exit(1), 10_000).unref();
}
process.on('SIGTERM', () => {
  shutdown('SIGTERM');
});
process.on('SIGINT', () => {
  shutdown('SIGINT');
});
