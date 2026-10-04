import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { run, type Io } from './cli.ts';

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });

const device = {
  id: 'dev_abc',
  scrinId: '123456789',
  devicePub: 'a'.repeat(64),
  name: 'Kiosk',
  platform: 'windows',
  groupId: null,
  tags: ['lobby'],
  lastSeenAt: null,
  createdAt: '2026-10-04T00:00:00.000Z',
};
const grant = {
  id: 'jit_1',
  deviceId: 'dev_abc',
  requesterUserId: 'u1',
  reason: 'ticket 42',
  windowStart: '2026-10-04T00:00:00.000Z',
  windowEnd: '2026-10-04T01:00:00.000Z',
  status: 'pending',
  approverUserId: null,
  decisionNote: null,
  decidedAt: null,
  createdAt: '2026-10-04T00:00:00.000Z',
};

interface Harness {
  io: Io;
  out: string[];
  err: string[];
  requests: { method: string; url: string; auth: string | null; body: string }[];
}

let dir: string;

function harness(
  route: (method: string, path: string) => Response,
  env: Record<string, string> = {},
): Harness {
  const out: string[] = [];
  const err: string[] = [];
  const requests: Harness['requests'] = [];
  const fetchFn = async (input: Request | string | URL, init?: RequestInit) => {
    const req = input instanceof Request ? input : new Request(input, init);
    const url = new URL(req.url);
    requests.push({
      method: req.method,
      url: url.pathname + url.search,
      auth: req.headers.get('authorization'),
      body: req.body === null ? '' : await req.text(),
    });
    return route(req.method, url.pathname);
  };
  return {
    io: {
      out: (s) => out.push(s),
      err: (s) => err.push(s),
      env: { SCRIN_CONFIG: join(dir, 'cli.json'), ...env },
      fetch: fetchFn,
    },
    out,
    err,
    requests,
  };
}

const KEY = 'sk_scrin_testtesttesttesttesttesttesttesttest';

beforeEach(async () => {
  dir = await mkdtemp(join(tmpdir(), 'scrin-cli-'));
});
afterEach(async () => {
  await rm(dir, { recursive: true, force: true });
});

