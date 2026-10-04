import { createRoute, z } from '@hono/zod-openapi';
import { and, desc, eq, lt } from 'drizzle-orm';
import { actorRef, authorize } from '../../auth/actor.ts';
import type { Actor, AppDeps } from '../../context.ts';
import type { Db } from '../../db/client.ts';
import type { Scope } from '../../db/scoped.ts';
import { device, jitGrant } from '../../db/schema/index.ts';
import { ApiError, conflict, forbidden, notFound } from '../../errors.ts';
import { newId } from '../../ids.ts';
import { recordAudit } from '../../services/audit.ts';
import {
  body,
  commonErrors,
  IdParam,
  JIT_STATUSES,
  JitDecisionInput,
  JitGrantSchema,
  JitListQuery,
  JitListSchema,
  JitRequestInput,
  json,
} from '../schemas.ts';
import { iso, isoOrNull, security, type App } from '../util.ts';
import { findDevice } from './devices.ts';

const tag = ['jit'];
const Status = z.enum(JIT_STATUSES);
const MAX_LEAD_MS = 7 * 24 * 60 * 60 * 1000;

type Row = typeof jitGrant.$inferSelect;

const toDto = (r: Row, devicePublicId: string) => ({
  id: r.publicId,
  deviceId: devicePublicId,
  requesterUserId: r.requesterUserId,
  reason: r.reason,
  windowStart: iso(r.windowStart),
  windowEnd: iso(r.windowEnd),
  status: Status.parse(r.status),
  approverUserId: r.approverUserId,
  decisionNote: r.decisionNote,
  decidedAt: isoOrNull(r.decidedAt),
  createdAt: iso(r.createdAt),
});

/** Pending requests whose window already closed can never be approved. */
async function expireStale(db: Db, scope: Scope, now: Date): Promise<void> {
  await db
    .update(jitGrant)
    .set({ status: 'expired' })
    .where(scope.where(jitGrant, eq(jitGrant.status, 'pending'), lt(jitGrant.windowEnd, now)));
}

async function find(scope: Scope, id: string) {
  const [r] = await scope.db
    .select({ g: jitGrant, d: device.publicId })
    .from(jitGrant)
    .innerJoin(device, eq(device.id, jitGrant.deviceId))
    .where(scope.where(jitGrant, eq(jitGrant.publicId, id)))
    .limit(1);
  if (r === undefined) throw notFound('JIT grant');
  return r;
}

