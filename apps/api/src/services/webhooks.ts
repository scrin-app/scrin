import { and, asc, eq, lte } from 'drizzle-orm';
import { hmacSha256Hex } from '../crypto.ts';
import type { Db } from '../db/client.ts';
import { webhook, webhookDelivery } from '../db/schema/index.ts';
import type { Logger } from '../logger.ts';

const SIGNATURE_HEADER = 'scrin-signature';
const EVENT_HEADER = 'scrin-event';
const DELIVERY_HEADER = 'scrin-delivery';
export const MAX_ATTEMPTS = 6;

/** Backoff after attempt n (1-based): 30 s, 2 min, 8 min, 32 min, ~2 h. */
export function backoffMs(attempt: number): number {
  return Math.min(30_000 * 4 ** (attempt - 1), 6 * 60 * 60 * 1000);
}

/**
 * `t=<unix seconds>,v1=<hex HMAC-SHA256(secret, "<t>.<body>")>` — the receiver
 * recomputes it over the raw body and rejects stale timestamps (replay guard).
 */
export function signPayload(secret: string, body: string, timestamp: number): string {
  return `t=${timestamp},v1=${hmacSha256Hex(secret, `${timestamp}.${body}`)}`;
}

export interface DeliveryOptions {
  fetch?: typeof fetch;
  now?: () => Date;
  timeoutMs?: number;
  logger?: Logger;
  limit?: number;
}

export interface DeliveryReport {
  attempted: number;
  succeeded: number;
  retried: number;
  failed: number;
}

/** Delivers due webhook rows once. Safe to run concurrently: a row is claimed by bumping `attempts`. */
export async function deliverDueWebhooks(
  db: Db,
  opts: DeliveryOptions = {},
): Promise<DeliveryReport> {
  const now = opts.now ?? (() => new Date());
  const doFetch = opts.fetch ?? fetch;
  const report: DeliveryReport = { attempted: 0, succeeded: 0, retried: 0, failed: 0 };
  const due = await db
    .select({ d: webhookDelivery, w: webhook })
    .from(webhookDelivery)
    .innerJoin(webhook, eq(webhook.id, webhookDelivery.webhookId))
    .where(and(eq(webhookDelivery.status, 'pending'), lte(webhookDelivery.nextAttemptAt, now())))
    .orderBy(asc(webhookDelivery.nextAttemptAt))
    .limit(opts.limit ?? 50);

  for (const { d, w } of due) {
    const attempt = d.attempts + 1;
    // Optimistic claim: only one worker moves attempts from n to n+1.
    const claimed = await db
      .update(webhookDelivery)
      .set({ attempts: attempt })
      .where(and(eq(webhookDelivery.id, d.id), eq(webhookDelivery.attempts, d.attempts)))
      .returning({ id: webhookDelivery.id });
    if (claimed.length === 0) continue;
    report.attempted += 1;

    const body = JSON.stringify(d.payload);
    const ts = Math.floor(now().getTime() / 1000);
    let status: number | null = null;
    let error: string | null = null;
    try {
      const res = await doFetch(w.url, {
        method: 'POST',
        headers: {
          'content-type': 'application/json',
          'user-agent': 'scrin-webhooks/1',
          [SIGNATURE_HEADER]: signPayload(w.secret, body, ts),
          [EVENT_HEADER]: d.event,
          [DELIVERY_HEADER]: d.publicId,
        },
        body,
        redirect: 'manual',
        signal: AbortSignal.timeout(opts.timeoutMs ?? 10_000),
      });
      status = res.status;
      await res.body?.cancel();
      if (!res.ok) error = `HTTP ${res.status}`;
    } catch (err) {
      error = err instanceof Error ? err.message : 'request failed';
    }

    if (error === null) {
      report.succeeded += 1;
      await db
        .update(webhookDelivery)
        .set({ status: 'succeeded', lastStatusCode: status, lastError: null, deliveredAt: now() })
        .where(eq(webhookDelivery.id, d.id));
    } else if (attempt >= MAX_ATTEMPTS) {
      report.failed += 1;
      await db
        .update(webhookDelivery)
        .set({ status: 'failed', lastStatusCode: status, lastError: error })
        .where(eq(webhookDelivery.id, d.id));
      opts.logger?.warn({ delivery: d.publicId, attempt }, 'webhook delivery gave up');
    } else {
      report.retried += 1;
      await db
        .update(webhookDelivery)
        .set({
          lastStatusCode: status,
          lastError: error,
          nextAttemptAt: new Date(now().getTime() + backoffMs(attempt)),
        })
        .where(eq(webhookDelivery.id, d.id));
    }
  }
  return report;
}

/** Background loop for the single-process deployment. Returns a stop function. */
export function startWebhookWorker(db: Db, logger: Logger, intervalMs = 5_000): () => void {
  let running = false;
  const timer = setInterval(() => {
    if (running) return;
    running = true;
    deliverDueWebhooks(db, { logger })
      .then((r) => {
        if (r.attempted > 0) logger.info(r, 'webhook deliveries');
      })
      .catch((err: unknown) => {
        logger.error({ err }, 'webhook worker tick failed');
      })
      .finally(() => {
        running = false;
      });
  }, intervalMs);
  timer.unref();
  return () => {
    clearInterval(timer);
  };
}
