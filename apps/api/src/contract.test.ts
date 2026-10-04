import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { openApiDocument } from './app.ts';
import { DisabledMailer } from './mailer.ts';
import { createLogger } from './logger.ts';
import {
  as,
  createOrg,
  createTestRuntime,
  ORIGIN,
  registerDevice,
  signUp,
  type TestRuntime,
  type TestUser,
} from './test/harness.ts';

let rt: TestRuntime;
let owner: TestUser;
let org: string;

beforeAll(async () => {
  rt = await createTestRuntime();
  owner = await signUp(rt, 'owner');
  org = await createOrg(rt, [{ user: owner, role: 'owner' }]);
});
afterAll(async () => {
  await rt.handle.close();
});

interface ErrorShape {
  error: { code: string; message: string; details?: unknown };
}

function expectErrorShape(body: unknown, code: string): asserts body is ErrorShape {
  expect(body).toMatchObject({ error: { code, message: expect.any(String) as unknown } });
  const keys = Object.keys((body as ErrorShape).error).sort();
  expect(keys.every((k) => ['code', 'details', 'message'].includes(k))).toBe(true);
}

describe('error contract', () => {
  it('400 validation_failed with zod issues as details', async () => {
    const res = await rt.request('/v1/devices/challenge', {
      method: 'POST',
      headers: as(owner, org),
      json: { devicePub: 'zz', scrinId: '12' },
    });
    expect(res.status).toBe(400);
    const body: unknown = await res.json();
    expectErrorShape(body, 'validation_failed');
    expect(Array.isArray(body.error.details)).toBe(true);
  });

  it('400 on malformed JSON', async () => {
    const res = await rt.request('/v1/groups', {
      method: 'POST',
      headers: { cookie: owner.cookie, 'x-scrin-org': org, 'content-type': 'application/json' },
      body: '{not json',
    });
    expect(res.status).toBe(400);
    expectErrorShape(await res.json(), 'bad_request');
  });

  it('401 / 403 / 404 / 409 shapes', async () => {
    const r401 = await rt.app.request(`${ORIGIN}/v1/devices`);
    expect(r401.status).toBe(401);
    expectErrorShape(await r401.json(), 'unauthorized');

    const viewer = await signUp(rt, 'viewer');
    const o = await createOrg(rt, [{ user: viewer, role: 'viewer' }]);
    const r403 = await rt.request('/v1/policies', {
      method: 'POST',
      headers: as(viewer, o),
      json: {
        name: 'p',
        document: {
          allowedPermissions: ['view'],
          recordingRequired: false,
          privacyModeAllowed: false,
          unattendedAllowed: false,
          sessionMaxMinutes: 60,
        },
      },
    });
    expect(r403.status).toBe(403);
    expectErrorShape(await r403.json(), 'insufficient_permission');

    const r404 = await rt.request('/v1/devices/dev_doesnotexist', { headers: as(owner, org) });
    expect(r404.status).toBe(404);
    expectErrorShape(await r404.json(), 'not_found');

    const route404 = await rt.app.request(`${ORIGIN}/nope`);
    expect(route404.status).toBe(404);
    expectErrorShape(await route404.json(), 'not_found');

    await rt.request('/v1/groups', {
      method: 'POST',
      headers: as(owner, org),
      json: { name: 'Dup' },
    });
    const r409 = await rt.request('/v1/groups', {
      method: 'POST',
      headers: as(owner, org),
      json: { name: 'Dup' },
    });
    expect(r409.status).toBe(409);
    expectErrorShape(await r409.json(), 'group_exists');
  });

  it('400 no_active_organization when a session names no organisation', async () => {
    const res = await rt.request('/v1/devices', { headers: { cookie: owner.cookie } });
    expect(res.status).toBe(400);
    expectErrorShape(await res.json(), 'no_active_organization');
  });

  it('every response carries a request id; a sane incoming one is echoed', async () => {
    const res = await rt.app.request(`${ORIGIN}/health`, {
      headers: { 'x-request-id': 'abc12345-req' },
    });
    expect(res.headers.get('x-request-id')).toBe('abc12345-req');
    const other = await rt.app.request(`${ORIGIN}/health`, {
      headers: { 'x-request-id': 'bad id with spaces' },
    });
    expect(other.headers.get('x-request-id')).toMatch(/^[0-9A-Za-z]{16}$/);
  });
});

describe('system endpoints', () => {
  it('/health and /ready', async () => {
    expect(await (await rt.app.request(`${ORIGIN}/health`)).json()).toEqual({ status: 'ok' });
    const ready = await rt.app.request(`${ORIGIN}/ready`);
    expect(ready.status).toBe(200);
    expect(await ready.json()).toEqual({ status: 'ready', db: 'ok' });
  });

  it('/ready is 503 when the database is down', async () => {
    const deps = { ...rt.deps, ping: () => Promise.reject(new Error('down')) };
    const { createApp } = await import('./app.ts');
    const app = createApp(deps);
    const res = await app.request(`${ORIGIN}/ready`);
    expect(res.status).toBe(503);
    expect(await res.json()).toEqual({ status: 'unavailable', db: 'down' });
  });

  it('/docs serves the Scalar reference', async () => {
    const res = await rt.app.request(`${ORIGIN}/docs`);
    expect(res.status).toBe(200);
    expect(await res.text()).toContain('/openapi.json');
  });
});