export function registerJitRoutes(app: App, deps: AppDeps): void {
  const now = () => deps.now?.() ?? new Date();

  app.openapi(
    createRoute({
      method: 'get',
      path: '/v1/jit',
      operationId: 'listJitGrants',
      summary: 'List just-in-time access grants',
      tags: tag,
      security,
      request: { query: JitListQuery },
      responses: { 200: json(JitListSchema, 'Grants'), ...commonErrors },
    }),
    async (c) => {
      const scope = authorize(c.var.actor, deps, 'jit:read');
      await expireStale(deps.db, scope, now());
      const q = c.req.valid('query');
      const dev = q.device === undefined ? undefined : await findDevice(scope, q.device);
      const rows = await deps.db
        .select({ g: jitGrant, d: device.publicId })
        .from(jitGrant)
        .innerJoin(device, eq(device.id, jitGrant.deviceId))
        .where(
          scope.where(
            jitGrant,
            q.status === undefined ? undefined : eq(jitGrant.status, q.status),
            dev === undefined ? undefined : eq(jitGrant.deviceId, dev.id),
          ),
        )
        .orderBy(desc(jitGrant.createdAt), desc(jitGrant.id))
        .limit(q.limit);
      return c.json({ items: rows.map((r) => toDto(r.g, r.d)) }, 200);
    },
  );

  app.openapi(
    createRoute({
      method: 'post',
      path: '/v1/jit',
      operationId: 'requestJitAccess',
      summary: 'Request time-boxed access to a device',
      tags: tag,
      security,
      request: { body: body(JitRequestInput) },
      responses: { 201: json(JitGrantSchema, 'Requested'), ...commonErrors },
    }),
    async (c) => {
      const actor = c.var.actor;
      const scope = authorize(actor, deps, 'jit:request');
      const input = c.req.valid('json');
      const t = now();
      const dev = await findDevice(scope, input.deviceId);
      const start = input.windowStart === undefined ? t : new Date(input.windowStart);
      if (start.getTime() < t.getTime() - 60_000 || start.getTime() > t.getTime() + MAX_LEAD_MS) {
        throw new ApiError(
          400,
          'invalid_window',
          'windowStart must be between now and 7 days ahead',
        );
      }
      const end = new Date(start.getTime() + input.durationMinutes * 60_000);
      const publicId = newId('jit');
      const row = await deps.db.transaction(async (tx) => {
        const [r] = await tx
          .insert(jitGrant)
          .values({
            publicId,
            orgId: scope.orgId,
            requesterUserId: actor.userId,
            deviceId: dev.id,
            reason: input.reason,
            windowStart: start,
            windowEnd: end,
          })
          .returning();
        await recordAudit(
          tx,
          scope.orgId,
          {
            ...actorRef(actor),
            action: 'jit.requested',
            targetType: 'jit_grant',
            targetId: publicId,
            data: { deviceId: dev.publicId, windowStart: iso(start), windowEnd: iso(end) },
          },
          t,
        );
        return r;
      });
      if (row === undefined) throw new Error('insert returned no row');
      return c.json(toDto(row, dev.publicId), 201);
    },
  );

  const decide = (decision: 'approved' | 'denied') =>
    async function handle(actor: Actor, id: string, note: string | undefined) {
      const scope = authorize(actor, deps, 'jit:approve');
      const t = now();
      await expireStale(deps.db, scope, t);
      const { g, d } = await find(scope, id);
      if (g.requesterUserId === actor.userId) {
        throw forbidden('You cannot decide on your own access request', 'self_approval');
      }
      if (g.status !== 'pending')
        throw conflict(`Grant is already ${g.status}`, 'grant_not_pending');
      const row = await deps.db.transaction(async (tx) => {
        const [r] = await tx
          .update(jitGrant)
          .set({
            status: decision,
            approverUserId: actor.userId,
            decisionNote: note ?? null,
            decidedAt: t,
          })
          .where(scope.where(jitGrant, and(eq(jitGrant.id, g.id), eq(jitGrant.status, 'pending'))))
          .returning();
        if (r === undefined) throw conflict('Grant was decided concurrently', 'grant_not_pending');
        await recordAudit(
          tx,
          scope.orgId,
          {
            ...actorRef(actor),
            action: decision === 'approved' ? 'jit.approved' : 'jit.denied',
            targetType: 'jit_grant',
            targetId: g.publicId,
            data: { deviceId: d, requester: g.requesterUserId },
          },
          t,
        );
        return r;
      });
      return toDto(row, d);
    };
  const approve = decide('approved');
  const deny = decide('denied');

  for (const [verb, fn, operationId] of [
    ['approve', approve, 'approveJitAccess'],
    ['deny', deny, 'denyJitAccess'],
  ] as const) {
    app.openapi(
      createRoute({
        method: 'post',
        path: `/v1/jit/{id}/${verb}`,
        operationId,
        summary: `${verb === 'approve' ? 'Approve' : 'Deny'} a pending JIT request (not your own)`,
        tags: tag,
        security,
        request: { params: IdParam, body: body(JitDecisionInput) },
        responses: {
          200: json(JitGrantSchema, 'Decided'),
          ...commonErrors,
          409: json(
            z.object({ error: z.object({ code: z.string(), message: z.string() }) }),
            'Not pending',
          ),
        },
      }),
      async (c) =>
        c.json(await fn(c.var.actor, c.req.valid('param').id, c.req.valid('json').note), 200),
    );
  }
}
