import { describe, expect, it } from 'vitest';
import { createScrinClient, ScrinApiError, unwrap } from './index.ts';

interface Call {
  url: string;
  method: string;
  headers: Headers;
  body: string | null;
}

function mockFetch(respond: (call: Call) => Response) {
  const calls: Call[] = [];
  const fn = async (input: Request | string | URL, init?: RequestInit): Promise<Response> => {
    const req = input instanceof Request ? input : new Request(input, init);
    const call = {
      url: req.url,
      method: req.method,
      headers: req.headers,
      body: req.body === null ? null : await req.text(),
    };
    calls.push(call);
    return respond(call);
  };
  return { fn, calls };
}

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });

const device = {
  id: 'dev_abc',
  scrinId: '123456789',
  devicePub: 'a'.repeat(64),
  name: 'Kiosk',
  platform: 'windows' as const,
  groupId: null,
  tags: [],
  lastSeenAt: null,
  createdAt: '2026-10-04T00:00:00.000Z',
};

describe('createScrinClient', () => {
  it('sends the API key as a bearer token and builds typed paths', async () => {
    const m = mockFetch(() => json({ items: [device], total: 1 }));
    const client = createScrinClient({
      baseUrl: 'https://api.test/',
      apiKey: 'sk_scrin_x',
      fetch: m.fn,
    });
    const list = await unwrap(
      client.GET('/v1/devices', { params: { query: { limit: 10, offset: 0 } } }),
    );
    expect(list.items[0]?.name).toBe('Kiosk');
    expect(m.calls[0]?.url).toBe('https://api.test/v1/devices?limit=10&offset=0');
    expect(m.calls[0]?.headers.get('authorization')).toBe('Bearer sk_scrin_x');
  });

  it('fills path params and JSON bodies', async () => {
    const m = mockFetch(() =>
      json(
        {
          id: 'jit_1',
          deviceId: 'dev_abc',
          requesterUserId: 'u1',
          reason: 'r',
          windowStart: '2026-10-04T00:00:00.000Z',
          windowEnd: '2026-10-04T01:00:00.000Z',
          status: 'approved',
          approverUserId: 'u2',
          decisionNote: null,
          decidedAt: '2026-10-04T00:01:00.000Z',
          createdAt: '2026-10-04T00:00:00.000Z',
        },
        200,
      ),
    );
    const client = createScrinClient({ baseUrl: 'https://api.test', orgId: 'org_1', fetch: m.fn });
    const g = await unwrap(
      client.POST('/v1/jit/{id}/approve', {
        params: { path: { id: 'jit_1' } },
        body: { note: 'ok' },
      }),
    );
    expect(g.status).toBe('approved');
    expect(m.calls[0]).toMatchObject({
      url: 'https://api.test/v1/jit/jit_1/approve',
      method: 'POST',
    });
    expect(JSON.parse(m.calls[0]?.body ?? '')).toEqual({ note: 'ok' });
    expect(m.calls[0]?.headers.get('x-scrin-org')).toBe('org_1');
    expect(m.calls[0]?.headers.get('authorization')).toBeNull();
  });

  it('unwrap throws ScrinApiError with the error contract fields', async () => {
    const m = mockFetch(() =>
      json({ error: { code: 'not_found', message: 'Device not found' } }, 404),
    );
    const client = createScrinClient({ baseUrl: 'https://api.test', apiKey: 'k', fetch: m.fn });
    const err = await unwrap(
      client.GET('/v1/devices/{id}', { params: { path: { id: 'dev_x' } } }),
    ).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(ScrinApiError);
    expect(err).toMatchObject({ status: 404, code: 'not_found', message: 'Device not found' });
  });

  it('unwrap copes with a non-contract error body', async () => {
    const m = mockFetch(() => new Response('upstream down', { status: 502 }));
    const client = createScrinClient({ baseUrl: 'https://api.test', apiKey: 'k', fetch: m.fn });
    await expect(unwrap(client.GET('/v1/me'))).rejects.toMatchObject({
      status: 502,
      code: 'http_error',
    });
  });
});
