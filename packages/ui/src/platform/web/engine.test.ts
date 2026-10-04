import { describe, expect, it } from 'vitest';

import type { EngineEvent } from '../../platform';
import { createWebEngine } from './engine';
import type { WebRuntime } from './runtime';

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

const tick = () => new Promise((r) => setTimeout(r, 0));

function engineWith(routes: Record<string, () => Response>, runtime: Partial<WebRuntime> = {}) {
  const calls: string[] = [];
  const fetchFn = (url: string | URL | Request) => {
    const u = typeof url === 'string' ? url : url instanceof URL ? url.href : url.url;
    calls.push(u);
    const path = new URL(u).pathname;
    const route = routes[path];
    return Promise.resolve(route ? route() : json(404, { error: 'not_found' }));
  };
  const rt = {
    connectWebTransport: () => Promise.reject(new Error('no wt')),
    connectWebSocket: () => Promise.reject(new Error('no ws')),
    isSessionError: (_e: unknown): _e is never => false,
    ...runtime,
  } as unknown as WebRuntime;
  const engine = createWebEngine({
    serverUrl: 'https://s.example/',
    fetch: fetchFn,
    loadRuntime: () => Promise.resolve(rt),
    dialAttempts: 1,
  });
  const events: EngineEvent[] = [];
  engine.onEvent((e) => events.push(e));
  return { engine, events, calls };
}

describe('web engine connect flow', () => {
  it('reports network-blocked when the server has no gateway', async () => {
    const { engine, events } = engineWith({ '/v1/info': () => json(200, { gateway: false }) });
    const h = await engine.connect('123 456 789', 'ABCD-EFGH');
    for (let i = 0; i < 5; i += 1) await tick();
    expect(events.map((e) => e.type)).toEqual(['stage', 'error']);
    expect(events[1]).toEqual({ type: 'error', sessionId: h.sessionId, error: 'network-blocked' });
  });

  it('reports offline when the directory does not know the id', async () => {
    const { engine, events, calls } = engineWith({
      '/v1/info': () => json(200, { gateway: true }),
    });
    await engine.connect('123456789', 'ABCDEFGH');
    for (let i = 0; i < 5; i += 1) await tick();
    expect(calls.map((c) => new URL(c).pathname)).toEqual(['/v1/info', '/v1/resolve/123456789']);
    expect(events.at(-1)).toMatchObject({ type: 'error', error: 'offline' });
  });

  it('reports offline when no transport connects (WT then WS)', async () => {
    let wt = 0;
    let ws = 0;
    const { engine, events } = engineWith(
      {
        '/v1/info': () => json(200, { gateway: true, wt_cert_sha256: 'ab' }),
        '/v1/resolve/123456789': () => json(429, { error: 'rate_limited' }),
      },
      {
        connectWebTransport: (e) => {
          wt += 1;
          expect(e.certSha256).toBe('ab');
          return Promise.reject(new Error('udp blocked'));
        },
        connectWebSocket: () => {
          ws += 1;
          return Promise.reject(new Error('down'));
        },
      },
    );
    await engine.connect('123456789', 'ABCDEFGH');
    for (let i = 0; i < 8; i += 1) await tick();
    expect([wt, ws]).toEqual([1, 1]);
    expect(events.at(-1)).toMatchObject({ type: 'error', error: 'offline' });
  });

  it('is not a host: placeholder id and code', async () => {
    const { engine } = engineWith({});
    expect(await engine.getMyId()).toBe('000000000');
    expect((await engine.getCode()).code).toBe('--------');
  });
});

/** A runtime whose session pairs at once and (optionally) is accepted by the host. */
function pairing(accept: boolean) {
  const ended: number[] = [];
  class FakeSession {
    constructor(
      private readonly o: {
        events: { onMessage(m: unknown): void; onSas(e: number[]): void };
      },
    ) {}
    pair() {
      this.o.events.onSas([1, 2, 3, 4, 5]);
      if (accept)
        this.o.events.onMessage({
          type: 'sessionAccept',
          granted: [1, 2, 3],
          displays: [],
          maxDurationS: 0,
        });
      return Promise.resolve();
    }
    end() {
      ended.push(1);
    }
  }
  const transport = { close: () => undefined };
  const env = engineWith(
    {
      '/v1/info': () => json(200, { gateway: true }),
      '/v1/resolve/123456789': () => json(429, {}),
    },
    {
      connectWebTransport: () => Promise.resolve(transport as never),
      GatewaySession: FakeSession as never,
      wasm: {} as never,
      identity: {} as never,
    },
  );
  return { ...env, ended };
}

describe('web engine cancel', () => {
  it('does not end an accepted session when the connect screen unmounts', async () => {
    const { engine, events, ended } = pairing(true);
    const h = await engine.connect('123456789', 'ABCDEFGH');
    for (let i = 0; i < 8; i += 1) await tick();
    expect(events).toContainEqual(expect.objectContaining({ type: 'stage', stage: 'connected' }));
    h.cancel();
    expect(ended).toEqual([]);
    await engine.endSession(h.sessionId);
    expect(ended).toEqual([1]);
  });

  it('ends a session that is still waiting for the host', async () => {
    const { engine, ended } = pairing(false);
    const h = await engine.connect('123456789', 'ABCDEFGH');
    for (let i = 0; i < 8; i += 1) await tick();
    h.cancel();
    expect(ended).toEqual([1]);
  });
});
