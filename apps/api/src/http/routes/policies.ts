import { createRoute } from '@hono/zod-openapi';
import { asc, eq } from 'drizzle-orm';
import { actorRef, authorize } from '../../auth/actor.ts';
import type { AppDeps } from '../../context.ts';
import type { Scope } from '../../db/scoped.ts';
import { policy } from '../../db/schema/index.ts';
import { conflict, notFound } from '../../errors.ts';
import { newId } from '../../ids.ts';
import { recordAudit } from '../../services/audit.ts';
import {
  body,
  commonErrors,
  IdParam,
  json,
  PolicyDocument,
  PolicyInput,
  PolicyListSchema,
  PolicySchema,
} from '../schemas.ts';
import { iso, security, type App } from '../util.ts';

const tag = ['policies'];

type PolicyRow = typeof policy.$inferSelect;

const toDto = (p: PolicyRow) => ({
  id: p.publicId,
  name: p.name,
  // Stored documents were validated on write; re-parse so the response contract holds.
  document: PolicyDocument.parse(p.document),
  createdAt: iso(p.createdAt),
  updatedAt: iso(p.updatedAt),
});

async function find(scope: Scope, id: string): Promise<PolicyRow> {
  const [p] = await scope.db
    .select()
    .from(policy)
    .where(scope.where(policy, eq(policy.publicId, id)))
    .limit(1);
  if (p === undefined) throw notFound('Policy');
  return p;
}

export function registerPolicyRoutes(app: App, deps: AppDeps): void {
  const now = () => deps.now?.() ?? new Date();

  app.openapi(
    createRoute({
      method: 'get',
      path: '/v1/policies',
      operationId: 'listPolicies',
      summary: 'List access policies',
      tags: tag,
      security,
      responses: { 200: json(PolicyListSchema, 'Policies'), ...commonErrors },
    }),
    async (c) => {
      const scope = authorize(c.var.actor, deps, 'policies:read');
      const rows = await deps.db
        .select()
        .from(policy)
        .where(scope.where(policy))
        .orderBy(asc(policy.name));
      return c.json({ items: rows.map(toDto) }, 200);
    },
  );

  app.openapi(
    createRoute({
      method: 'get',
      path: '/v1/policies/{id}',
      operationId: 'getPolicy',
      summary: 'Get one policy',
      tags: tag,
      security,
      request: { params: IdParam },
      responses: { 200: json(PolicySchema, 'Policy'), ...commonErrors },
    }),
    async (c) => {
      const scope = authorize(c.var.actor, deps, 'policies:read');
      return c.json(toDto(await find(scope, c.req.valid('param').id)), 200);
    },
  );

  app.openapi(
    createRoute({
      method: 'post',
      path: '/v1/policies',
      operationId: 'createPolicy',
      summary: 'Create an access policy',
      tags: tag,
      security,
      request: { body: body(PolicyInput) },
      responses: { 201: json(PolicySchema, 'Created'), ...commonErrors },
    }),
    async (c) => {
      const actor = c.var.actor;
      const scope = authorize(actor, deps, 'policies:write');
      const input = c.req.valid('json');
      const [dup] = await deps.db
        .select({ id: policy.id })
        .from(policy)
        .where(scope.where(policy, eq(policy.name, input.name)))
        .limit(1);
      if (dup !== undefined) throw conflict('A policy with this name exists', 'policy_exists');
      const publicId = newId('pol');
      const row = await deps.db.transaction(async (tx) => {
        const [p] = await tx
          .insert(policy)
          .values({ publicId, orgId: scope.orgId, name: input.name, document: input.document })
          .returning();
        await recordAudit(
          tx,
          scope.orgId,
          {
            ...actorRef(actor),
            action: 'policy.created',
            targetType: 'policy',
            targetId: publicId,
            data: { document: input.document },
          },
          now(),
        );
        return p;
      });
      if (row === undefined) throw new Error('insert returned no row');
      return c.json(toDto(row), 201);
    },
  );

  app.openapi(
    createRoute({
      method: 'put',
      path: '/v1/policies/{id}',
      operationId: 'updatePolicy',
      summary: 'Replace a policy',
      tags: tag,
      security,
      request: { params: IdParam, body: body(PolicyInput) },
      responses: { 200: json(PolicySchema, 'Updated'), ...commonErrors },
    }),
    async (c) => {
      const actor = c.var.actor;
      const scope = authorize(actor, deps, 'policies:write');
      const input = c.req.valid('json');
      const existing = await find(scope, c.req.valid('param').id);
      const row = await deps.db.transaction(async (tx) => {
        const [p] = await tx
          .update(policy)
          .set({ name: input.name, document: input.document, updatedAt: now() })
          .where(scope.where(policy, eq(policy.id, existing.id)))
          .returning();
        await recordAudit(
          tx,
          scope.orgId,
          {
            ...actorRef(actor),
            action: 'policy.updated',
            targetType: 'policy',
            targetId: existing.publicId,
            data: { document: input.document },
          },
          now(),
        );
        return p;
      });
      if (row === undefined) throw notFound('Policy');
      return c.json(toDto(row), 200);
    },
  );

  app.openapi(
    createRoute({
      method: 'delete',
      path: '/v1/policies/{id}',
      operationId: 'deletePolicy',
      summary: 'Delete a policy',
      tags: tag,
      security,
      request: { params: IdParam },
      responses: { 204: { description: 'Deleted' }, ...commonErrors },
    }),
    async (c) => {
      const actor = c.var.actor;
      const scope = authorize(actor, deps, 'policies:write');
      const existing = await find(scope, c.req.valid('param').id);
      await deps.db.transaction(async (tx) => {
        await tx.delete(policy).where(scope.where(policy, eq(policy.id, existing.id)));
        await recordAudit(
          tx,
          scope.orgId,
          {
            ...actorRef(actor),
            action: 'policy.deleted',
            targetType: 'policy',
            targetId: existing.publicId,
          },
          now(),
        );
      });
      return c.body(null, 204);
    },
  );
}
