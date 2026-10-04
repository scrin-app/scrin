import { createRoute } from '@hono/zod-openapi';
import { desc, eq, isNull } from 'drizzle-orm';
import { actorRef, API_KEY_PREFIX, authorize } from '../../auth/actor.ts';
import { isPermission, type Permission } from '../../authz.ts';
import type { AppDeps } from '../../context.ts';
import { sha256Hex } from '../../crypto.ts';
import { apiKey } from '../../db/schema/index.ts';
import { forbidden, notFound } from '../../errors.ts';
import { newId, randomBase62 } from '../../ids.ts';
import { recordAudit } from '../../services/audit.ts';
import {
  ApiKeyCreatedSchema,
  ApiKeyInput,
  ApiKeyListSchema,
  body,
  commonErrors,
  IdParam,
  json,
} from '../schemas.ts';
import { iso, isoOrNull, security, type App } from '../util.ts';

const tag = ['api-keys'];

type Row = typeof apiKey.$inferSelect;
const asPermissions = (scopes: string[]): Permission[] => scopes.filter(isPermission);

const toDto = (r: Row) => ({
  id: r.publicId,
  name: r.name,
  prefix: r.prefix,
  scopes: asPermissions(r.scopes),
  lastUsedAt: isoOrNull(r.lastUsedAt),
  expiresAt: isoOrNull(r.expiresAt),
  revokedAt: isoOrNull(r.revokedAt),
  createdAt: iso(r.createdAt),
});

/** `sk_scrin_` + 40 base62 chars (~238 bits). Only the SHA-256 is stored. */
function generateApiKey(): { token: string; prefix: string; hash: string } {
  const token = `${API_KEY_PREFIX}${randomBase62(40)}`;
  return { token, prefix: token.slice(0, API_KEY_PREFIX.length + 6), hash: sha256Hex(token) };
}

export function registerApiKeyRoutes(app: App, deps: AppDeps): void {
  const now = () => deps.now?.() ?? new Date();

  app.openapi(
    createRoute({
      method: 'get',
      path: '/v1/api-keys',
      operationId: 'listApiKeys',
      summary: 'List API keys of the organisation',
      tags: tag,
      security,
      responses: { 200: json(ApiKeyListSchema, 'Keys'), ...commonErrors },
    }),
    async (c) => {
      const scope = authorize(c.var.actor, deps, 'api_keys:manage');
      const rows = await deps.db
        .select()
        .from(apiKey)
        .where(scope.where(apiKey))
        .orderBy(desc(apiKey.createdAt));
      return c.json({ items: rows.map(toDto) }, 200);
    },
  );

  app.openapi(
    createRoute({
      method: 'post',
      path: '/v1/api-keys',
      operationId: 'createApiKey',
      summary: 'Create an API key; the token is returned once',
      tags: tag,
      security,
      request: { body: body(ApiKeyInput) },
      responses: { 201: json(ApiKeyCreatedSchema, 'Created'), ...commonErrors },
    }),
    async (c) => {
      const actor = c.var.actor;
      const scope = authorize(actor, deps, 'api_keys:manage');
      // Keys are minted by people, never by other keys (no privilege laundering).
      if (actor.kind !== 'user') throw forbidden('API keys cannot create API keys');
      const input = c.req.valid('json');
      const excess = input.scopes.filter((s) => !actor.permissions.has(s));
      if (excess.length > 0)
        throw forbidden(`You cannot grant scopes you do not hold: ${excess.join(', ')}`);
      const { token, prefix, hash } = generateApiKey();
      const publicId = newId('key');
      const t = now();
      const row = await deps.db.transaction(async (tx) => {
        const [r] = await tx
          .insert(apiKey)
          .values({
            publicId,
            orgId: scope.orgId,
            createdBy: actor.userId,
            name: input.name,
            prefix,
            hash,
            scopes: [...new Set(input.scopes)],
            expiresAt:
              input.expiresInDays === undefined
                ? null
                : new Date(t.getTime() + input.expiresInDays * 86_400_000),
          })
          .returning();
        await recordAudit(
          tx,
          scope.orgId,
          {
            ...actorRef(actor),
            action: 'api_key.created',
            targetType: 'api_key',
            targetId: publicId,
            data: { name: input.name, scopes: input.scopes },
          },
          t,
        );
        return r;
      });
      if (row === undefined) throw new Error('insert returned no row');
      return c.json({ ...toDto(row), token }, 201);
    },
  );

  app.openapi(
    createRoute({
      method: 'delete',
      path: '/v1/api-keys/{id}',
      operationId: 'revokeApiKey',
      summary: 'Revoke an API key',
      tags: tag,
      security,
      request: { params: IdParam },
      responses: { 204: { description: 'Revoked' }, ...commonErrors },
    }),
    async (c) => {
      const actor = c.var.actor;
      const scope = authorize(actor, deps, 'api_keys:manage');
      const id = c.req.valid('param').id;
      await deps.db.transaction(async (tx) => {
        const revoked = await tx
          .update(apiKey)
          .set({ revokedAt: now() })
          .where(scope.where(apiKey, eq(apiKey.publicId, id), isNull(apiKey.revokedAt)))
          .returning({ id: apiKey.id });
        if (revoked.length === 0) throw notFound('API key');
        await recordAudit(
          tx,
          scope.orgId,
          { ...actorRef(actor), action: 'api_key.revoked', targetType: 'api_key', targetId: id },
          now(),
        );
      });
      return c.body(null, 204);
    },
  );
}
