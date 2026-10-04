import type { OpenAPIHono } from '@hono/zod-openapi';
import type { AppEnv } from '../context.ts';

export type App = OpenAPIHono<AppEnv>;

export const iso = (d: Date): string => d.toISOString();
export const isoOrNull = (d: Date | null): string | null => (d === null ? null : d.toISOString());

/** Both auth methods are accepted on every /v1 route. */
export const security = [{ bearerAuth: [] }, { sessionCookie: [] }];
