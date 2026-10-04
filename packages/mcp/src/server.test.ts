import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { InMemoryTransport } from '@modelcontextprotocol/sdk/inMemory.js';
import { createScrinClient } from '@scrin/sdk';
import { afterEach, describe, expect, it } from 'vitest';
import { createScrinMcpServer } from './server.ts';

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });

interface Seen {
  method: string;
  url: string;
  body: string;
}

let close: (() => Promise<void>) | undefined;
afterEach(async () => {
  await close?.();
  close = undefined;
});

async function connect(route: (method: string, path: string) => Response) {
  const seen: Seen[] = [];
  const fetchFn = async (input: Request | string | URL, init?: RequestInit) => {
    const req = input instanceof Request ? input : new Request(input, init);
    const url = new URL(req.url);
    seen.push({
      method: req.method,
      url: url.pathname + url.search,
      body: req.body === null ? '' : await req.text(),
    });
    return route(req.method, url.pathname);
  };
  const api = createScrinClient({
    baseUrl: 'https://api.test',
    apiKey: 'sk_scrin_x',
    fetch: fetchFn,
  });
  const server = createScrinMcpServer(api);
  const [clientT, serverT] = InMemoryTransport.createLinkedPair();
  const client = new Client({ name: 'test', version: '0.0.0' });
  await Promise.all([server.connect(serverT), client.connect(clientT)]);
  close = async () => {
    await client.close();
    await server.close();
  };
  return { client, seen };
}

function text(result: unknown): string {
  const r = result as { content: { type: string; text: string }[] };
  return r.content.map((c) => c.text).join('');
}

describe('scrin MCP server', () => {
  it('exposes exactly the six tools with input schemas', async () => {
    const { client } = await connect(() => json({}));
    const { tools } = await client.listTools();
    expect(tools.map((t) => t.name).sort()).toEqual([
      'approve_jit',
      'get_device',
      'list_devices',
      'list_sessions',
      'request_jit_access',
      'verify_audit',
    ]);
    const approve = tools.find((t) => t.name === 'approve_jit');
    expect(approve?.annotations?.destructiveHint).toBe(true);
    expect(approve?.inputSchema.required).toEqual(['grantId']);
  });

  it('list_devices maps arguments to the query string', async () => {
    const { client, seen } = await connect(() => json({ items: [], total: 0 }));
    const r = await client.callTool({
      name: 'list_devices',
      arguments: { tag: 'lobby', limit: 5 },
    });
    expect(r.isError).toBeFalsy();
    expect(seen[0]?.url).toBe('/v1/devices?limit=5&offset=0&tag=lobby');
    expect(JSON.parse(text(r))).toEqual({ items: [], total: 0 });
  });

  it('get_device, list_sessions and verify_audit hit the right endpoints', async () => {
    const { client, seen } = await connect((_m, path) =>
      path === '/v1/audit/verify'
        ? json({ valid: true, count: 3, headHash: 'a'.repeat(64), brokenAt: null })
        : path === '/v1/sessions'
          ? json({ items: [] })
          : json({ id: 'dev_1' }),
    );
    await client.callTool({ name: 'get_device', arguments: { deviceId: 'dev_1' } });
    await client.callTool({ name: 'list_sessions', arguments: { deviceId: 'dev_1' } });
    const v = await client.callTool({ name: 'verify_audit', arguments: {} });
    expect(seen.map((s) => s.url)).toEqual([
      '/v1/devices/dev_1',
      '/v1/sessions?limit=50&device=dev_1',
      '/v1/audit/verify',
    ]);
    expect(JSON.parse(text(v))).toMatchObject({ valid: true, count: 3 });
  });

  it('request_jit_access and approve_jit send the right bodies', async () => {
    const { client, seen } = await connect(() => json({ id: 'jit_1', status: 'pending' }, 201));
    await client.callTool({
      name: 'request_jit_access',
      arguments: { deviceId: 'dev_1', reason: 'ticket 9' },
    });
    await client.callTool({ name: 'approve_jit', arguments: { grantId: 'jit_1', note: 'ok' } });
    expect(seen[0]).toMatchObject({ method: 'POST', url: '/v1/jit' });
    expect(JSON.parse(seen[0]?.body ?? '')).toEqual({
      deviceId: 'dev_1',
      reason: 'ticket 9',
      durationMinutes: 60,
    });
    expect(seen[1]).toMatchObject({ method: 'POST', url: '/v1/jit/jit_1/approve' });
    expect(JSON.parse(seen[1]?.body ?? '')).toEqual({ note: 'ok' });
  });

  it('API errors become isError tool results, not protocol errors', async () => {
    const { client } = await connect(() =>
      json(
        {
          error: { code: 'self_approval', message: 'You cannot decide on your own access request' },
        },
        403,
      ),
    );
    const r = await client.callTool({ name: 'approve_jit', arguments: { grantId: 'jit_1' } });
    expect(r.isError).toBe(true);
    expect(text(r)).toBe(
      'scrin API error 403 self_approval: You cannot decide on your own access request',
    );
  });

  it('rejects invalid arguments before calling the API', async () => {
    const { client, seen } = await connect(() => json({}));
    const r = await client.callTool({ name: 'get_device', arguments: { deviceId: 'x' } });
    expect(r.isError).toBe(true);
    expect(seen).toHaveLength(0);
  });
});
