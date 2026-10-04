import { createServer, type IncomingMessage, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { eq } from 'drizzle-orm';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { hmacSha256Hex } from './crypto.ts';
import { webhookDelivery } from './db/schema/index.ts';
import { assertWebhookUrl } from './http/routes/webhooks.ts';
import { backoffMs, deliverDueWebhooks, MAX_ATTEMPTS, signPayload } from './services/webhooks.ts';
import {
  as,
  createOrg,
  createTestRuntime,
  registerDevice,
  signUp,
  type TestRuntime,
  type TestUser,
} from './test/harness.ts';

interface Received {
  headers: IncomingMessage['headers'];
  body: string;
}

let rt: TestRuntime;
let owner: TestUser;
let org: string;
let server: Server;
let url: string;
const received: Received[] = [];
/** Status codes to answer with, in order; then 200. */
const script: number[] = [];

beforeAll(async () => {
  rt = await createTestRuntime();
  owner = await signUp(rt, 'owner');
  org = await createOrg(rt, [{ user: owner, role: 'owner' }]);
  server = createServer((req, res) => {
    const chunks: Buffer[] = [];
    req.on('data', (c: Buffer) => chunks.push(c));
    req.on('end', () => {
      received.push({ headers: req.headers, body: Buffer.concat(chunks).toString('utf8') });
      res.statusCode = script.shift() ?? 200;
      res.end('ok');
    });
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  url = `http://127.0.0.1:${(server.address() as AddressInfo).port}/hook`;
});
afterAll(async () => {
  await new Promise<void>((resolve) => server.close(() => resolve()));
  await rt.handle.close();
});

function verifySignature(header: string, body: string, secret: string): boolean {
  const m = /^t=(\d+),v1=([0-9a-f]{64})$/.exec(header);
  if (m === null) return false;
  return hmacSha256Hex(secret, `${m[1] ?? ''}.${body}`) === m[2];
}

describe('webhook signing', () => {
  it('signPayload is HMAC-SHA256 over "<t>.<body>" (known answer)', () => {
    expect(signPayload('k', '{}', 1)).toBe(`t=1,v1=${hmacSha256Hex('k', '1.{}')}`);
  });

  it('backoff grows 4x from 30 s', () => {
    expect([1, 2, 3].map(backoffMs)).toEqual([30_000, 120_000, 480_000]);
  });

  it('refuses http and private targets unless insecure is allowed', () => {
    expect(() => assertWebhookUrl('http://example.com/h', false)).toThrow(/https/);
    expect(() => assertWebhookUrl('https://127.0.0.1/h', false)).toThrow(/private/);
    expect(() => assertWebhookUrl('https://10.1.2.3/h', false)).toThrow(/private/);
    expect(() => assertWebhookUrl('https://[::1]/h', false)).toThrow(/private/);
    expect(() => assertWebhookUrl('https://hooks.example.com/h', false)).not.toThrow();
    expect(() => assertWebhookUrl('http://127.0.0.1/h', true)).not.toThrow();
  });
});

describe('webhook delivery', () => {
  it('delivers a signed event to a local server', async () => {
    const created = await rt.request('/v1/webhooks', {
      method: 'POST',
      headers: as(owner, org),
      json: { url, events: ['device.registered'] },
    });
    expect(created.status).toBe(201);
    const hook = (await created.json()) as { id: string; secret: string };
    expect(hook.secret).toMatch(/^whsec_/);
    // The secret is never returned again.
    const list = await rt.request('/v1/webhooks', { headers: as(owner, org) });
    expect(JSON.stringify(await list.json())).not.toContain(hook.secret);

    const dev = await registerDevice(rt, as(owner, org));
    const report = await deliverDueWebhooks(rt.deps.db);
    expect(report).toMatchObject({ attempted: 1, succeeded: 1 });
    const got = received.at(-1);
    expect(got).toBeDefined();
    expect(got?.headers['scrin-event']).toBe('device.registered');
    const sig = String(got?.headers['scrin-signature']);
    expect(verifySignature(sig, got?.body ?? '', hook.secret)).toBe(true);
    expect(verifySignature(sig, `${got?.body ?? ''} `, hook.secret)).toBe(false);
    const payload = JSON.parse(got?.body ?? '{}') as { event: string; target: { id: string } };
    expect(payload).toMatchObject({ event: 'device.registered', target: { id: dev.id } });
  });

  it('retries with backoff on 5xx and gives up after MAX_ATTEMPTS', async () => {
    const o = await createOrg(rt, [{ user: owner, role: 'owner' }]);
    await rt.request('/v1/webhooks', {
      method: 'POST',
      headers: as(owner, o),
      json: { url, events: ['device.registered'] },
    });
    await registerDevice(rt, as(owner, o));

    let clock = new Date();
    const now = () => clock;
    script.push(500, 503);
    const first = await deliverDueWebhooks(rt.deps.db, { now });
    expect(first).toMatchObject({ attempted: 1, retried: 1 });
    // Not due yet: nothing happens.
    expect((await deliverDueWebhooks(rt.deps.db, { now })).attempted).toBe(0);
    clock = new Date(clock.getTime() + backoffMs(1) + 1);
    expect(await deliverDueWebhooks(rt.deps.db, { now })).toMatchObject({
      attempted: 1,
      retried: 1,
    });
    clock = new Date(clock.getTime() + backoffMs(2) + 1);
    expect(await deliverDueWebhooks(rt.deps.db, { now })).toMatchObject({
      attempted: 1,
      succeeded: 1,
    });
    const [row] = await rt.deps.db
      .select()
      .from(webhookDelivery)
      .where(eq(webhookDelivery.orgId, o));
    expect(row).toMatchObject({ status: 'succeeded', attempts: 3, lastStatusCode: 200 });

    // Permanently failing endpoint.
    const failing = await createOrg(rt, [{ user: owner, role: 'owner' }]);
    await rt.request('/v1/webhooks', {
      method: 'POST',
      headers: as(owner, failing),
      json: { url, events: ['device.registered'] },
    });
    await registerDevice(rt, as(owner, failing));
    for (let i = 0; i < MAX_ATTEMPTS; i++) {
      script.push(500);
      clock = new Date(clock.getTime() + backoffMs(MAX_ATTEMPTS) + 1);
      await deliverDueWebhooks(rt.deps.db, { now });
    }
    const [dead] = await rt.deps.db
      .select()
      .from(webhookDelivery)
      .where(eq(webhookDelivery.orgId, failing));
    expect(dead).toMatchObject({ status: 'failed', attempts: MAX_ATTEMPTS, lastError: 'HTTP 500' });
    script.length = 0;
  });

  it('a wildcard hook also receives its own creation event', async () => {
    const o = await createOrg(rt, [{ user: owner, role: 'owner' }]);
    await rt.request('/v1/webhooks', {
      method: 'POST',
      headers: as(owner, o),
      json: { url, events: ['*'] },
    });
    const rows = await rt.deps.db
      .select()
      .from(webhookDelivery)
      .where(eq(webhookDelivery.orgId, o));
    expect(rows.map((r) => r.event)).toEqual(['webhook.created']);
  });

  it('records network errors as retryable', async () => {
    const o = await createOrg(rt, [{ user: owner, role: 'owner' }]);
    await rt.request('/v1/webhooks', {
      method: 'POST',
      headers: as(owner, o),
      json: { url: 'http://127.0.0.1:1/unreachable', events: ['*'] },
    });
    await registerDevice(rt, as(owner, o));
    const r = await deliverDueWebhooks(rt.deps.db, { timeoutMs: 2_000 });
    expect(r.retried).toBeGreaterThanOrEqual(1);
  });
});
