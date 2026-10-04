import { createApp } from './app.ts';
import { createAuth } from './auth/auth.ts';
import type { AppDeps } from './context.ts';
import { openDb, type DbHandle } from './db/client.ts';
import type { Env } from './env.ts';
import { createLogger, type Logger } from './logger.ts';
import { createMailer, type Mailer } from './mailer.ts';

export interface Runtime {
  app: ReturnType<typeof createApp>;
  deps: AppDeps;
  handle: DbHandle;
}

/** Wires env → db → auth → app. Shared by the server, the OpenAPI export and tests. */
export async function bootstrap(
  env: Env,
  overrides: { logger?: Logger; mailer?: Mailer; handle?: DbHandle; migrate?: boolean } = {},
): Promise<Runtime> {
  const logger = overrides.logger ?? createLogger(env.LOG_LEVEL);
  const handle = overrides.handle ?? (await openDb(env.DATABASE_URL));
  if (overrides.migrate === true || handle.kind === 'pglite') await handle.migrate();
  const mailer =
    overrides.mailer ??
    createMailer({
      apiUrl: env.BRIVIO_API_URL,
      apiKey: env.BRIVIO_API_KEY,
      from: env.MAIL_FROM,
      logger,
    });
  const publicUrl = new URL(env.PUBLIC_URL);
  const auth = createAuth({
    db: handle.db,
    secret: env.BETTER_AUTH_SECRET,
    baseURL: publicUrl.origin,
    appURL: env.TRUSTED_ORIGINS[0] ?? publicUrl.origin,
    trustedOrigins: env.TRUSTED_ORIGINS,
    passkeyRpId: env.PASSKEY_RP_ID ?? publicUrl.hostname,
    requireEmailVerification: env.REQUIRE_EMAIL_VERIFICATION,
    mailer,
    logger,
  });
  const deps: AppDeps = {
    db: handle.db,
    auth,
    logger,
    mailer,
    ping: () => handle.ping(),
    webhookAllowInsecure: env.WEBHOOK_ALLOW_INSECURE,
    publicUrl: publicUrl.origin,
  };
  return { app: createApp(deps), deps, handle };
}
