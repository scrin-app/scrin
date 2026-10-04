import { createRoute, OpenAPIHono, type OpenAPIObjectConfigure } from '@hono/zod-openapi';
import { Scalar } from '@scalar/hono-api-reference';
import { HTTPException } from 'hono/http-exception';
import { secureHeaders } from 'hono/secure-headers';
import { requireActor } from './auth/actor.ts';
import type { AppDeps, AppEnv } from './context.ts';
import { ApiError, codeForStatus, errorBody } from './errors.ts';
import { randomBase62 } from './ids.ts';
import { clientIp, rateLimit } from './http/rate-limit.ts';
import { registerAddressBookRoutes } from './http/routes/address-book.ts';
import { registerApiKeyRoutes } from './http/routes/api-keys.ts';
import { registerAuditRoutes } from './http/routes/audit.ts';
import { registerDeviceRoutes } from './http/routes/devices.ts';
import { registerGroupRoutes } from './http/routes/groups.ts';
import { registerJitRoutes } from './http/routes/jit.ts';
import { registerPolicyRoutes } from './http/routes/policies.ts';
import { registerSessionRoutes } from './http/routes/sessions.ts';
import { registerWebhookRoutes } from './http/routes/webhooks.ts';
import { commonErrors, HealthSchema, json, MeSchema, ReadySchema } from './http/schemas.ts';
import { security } from './http/util.ts';

export const API_VERSION = '0.1.0';

type OpenApiConfig = Exclude<
  OpenAPIObjectConfigure<AppEnv, '/openapi.json'>,
  (...args: never[]) => unknown
>;

const openApiConfig = (): OpenApiConfig => ({
  openapi: '3.1.0',
  info: {
    title: 'scrin accounts API',
    version: API_VERSION,
    description:
      'Devices, organisations, policies, audit, webhooks and just-in-time access for scrin. ' +
      'Authenticate with `Authorization: Bearer sk_scrin_…` (API key) or a session cookie from ' +
      '`/api/auth/*` plus the `X-Scrin-Org` header.',
    license: { name: 'AGPL-3.0-only', url: 'https://spdx.org/licenses/AGPL-3.0-only.html' },
  },
  servers: [{ url: '/' }],
});

const REQUEST_ID = /^[A-Za-z0-9._-]{8,64}$/;

