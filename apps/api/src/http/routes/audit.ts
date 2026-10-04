import { createRoute, z } from '@hono/zod-openapi';
import { asc, gt } from 'drizzle-orm';
import { authorize } from '../../auth/actor.ts';
import type { AppDeps } from '../../context.ts';
import { auditEvent } from '../../db/schema/index.ts';
import { verifyChain } from '../../services/audit.ts';
import { AuditListSchema, AuditVerifySchema, commonErrors, json, Limit } from '../schemas.ts';
import { iso, security, type App } from '../util.ts';

const tag = ['audit'];

export function registerAuditRoutes(app: App, deps: AppDeps): void {
  app.openapi(
    createRoute({
      method: 'get',
      path: '/v1/audit',
      operationId: 'listAuditEvents',
      summary: 'Read the audit chain in order (cursor = last seen seq)',
      tags: tag,
      security,
      request: {
        query: z.object({ after: z.coerce.number().int().min(0).default(0), limit: Limit }),
      },
      responses: { 200: json(AuditListSchema, 'Audit events'), ...commonErrors },
    }),
    async (c) => {
      const scope = authorize(c.var.actor, deps, 'audit:read');
      const q = c.req.valid('query');
      const rows = await deps.db
        .select()
        .from(auditEvent)
        .where(scope.where(auditEvent, gt(auditEvent.seq, q.after)))
        .orderBy(asc(auditEvent.seq))
        .limit(q.limit);
      return c.json(
        {
          items: rows.map((r) => ({
            id: r.publicId,
            seq: r.seq,
            actorType: r.actorType,
            actorId: r.actorId,
            action: r.action,
            targetType: r.targetType,
            targetId: r.targetId,
            data: r.data,
            createdAt: iso(r.createdAt),
            prevHash: r.prevHash,
            hash: r.hash,
          })),
        },
        200,
      );
    },
  );

  app.openapi(
    createRoute({
      method: 'get',
      path: '/v1/audit/verify',
      operationId: 'verifyAudit',
      summary: 'Recompute the hash chain and report the first broken record',
      tags: tag,
      security,
      responses: { 200: json(AuditVerifySchema, 'Verification result'), ...commonErrors },
    }),
    async (c) => {
      const scope = authorize(c.var.actor, deps, 'audit:read');
      const r = await verifyChain(deps.db, scope.orgId);
      return c.json(
        { valid: r.valid, count: r.count, headHash: r.headHash, brokenAt: r.brokenAt ?? null },
        200,
      );
    },
  );
}
