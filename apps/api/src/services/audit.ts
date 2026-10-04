import { and, arrayOverlaps, asc, eq, gt, sql } from 'drizzle-orm';
import { canonicalJson, sha256Hex } from '../crypto.ts';
import type { Db } from '../db/client.ts';
import { auditEvent, webhook, webhookDelivery } from '../db/schema/index.ts';
import { newId } from '../ids.ts';

export const GENESIS_HASH = '0'.repeat(64);

export interface AuditInput {
  actorType: string;
  actorId: string;
  action: string;
  targetType?: string | undefined;
  targetId?: string | undefined;
  data?: Record<string, unknown> | undefined;
}

interface ChainFields {
  orgId: string;
  seq: number;
  actorType: string;
  actorId: string;
  action: string;
  targetType: string | null;
  targetId: string | null;
  data: unknown;
  createdAt: Date;
}

/** hash = sha256(prev_hash ‖ canonical_json(record)) — both as UTF-8 text. */
export function chainHash(prevHash: string, f: ChainFields): string {
  const record = canonicalJson({
    action: f.action,
    actorId: f.actorId,
    actorType: f.actorType,
    createdAt: f.createdAt.toISOString(),
    data: f.data,
    orgId: f.orgId,
    seq: f.seq,
    targetId: f.targetId,
    targetType: f.targetType,
  });
  return sha256Hex(prevHash + record);
}

/**
 * Appends one audit event to the organisation's chain and enqueues webhook
 * deliveries for it. Must run inside the transaction of the mutation it
 * records; a per-organisation advisory lock serialises appends.
 */
export async function recordAudit(
  tx: Db,
  orgId: string,
  input: AuditInput,
  now: Date = new Date(),
): Promise<{ publicId: string; seq: number; hash: string }> {
  await tx.execute(sql`select pg_advisory_xact_lock(hashtext(${`audit:${orgId}`}))`);
  const [last] = await tx
    .select({ seq: auditEvent.seq, hash: auditEvent.hash })
    .from(auditEvent)
    .where(eq(auditEvent.orgId, orgId))
    .orderBy(sql`${auditEvent.seq} desc`)
    .limit(1);
  const seq = (last?.seq ?? 0) + 1;
  const prevHash = last?.hash ?? GENESIS_HASH;
  // Millisecond precision so the stored timestamptz round-trips exactly.
  const createdAt = new Date(now.getTime());
  const fields: ChainFields = {
    orgId,
    seq,
    actorType: input.actorType,
    actorId: input.actorId,
    action: input.action,
    targetType: input.targetType ?? null,
    targetId: input.targetId ?? null,
    data: input.data ?? {},
    createdAt,
  };
  const hash = chainHash(prevHash, fields);
  const publicId = newId('aud');
  await tx.insert(auditEvent).values({ publicId, ...fields, prevHash, hash });
  await enqueueWebhooks(tx, orgId, input.action, {
    id: publicId,
    event: input.action,
    seq,
    createdAt: createdAt.toISOString(),
    actor: { type: input.actorType, id: input.actorId },
    target:
      input.targetId === undefined ? null : { type: input.targetType ?? null, id: input.targetId },
    data: fields.data,
  });
  return { publicId, seq, hash };
}

async function enqueueWebhooks(tx: Db, orgId: string, event: string, payload: unknown) {
  const hooks = await tx
    .select({ id: webhook.id })
    .from(webhook)
    .where(
      and(
        eq(webhook.orgId, orgId),
        eq(webhook.active, true),
        arrayOverlaps(webhook.events, [event, '*']),
      ),
    );
  if (hooks.length === 0) return;
  await tx.insert(webhookDelivery).values(
    hooks.map((h) => ({
      publicId: newId('whd'),
      orgId,
      webhookId: h.id,
      event,
      payload,
    })),
  );
}

export interface ChainVerification {
  valid: boolean;
  count: number;
  headHash: string;
  /** First broken record, if any. */
  brokenAt?: {
    seq: number;
    id: string;
    reason: 'prev_hash_mismatch' | 'hash_mismatch' | 'seq_gap';
  };
}

/** Re-computes the whole chain of one organisation, in pages. */
export async function verifyChain(
  db: Db,
  orgId: string,
  pageSize = 1000,
): Promise<ChainVerification> {
  let prev = GENESIS_HASH;
  let expectedSeq = 1;
  let count = 0;
  let afterSeq = 0;
  for (;;) {
    const rows = await db
      .select()
      .from(auditEvent)
      .where(and(eq(auditEvent.orgId, orgId), gt(auditEvent.seq, afterSeq)))
      .orderBy(asc(auditEvent.seq))
      .limit(pageSize);
    for (const r of rows) {
      const broken = (reason: 'prev_hash_mismatch' | 'hash_mismatch' | 'seq_gap') => ({
        valid: false,
        count,
        headHash: prev,
        brokenAt: { seq: r.seq, id: r.publicId, reason },
      });
      if (r.seq !== expectedSeq) return broken('seq_gap');
      if (r.prevHash !== prev) return broken('prev_hash_mismatch');
      if (chainHash(prev, r) !== r.hash) return broken('hash_mismatch');
      prev = r.hash;
      expectedSeq += 1;
      count += 1;
      afterSeq = r.seq;
    }
    if (rows.length < pageSize) return { valid: true, count, headHash: prev };
  }
}