describe('OpenAPI document', () => {
  const expected: [string, string][] = [
    ['/health', 'get'],
    ['/ready', 'get'],
    ['/v1/me', 'get'],
    ['/v1/devices', 'get'],
    ['/v1/devices', 'post'],
    ['/v1/devices/challenge', 'post'],
    ['/v1/devices/{id}', 'get'],
    ['/v1/devices/{id}', 'patch'],
    ['/v1/devices/{id}', 'delete'],
    ['/v1/groups', 'get'],
    ['/v1/groups', 'post'],
    ['/v1/groups/{id}', 'delete'],
    ['/v1/address-book', 'get'],
    ['/v1/address-book', 'post'],
    ['/v1/address-book/{id}', 'patch'],
    ['/v1/address-book/{id}', 'delete'],
    ['/v1/policies', 'get'],
    ['/v1/policies', 'post'],
    ['/v1/policies/{id}', 'get'],
    ['/v1/policies/{id}', 'put'],
    ['/v1/policies/{id}', 'delete'],
    ['/v1/sessions', 'get'],
    ['/v1/sessions', 'post'],
    ['/v1/audit', 'get'],
    ['/v1/audit/verify', 'get'],
    ['/v1/webhooks', 'get'],
    ['/v1/webhooks', 'post'],
    ['/v1/webhooks/{id}', 'delete'],
    ['/v1/webhooks/{id}/deliveries', 'get'],
    ['/v1/jit', 'get'],
    ['/v1/jit', 'post'],
    ['/v1/jit/{id}/approve', 'post'],
    ['/v1/jit/{id}/deny', 'post'],
    ['/v1/api-keys', 'get'],
    ['/v1/api-keys', 'post'],
    ['/v1/api-keys/{id}', 'delete'],
  ];

  it('is served at /openapi.json as OpenAPI 3.1 with every route', async () => {
    const res = await rt.app.request(`${ORIGIN}/openapi.json`);
    expect(res.status).toBe(200);
    const doc = (await res.json()) as {
      openapi: string;
      info: { title: string; license: { name: string } };
      paths: Record<
        string,
        Record<string, { operationId?: string; responses: Record<string, unknown> }>
      >;
      components: { schemas: Record<string, unknown>; securitySchemes: Record<string, unknown> };
    };
    expect(doc.openapi).toBe('3.1.0');
    expect(doc.info.license.name).toBe('AGPL-3.0-only');
    for (const [path, method] of expected) {
      expect(doc.paths[path]?.[method], `${method.toUpperCase()} ${path}`).toBeDefined();
    }
    const ops = Object.values(doc.paths).flatMap((p) => Object.values(p));
    const ids = ops.map((o) => o.operationId);
    expect(ids.every((id) => typeof id === 'string')).toBe(true);
    expect(new Set(ids).size).toBe(ids.length);
    expect(Object.keys(doc.components.securitySchemes).sort()).toEqual([
      'bearerAuth',
      'sessionCookie',
    ]);
    expect(doc.components.schemas).toHaveProperty('Error');
    expect(doc.components.schemas).toHaveProperty('Device');
    // Every $ref resolves.
    const refs = [
      ...JSON.stringify(doc).matchAll(/"\$ref":"#\/components\/schemas\/([^"]+)"/g),
    ].map((m) => m[1] ?? '');
    for (const r of refs) expect(doc.components.schemas, r).toHaveProperty(r);
  });

  it('openApiDocument() matches the served document', async () => {
    const served: unknown = await (await rt.app.request(`${ORIGIN}/openapi.json`)).json();
    expect(JSON.parse(JSON.stringify(openApiDocument(rt.app)))).toEqual(served);
  });
});

describe('mailer', () => {
  it('disabled mailer is a no-op', async () => {
    const m = new DisabledMailer(createLogger('silent'));
    expect(m.enabled).toBe(false);
    await expect(m.send({ to: 'a@b.c', subject: 's', text: 't' })).resolves.toBeUndefined();
  });

  it('brivio mailer POSTs to /v1/email/send with the bearer key', async () => {
    const { BrivioMailer } = await import('./mailer.ts');
    const calls: { url: string; init: RequestInit; body: string }[] = [];
    const m = new BrivioMailer({
      apiUrl: 'https://api.brivio.test/',
      apiKey: 'brv_test',
      from: 'scrin <no-reply@x.test>',
      logger: createLogger('silent'),
      fetch: (url, init) => {
        calls.push({
          url: url instanceof Request ? url.url : url.toString(),
          init: init ?? {},
          body: typeof init?.body === 'string' ? init.body : '',
        });
        return Promise.resolve(new Response('{}', { status: 202 }));
      },
    });
    await m.send({ to: 'u@x.test', subject: 'Hi', text: 'Body' });
    expect(calls[0]?.url).toBe('https://api.brivio.test/v1/email/send');
    expect(new Headers(calls[0]?.init.headers).get('authorization')).toBe('Bearer brv_test');
    expect(JSON.parse(calls[0]?.body ?? '')).toEqual({
      from: 'scrin <no-reply@x.test>',
      to: 'u@x.test',
      subject: 'Hi',
      text: 'Body',
    });
  });

  it('sign-up with verification required sends the verification mail', async () => {
    const rt2 = await createTestRuntime({ REQUIRE_EMAIL_VERIFICATION: 'true' });
    try {
      const res = await rt2.request('/api/auth/sign-up/email', {
        method: 'POST',
        json: {
          email: `v-${Date.now()}@example.test`,
          password: 'correct horse battery staple',
          name: 'V',
        },
      });
      expect(res.status).toBe(200);
      expect(rt2.mailer.sent.map((s) => s.subject)).toContain('Verify your scrin email');
    } finally {
      await rt2.handle.close();
    }
  });

  it('registering a device still works when the org has a policy (smoke)', async () => {
    const d = await registerDevice(rt, as(owner, org));
    expect(d.id).toMatch(/^dev_/);
  });
});