export function createApp(deps: AppDeps) {
  const app = new OpenAPIHono<AppEnv>({
    // Zod validation failures → the shared error contract.
    defaultHook: (result, c) => {
      if (!result.success) {
        return c.json(
          errorBody('validation_failed', 'Request validation failed', result.error.issues),
          400,
        );
      }
      return undefined;
    },
  });

  app.openAPIRegistry.registerComponent('securitySchemes', 'bearerAuth', {
    type: 'http',
    scheme: 'bearer',
    description: 'API key (sk_scrin_…)',
  });
  app.openAPIRegistry.registerComponent('securitySchemes', 'sessionCookie', {
    type: 'apiKey',
    in: 'cookie',
    name: 'scrin.session_token',
    description: 'better-auth session; select the organisation with X-Scrin-Org',
  });

  // ---- request id + structured access log --------------------------------
  app.use('*', async (c, next) => {
    const incoming = c.req.header('x-request-id');
    const requestId =
      incoming !== undefined && REQUEST_ID.test(incoming) ? incoming : randomBase62(16);
    const log = deps.logger.child({ requestId });
    c.set('requestId', requestId);
    c.set('log', log);
    c.header('x-request-id', requestId);
    const started = performance.now();
    await next();
    const ms = Math.round((performance.now() - started) * 10) / 10;
    const path = new URL(c.req.url).pathname;
    if (path !== '/health') {
      log.info({ method: c.req.method, path, status: c.res.status, ms }, 'request');
    }
  });
  app.use('*', secureHeaders({ crossOriginResourcePolicy: 'same-site' }));

  // ---- error contract -------------------------------------------------------
  app.onError((err, c) => {
    if (err instanceof ApiError) {
      return c.json(errorBody(err.code, err.message, err.details ?? undefined), err.status);
    }
    if (err instanceof HTTPException) {
      const status = err.status;
      return c.json(errorBody(codeForStatus(status), err.message || 'Request failed'), status);
    }
    c.var.log.error({ err }, 'unhandled error');
    return c.json(errorBody('internal', 'Internal server error'), 500);
  });
  app.notFound((c) => c.json(errorBody('not_found', 'Route not found'), 404));

  // ---- system -----------------------------------------------------------------
  app.openapi(
    createRoute({
      method: 'get',
      path: '/health',
      operationId: 'health',
      summary: 'Liveness',
      tags: ['system'],
      responses: { 200: json(HealthSchema, 'Process is up') },
    }),
    (c) => c.json({ status: 'ok' as const }, 200),
  );
  app.openapi(
    createRoute({
      method: 'get',
      path: '/ready',
      operationId: 'ready',
      summary: 'Readiness (database reachable)',
      tags: ['system'],
      responses: {
        200: json(ReadySchema, 'Ready'),
        503: json(ReadySchema, 'Database unreachable'),
      },
    }),
    async (c) => {
      try {
        await deps.ping();
        return c.json({ status: 'ready' as const, db: 'ok' as const }, 200);
      } catch (err) {
        c.var.log.warn({ err }, 'readiness: database ping failed');
        return c.json({ status: 'unavailable' as const, db: 'down' as const }, 503);
      }
    },
  );

  // ---- better-auth ------------------------------------------------------------
  const authLimit = rateLimit({ name: 'auth', limit: 20, windowMs: 60_000, key: clientIp });
  app.use('/api/auth/sign-in/*', authLimit);
  app.use('/api/auth/sign-up/*', authLimit);
  app.use('/api/auth/request-password-reset', authLimit);
  app.use('/api/auth/two-factor/*', authLimit);
  app.on(['GET', 'POST'], '/api/auth/*', (c) => deps.auth.handler(c.req.raw));

  // ---- /v1 ----------------------------------------------------------------------
  app.use('/v1/*', rateLimit({ name: 'v1', limit: 600, windowMs: 60_000 }));
  app.use('/v1/*', requireActor(deps));

  app.openapi(
    createRoute({
      method: 'get',
      path: '/v1/me',
      operationId: 'getMe',
      summary: 'Who am I, in which organisation, with which permissions',
      tags: ['me'],
      security,
      responses: { 200: json(MeSchema, 'Caller'), ...commonErrors },
    }),
    (c) => {
      const a = c.var.actor;
      return c.json(
        {
          kind: a.kind,
          userId: a.userId,
          orgId: a.orgId,
          role: a.kind === 'user' ? a.role : null,
          apiKeyId: a.kind === 'api_key' ? a.keyId : null,
          permissions: [...a.permissions].sort(),
        },
        200,
      );
    },
  );

  registerDeviceRoutes(app, deps);
  registerGroupRoutes(app, deps);
  registerAddressBookRoutes(app, deps);
  registerPolicyRoutes(app, deps);
  registerSessionRoutes(app, deps);
  registerAuditRoutes(app, deps);
  registerWebhookRoutes(app, deps);
  registerJitRoutes(app, deps);
  registerApiKeyRoutes(app, deps);

  // ---- docs ------------------------------------------------------------------------
  app.doc31('/openapi.json', openApiConfig());
  app.get('/docs', Scalar({ url: '/openapi.json', pageTitle: 'scrin API' }));

  return app;
}

export type ScrinApp = ReturnType<typeof createApp>;

/** The OpenAPI 3.1 document, without starting a server (SDK generation). */
export function openApiDocument(app: ScrinApp): ReturnType<ScrinApp['getOpenAPI31Document']> {
  return app.getOpenAPI31Document(openApiConfig());
}
