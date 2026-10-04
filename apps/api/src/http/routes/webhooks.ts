import { createRoute, z } from '@hono/zod-openapi';
import { asc, desc, eq } from 'drizzle-orm';
import { actorRef, authorize } from '../../auth/actor.ts';
import type { AppDeps } from '../../context.ts';
import type { Scope } from '../../db/scoped.ts';
import { webhook, webhookDelivery } from '../../db/schema/index.ts';
import { ApiError, notFound } from '../../errors.ts';
import { newId, randomBase62 } from '../../ids.ts';
import { recordAudit } from '../../services/audit.ts';
import {
  body,
  commonErrors,
  IdParam,
  json,
  Limit,
  WebhookCreatedSchema,
  WebhookDeliveryListSchema,
  WebhookInput,
  WebhookListSchema,
} from '../schemas.ts';
import { iso, isoOrNull, security, type App } from '../util.ts';

const tag = ['webhooks'];

const DeliveryStatus = z.enum(['pending', 'succeeded', 'failed']);

/** Literal private / loopback / link-local hosts. DNS rebinding needs an egress proxy on top. */
const PRIVATE_HOST =
  /^(localhost|.*\.localhost|.*\.internal|127\.\d+\.\d+\.\d+|10\.\d+\.\d+\.\d+|192\.168\.\d+\.\d+|172\.(1[6-9]|2\d|3[01])\.\d+\.\d+|169\.254\.\d+\.\d+|0\.0\.0\.0|\[::1?\]|\[f[cd][0-9a-f:]*\]|\[fe80:[0-9a-f:]*\])$/i;

export function assertWebhookUrl(raw: string, allowInsecure: boolean): void {
  const u = new URL(raw);
  if (allowInsecure) {
    if (u.protocol !== 'https:' && u.protocol !== 'http:') {
      throw new ApiError(400, 'invalid_webhook_url', 'Webhook URL must be http(s)');
    }
    return;
  }
  if (u.protocol !== 'https:')
    throw new ApiError(400, 'invalid_webhook_url', 'Webhook URL must use https');
  if (PRIVATE_HOST.test(u.hostname)) {
    throw new ApiError(
      400,
      'invalid_webhook_url',
      'Webhook URL must not point at a private address',
    );
  }
}

type Hook = typeof webhook.$inferSelect;
const toDto = (w: Hook) => ({
  id: w.publicId,
  url: w.url,
  events: w.events,
  active: w.active,
  createdAt: iso(w.createdAt),
});

async function find(scope: Scope, id: string): Promise<Hook> {
  const [w] = await scope.db
    .select()
    .from(webhook)
    .where(scope.where(webhook, eq(webhook.publicId, id)))
    .limit(1);
  if (w === undefined) throw notFound('Webhook');
  return w;
}

export function registerWebhookRoutes(app: App, deps: AppDeps): void {
  const now = () => deps.now?.() ?? new Date();

  app.openapi(
    createRoute({
      method: 'get',
      path: '/v1/webhooks',
      operationId: 'listWebhooks',
      summary: 'List webhooks',
      tags: tag,
      security,
      responses: { 200: json(WebhookListSchema, 'Webhooks'), ...commonErrors },
    }),
    async (c) => {
      const scope = authorize(c.var.actor, deps, 'webhooks:manage');
      const rows = await deps.db
        .select()
        .from(webhook)
        .where(scope.where(webhook))
        .orderBy(asc(webhook.createdAt));
      return c.json({ items: rows.map(toDto) }, 200);
    },
  );

  app.openapi(
    createRoute({
      method: 'post',
      path: '/v1/webhooks',
      operationId: 'createWebhook',
      summary: 'Create a webhook; the signing secret is returned once',
      tags: tag,
      security,
      request: { body: body(WebhookInput) },
      responses: { 201: json(WebhookCreatedSchema, 'Created'), ...commonErrors },
    }),
    async (c) => {
      const actor = c.var.actor;
      const scope = authorize(actor, deps, 'webhooks:manage');
      const input = c.req.valid('json');
      assertWebhookUrl(input.url, deps.webhookAllowInsecure);
      const publicId = newId('whk');
      const secret = `whsec_${randomBase62(40)}`;
      const row = await deps.db.transaction(async (tx) => {
        const [w] = await tx
          .insert(webhook)
          .values({
            publicId,
            orgId: scope.orgId,
            url: input.url,
            secret,
            events: [...new Set(input.events)],
          })
          .returning();
        await recordAudit(
          tx,
          scope.orgId,
          {
            ...actorRef(actor),
            action: 'webhook.created',
            targetType: 'webhook',
            targetId: publicId,
            data: { url: input.url, events: input.events },
          },
          now(),
        );
        return w;
      });
      if (row === undefined) throw new Error('insert returned no row');
      return c.json({ ...toDto(row), secret }, 201);
    },
  );

  app.openapi(
    createRoute({
      method: 'delete',
      path: '/v1/webhooks/{id}',
      operationId: 'deleteWebhook',
      summary: 'Delete a webhook and its pending deliveries',
      tags: tag,
      security,
      request: { params: IdParam },
      responses: { 204: { description: 'Deleted' }, ...commonErrors },
    }),
    async (c) => {
      const actor = c.var.actor;
      const scope = authorize(actor, deps, 'webhooks:manage');
      const w = await find(scope, c.req.valid('param').id);
      await deps.db.transaction(async (tx) => {
        await tx.delete(webhook).where(scope.where(webhook, eq(webhook.id, w.id)));
        await recordAudit(
          tx,
          scope.orgId,
          {
            ...actorRef(actor),
            action: 'webhook.deleted',
            targetType: 'webhook',
            targetId: w.publicId,
          },
          now(),
        );
      });
      return c.body(null, 204);
    },
  );

  app.openapi(
    createRoute({
      method: 'get',
      path: '/v1/webhooks/{id}/deliveries',
      operationId: 'listWebhookDeliveries',
      summary: 'Recent delivery attempts of a webhook',
      tags: tag,
      security,
      request: { params: IdParam, query: z.object({ limit: Limit }) },
      responses: { 200: json(WebhookDeliveryListSchema, 'Deliveries'), ...commonErrors },
    }),
    async (c) => {
      const scope = authorize(c.var.actor, deps, 'webhooks:manage');
      const w = await find(scope, c.req.valid('param').id);
      const rows = await deps.db
        .select()
        .from(webhookDelivery)
        .where(scope.where(webhookDelivery, eq(webhookDelivery.webhookId, w.id)))
        .orderBy(desc(webhookDelivery.createdAt), desc(webhookDelivery.id))
        .limit(c.req.valid('query').limit);
      return c.json(
        {
          items: rows.map((d) => ({
            id: d.publicId,
            event: d.event,
            status: DeliveryStatus.parse(d.status),
            attempts: d.attempts,
            lastStatusCode: d.lastStatusCode,
            lastError: d.lastError,
            nextAttemptAt: iso(d.nextAttemptAt),
            deliveredAt: isoOrNull(d.deliveredAt),
            createdAt: iso(d.createdAt),
          })),
        },
        200,
      );
    },
  );
}