describe('scrin CLI', () => {
  it('prints usage with no arguments', async () => {
    const h = harness(() => json({}));
    expect(await run([], h.io)).toBe(0);
    expect(h.out.join('')).toContain('scrin devices list');
  });

  it('login validates the key against /v1/me and saves it', async () => {
    const h = harness(() =>
      json({
        kind: 'api_key',
        userId: 'u',
        orgId: 'org_1',
        role: null,
        apiKeyId: 'key_1',
        permissions: [],
      }),
    );
    expect(await run(['login', '--api-key', KEY, '--base-url', 'https://api.test'], h.io)).toBe(0);
    expect(h.requests[0]).toMatchObject({ url: '/v1/me', auth: `Bearer ${KEY}` });
    const saved = JSON.parse(await readFile(join(dir, 'cli.json'), 'utf8')) as {
      apiKey: string;
      baseUrl: string;
    };
    expect(saved).toEqual({ baseUrl: 'https://api.test', apiKey: KEY });
    // A later command uses the saved key without flags.
    const h2 = harness(() => json({ items: [], total: 0 }));
    expect(await run(['devices', 'list'], h2.io)).toBe(0);
    expect(h2.requests[0]?.auth).toBe(`Bearer ${KEY}`);
  });

  it('login refuses a key that is not sk_scrin_…', async () => {
    const h = harness(() => json({}));
    expect(await run(['login', '--api-key', 'nope'], h.io)).toBe(1);
    expect(h.requests).toHaveLength(0);
  });

  it('devices list: table and --json', async () => {
    const h = harness(() => json({ items: [device], total: 1 }), { SCRIN_API_KEY: KEY });
    expect(await run(['devices', 'list', '--tag', 'lobby'], h.io)).toBe(0);
    expect(h.out.join('')).toMatch(/dev_abc\s+123456789\s+Kiosk\s+windows/);
    expect(h.requests[0]?.url).toBe('/v1/devices?limit=50&offset=0&tag=lobby');
    const hj = harness(() => json({ items: [device], total: 1 }), { SCRIN_API_KEY: KEY });
    expect(await run(['devices', 'list', '--json'], hj.io)).toBe(0);
    expect(JSON.parse(hj.out.join(''))).toEqual({ items: [device], total: 1 });
  });

  it('devices show and API errors', async () => {
    const h = harness(() => json(device), { SCRIN_API_KEY: KEY });
    expect(await run(['devices', 'show', 'dev_abc'], h.io)).toBe(0);
    expect(h.out.join('')).toContain('a'.repeat(64));
    const e = harness(
      () => json({ error: { code: 'not_found', message: 'Device not found' } }, 404),
      {
        SCRIN_API_KEY: KEY,
      },
    );
    expect(await run(['devices', 'show', 'dev_x'], e.io)).toBe(1);
    expect(e.err.join('')).toBe('error 404 not_found: Device not found\n');
  });

  it('groups list', async () => {
    const h = harness(
      () =>
        json({
          items: [
            { id: 'grp_1', name: 'Kiosks', description: null, deviceCount: 3, createdAt: 'x' },
          ],
        }),
      { SCRIN_API_KEY: KEY },
    );
    expect(await run(['groups', 'list'], h.io)).toBe(0);
    expect(h.out.join('')).toMatch(/grp_1\s+Kiosks\s+3/);
  });

  it('audit verify exits 2 when the chain is broken', async () => {
    const ok = harness(
      () => json({ valid: true, count: 5, headHash: 'f'.repeat(64), brokenAt: null }),
      {
        SCRIN_API_KEY: KEY,
      },
    );
    expect(await run(['audit', 'verify'], ok.io)).toBe(0);
    expect(ok.out.join('')).toContain('valid: 5 events');
    const bad = harness(
      () =>
        json({
          valid: false,
          count: 2,
          headHash: 'f'.repeat(64),
          brokenAt: { seq: 3, id: 'aud_x', reason: 'hash_mismatch' },
        }),
      { SCRIN_API_KEY: KEY },
    );
    expect(await run(['audit', 'verify'], bad.io)).toBe(2);
    expect(bad.out.join('')).toContain('BROKEN at seq 3');
  });

  it('jit request / approve / deny / list', async () => {
    const h = harness(
      (method, path) =>
        method === 'GET'
          ? json({ items: [grant] })
          : json({
              ...grant,
              status: path.endsWith('/approve')
                ? 'approved'
                : path.endsWith('/deny')
                  ? 'denied'
                  : 'pending',
            }),
      { SCRIN_API_KEY: KEY },
    );
    expect(
      await run(['jit', 'request', 'dev_abc', '--reason', 'ticket 42', '--minutes', '30'], h.io),
    ).toBe(0);
    expect(JSON.parse(h.requests[0]?.body ?? '')).toEqual({
      deviceId: 'dev_abc',
      reason: 'ticket 42',
      durationMinutes: 30,
    });
    expect(await run(['jit', 'approve', 'jit_1', '--note', 'ok'], h.io)).toBe(0);
    expect(h.requests[1]).toMatchObject({ method: 'POST', url: '/v1/jit/jit_1/approve' });
    expect(await run(['jit', 'deny', 'jit_1'], h.io)).toBe(0);
    expect(h.out.join('')).toContain('jit_1: denied');
    expect(await run(['jit', 'list', '--status', 'pending'], h.io)).toBe(0);
    expect(h.requests[3]?.url).toBe('/v1/jit?limit=50&status=pending');
    expect(await run(['jit', 'list', '--status', 'bogus'], h.io)).toBe(1);
  });

  it('usage errors: unknown command, unknown flag, not logged in, bad --minutes', async () => {
    const h = harness(() => json({}));
    expect(await run(['frobnicate'], h.io)).toBe(1);
    expect(await run(['devices', 'list', '--wat'], h.io)).toBe(1);
    expect(await run(['devices', 'list'], h.io)).toBe(1);
    expect(h.err.join('')).toContain('Not logged in');
    const k = harness(() => json({}), { SCRIN_API_KEY: KEY });
    expect(await run(['jit', 'request', 'dev_abc', '--reason', 'x', '--minutes', '-3'], k.io)).toBe(
      1,
    );
    expect(k.requests).toHaveLength(0);
  });
});
