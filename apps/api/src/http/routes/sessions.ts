import { createRoute, z } from '@hono/zod-openapi';
import { desc, eq } from 'drizzle-orm';
import { actorRef, authorize } from '../../auth/actor.ts';
import type { AppDeps } from '../../context.ts';
import { canonicalJson, utf8, verifyEd25519 } from '../../crypto.ts';
import { device, sessionLog } from '../../db/schema/index.ts';
import { ApiError, conflict } from '../../errors.ts';
import { newId } from '../../ids.ts';
import { recordAudit } from '../../services/audit.ts';
import {
  AppendSessionLogRequest,
  body,
  commonErrors,
  json,
  Limit,
  SessionLogListSchema,
  SessionLogSchema,
  type SessionLogPayload,
} from '../schemas.ts';
import { iso, isoOrNull, security, type App } from '../util.ts';
import { findDevice } from './devices.ts';

const tag = ['sessions'];

/** The exact text the host signs for one session log record. */
export function sessionLogMessage(
  orgId: string,
  deviceId: string,
  payload: z.infer<typeof SessionLogPayload>,
): string {
  return `scrin-session-log/v1\n${canonicalJson({ orgId, deviceId, ...payload })}`;
}

type Row = typeof sessionLog.$inferSelect;

const toDto = (r: Row, devicePublicId: string) => ({
  id: r.publicId,
  deviceId: devicePublicId,
  sessionKey: r.sessionKey,
  controllerPub: r.controllerPub,
  controllerScrinId: r.controllerScrinId,
  anonymous: r.anonymous,
  permissions: r.permissions,
  startedAt: iso(r.startedAt),
  endedAt: isoOrNull(r.endedAt),
  endReason: r.endReason,
  signature: r.signature,
  createdAt: iso(r.createdAt),
});

export function registerSessionRoutes(app: App, deps: AppDeps): void {
  const now = () => deps.now?.() ?? new Date();

  app.openapi(
    createRoute({
      method: 'get',
      path: '/v1/sessions',
      operationId: 'listSessions',
      summary: 'List session logs (newest first)',
      tags: tag,
      security,
      request: { query: z.object({ device: z.string().optional(), limit: Limit }) },
      responses: { 200: json(SessionLogListSchema, 'Session logs'), ...commonErrors },
    }),
    async (c) => {
      const scope = authorize(c.var.actor, deps, 'sessions:read');
      const q = c.req.valid('query');
      const dev = q.device === undefined ? undefined : await findDevice(scope, q.device);
      const rows = await deps.db
        .select({ s: sessionLog, d: device.publicId })
        .from(sessionLog)
        .innerJoin(device, eq(device.id, sessionLog.deviceId))
        .where(
          scope.where(sessionLog, dev === undefined ? undefined : eq(sessionLog.deviceId, dev.id)),
        )
        .orderBy(desc(sessionLog.startedAt), desc(sessionLog.id))
        .limit(q.limit);
      return c.json({ items: rows.map((r) => toDto(r.s, r.d)) }, 200);
    },
  );

  app.openapi(
    createRoute({
      method: 'post',
      path: '/v1/sessions',
      operationId: 'appendSessionLog',
      summary: 'Append a session log record signed by the host device key',
      tags: tag,
      security,
      request: { body: body(AppendSessionLogRequest) },
      responses: { 201: json(SessionLogSchema, 'Stored'), ...commonErrors },
    }),
    async (c) => {
      const actor = c.var.actor;
      const scope = authorize(actor, deps, 'devices:write');
      const input = c.req.valid('json');
      const dev = await findDevice(scope, input.deviceId);
      const message = sessionLogMessage(scope.orgId, dev.publicId, input.payload);
      if (!(await verifyEd25519(dev.devicePub, utf8(message), input.signature))) {
        throw new ApiError(
          400,
          'invalid_signature',
          'Session log is not signed by this device key',
        );
      }
      const p = input.payload;
      const row = await deps.db.transaction(async (tx) => {
        const [dup] = await tx
          .select({ id: sessionLog.id })
          .from(sessionLog)
          .where(
            scope.where(
              sessionLog,
              eq(sessionLog.deviceId, dev.id),
              eq(sessionLog.sessionKey, p.sessionKey),
            ),
          )
          .limit(1);
        if (dup !== undefined) throw conflict('Session already logged', 'session_exists');
        const publicId = newId('ses');
        const [r] = await tx
          .insert(sessionLog)
          .values({
            publicId,
            orgId: scope.orgId,
            deviceId: dev.id,
            sessionKey: p.sessionKey,
            controllerPub: p.controllerPub,
            controllerScrinId: p.controllerScrinId,
            anonymous: p.anonymous,
            permissions: p.permissions,
            startedAt: new Date(p.startedAt),
            endedAt: p.endedAt === null ? null : new Date(p.endedAt),
            endReason: p.endReason,
            payload: p,
            signature: input.signature,
          })
          .returning();
        await tx
          .update(device)
          .set({ lastSeenAt: now() })
          .where(scope.where(device, eq(device.id, dev.id)));
        await recordAudit(
          tx,
          scope.orgId,
          {
            ...actorRef(actor),
            action: 'session.logged',
            targetType: 'session',
            targetId: publicId,
            data: { deviceId: dev.publicId, anonymous: p.anonymous, permissions: p.permissions },
          },
          now(),
        );
        return r;
      });
      if (row === undefined) throw new Error('insert returned no row');
      return c.json(toDto(row, dev.publicId), 201);
    },
  );
}
