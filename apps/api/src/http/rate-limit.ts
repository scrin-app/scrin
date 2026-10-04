import type { Context, MiddlewareHandler } from 'hono';
import type { AppEnv } from '../context.ts';
import { ApiError } from '../errors.ts';

export function clientIp(c: Context): string {
  const fwd = c.req.header('x-forwarded-for');
  const first = fwd?.split(',')[0]?.trim();
  return first !== undefined && first !== '' ? first : (c.req.header('x-real-ip') ?? 'unknown');
}

/**
 * Fixed-window limiter, in process memory. Enough for one Cloud Run instance
 * and for self-host; a shared store is needed once the API scales out.
 */
export function rateLimit(opts: {
  name: string;
  limit: number;
  windowMs: number;
  key?: (c: Context<AppEnv>) => string;
  now?: () => number;
}): MiddlewareHandler<AppEnv> {
  const hits = new Map<string, { count: number; resetAt: number }>();
  const now = opts.now ?? Date.now;
  return async (c, next) => {
    const t = now();
    const key = `${opts.name}:${opts.key?.(c) ?? clientIp(c)}`;
    let bucket = hits.get(key);
    if (bucket === undefined || bucket.resetAt <= t) {
      if (hits.size > 50_000) {
        for (const [k, v] of hits) if (v.resetAt <= t) hits.delete(k);
      }
      bucket = { count: 0, resetAt: t + opts.windowMs };
      hits.set(key, bucket);
    }
    bucket.count += 1;
    if (bucket.count > opts.limit) {
      c.header('retry-after', String(Math.ceil((bucket.resetAt - t) / 1000)));
      throw new ApiError(429, 'rate_limited', 'Too many requests, slow down');
    }
    await next();
  };
}
