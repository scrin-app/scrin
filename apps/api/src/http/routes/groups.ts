import { createRoute } from '@hono/zod-openapi';
import { asc, count, eq } from 'drizzle-orm';
import { actorRef, authorize } from '../../auth/actor.ts';
import type { AppDeps } from '../../context.ts';
import type { Scope } from '../../db/scoped.ts';
import { device, deviceGroup } from '../../db/schema/index.ts';
import { conflict, notFound } from '../../errors.ts';
import { newId } from '../../ids.ts';
import { recordAudit } from '../../services/audit.ts';
import {
  body,
  commonErrors,
  GroupInput,
  GroupListSchema,
  GroupSchema,
  IdParam,
  json,
} from '../schemas.ts';
import { iso, security, type App } from '../util.ts';

const tag = ['groups'];

async function listGroups(scope: Scope) {
  const rows = await scope.db
    .select({ g: deviceGroup, n: count(device.id) })
    .from(deviceGroup)
    .leftJoin(device, eq(device.groupId, deviceGroup.id))
    .where(scope.where(deviceGroup))
    .groupBy(deviceGroup.id)
    .orderBy(asc(deviceGroup.name));
  return rows.map((r) => ({
    id: r.g.publicId,
    name: r.g.name,
    description: r.g.description,
    deviceCount: r.n,
    createdAt: iso(r.g.createdAt),
  }));
}

async function nameTaken(scope: Scope, name: string): Promise<boolean> {
  const [g] = await scope.db
    .select({ id: deviceGroup.id })
    .from(deviceGroup)
    .where(scope.where(deviceGroup, eq(deviceGroup.name, name)))
    .limit(1);
  return g !== undefined;
}

export function registerGroupRoutes(app: App, deps: AppDeps): void {
  const now = () => deps.now?.() ?? new Date();

  app.openapi(
    createRoute({
      method: 'get',
      path: '/v1/groups',
      operationId: 'listGroups',
      summary: 'List device groups',
      tags: tag,
      security,
      responses: { 200: json(GroupListSchema, 'Groups'), ...commonErrors },
    }),
    async (c) => {
      const scope = authorize(c.var.actor, deps, 'groups:read');
      return c.json({ items: await listGroups(scope) }, 200);
    },
  );

  app.openapi(
    createRoute({
      method: 'post',
      path: '/v1/groups',
      operationId: 'createGroup',
      summary: 'Create a device group',
      tags: tag,
      security,
      request: { body: body(GroupInput) },
      responses: { 201: json(GroupSchema, 'Created group'), ...commonErrors },
    }),
    async (c) => {
      const actor = c.var.actor;
      const scope = authorize(actor, deps, 'groups:write');
      const input = c.req.valid('json');
      if (await nameTaken(scope, input.name))
        throw conflict('A group with this name exists', 'group_exists');
      const publicId = newId('grp');
      const row = await deps.db.transaction(async (tx) => {
        const [g] = await tx
          .insert(deviceGroup)
          .values({
            publicId,
            orgId: scope.orgId,
            name: input.name,
            description: input.description ?? null,
          })
          .returning();
        await recordAudit(
          tx,
          scope.orgId,
          {
            ...actorRef(actor),
            action: 'group.created',
            targetType: 'group',
            targetId: publicId,
            data: { name: input.name },
          },
          now(),
        );
        return g;
      });
      if (row === undefined) throw new Error('insert returned no row');
      return c.json(
        {
          id: publicId,
          name: row.name,
          description: row.description,
          deviceCount: 0,
          createdAt: iso(row.createdAt),
        },
        201,
      );
    },
  );

  app.openapi(
    createRoute({
      method: 'delete',
      path: '/v1/groups/{id}',
      operationId: 'deleteGroup',
      summary: 'Delete a device group (its devices become ungrouped)',
      tags: tag,
      security,
      request: { params: IdParam },
      responses: { 204: { description: 'Deleted' }, ...commonErrors },
    }),
    async (c) => {
      const actor = c.var.actor;
      const scope = authorize(actor, deps, 'groups:write');
      const id = c.req.valid('param').id;
      await deps.db.transaction(async (tx) => {
        const deleted = await tx
          .delete(deviceGroup)
          .where(scope.where(deviceGroup, eq(deviceGroup.publicId, id)))
          .returning({ id: deviceGroup.id });
        if (deleted.length === 0) throw notFound('Device group');
        await recordAudit(
          tx,
          scope.orgId,
          { ...actorRef(actor), action: 'group.deleted', targetType: 'group', targetId: id },
          now(),
        );
      });
      return c.body(null, 204);
    },
  );
}
