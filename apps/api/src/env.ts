import { createEnv } from '@t3-oss/env-core';
import { z } from 'zod';

const flag = z.enum(['true', 'false', '1', '0']).transform((v) => v === 'true' || v === '1');

const csv = z.string().transform((v) =>
  v
    .split(',')
    .map((s) => s.trim())
    .filter((s) => s.length > 0),
);

/**
 * Validates configuration once at startup; an invalid or missing value aborts
 * the process with the variable name (never its value).
 */
export function loadEnv(source: Record<string, string | undefined>) {
  return createEnv({
    server: {
      NODE_ENV: z.enum(['development', 'test', 'production']).default('development'),
      PORT: z.coerce.number().int().min(1).max(65_535).default(8787),
      LOG_LEVEL: z
        .enum(['fatal', 'error', 'warn', 'info', 'debug', 'trace', 'silent'])
        .default('info'),
      /** `postgres://…` in production; `pglite:memory` or `pglite:<dir>` for local dev. */
      DATABASE_URL: z.string().min(1),
      BETTER_AUTH_SECRET: z.string().min(32),
      /** Public origin of this API, e.g. https://api.scrin.dragoscatalin.ro */
      PUBLIC_URL: z.url().default('http://localhost:8787'),
      TRUSTED_ORIGINS: csv.default([]),
      PASSKEY_RP_ID: z.string().min(1).optional(),
      REQUIRE_EMAIL_VERIFICATION: flag.default(true),
      /** brivio transactional email (ADR-0007). Both unset = email disabled (logged, no-op). */
      BRIVIO_API_URL: z.url().optional(),
      BRIVIO_API_KEY: z.string().min(1).optional(),
      MAIL_FROM: z.string().min(3).default('scrin <no-reply@scrin.dragoscatalin.ro>'),
      /** Allow http:// webhook URLs (local development and tests only). */
      WEBHOOK_ALLOW_INSECURE: flag.default(false),
      WEBHOOK_WORKER: flag.default(true),
    },
    runtimeEnv: source,
    emptyStringAsUndefined: true,
    isServer: true,
  });
}

export type Env = ReturnType<typeof loadEnv>;
