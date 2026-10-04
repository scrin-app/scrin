import { decodeWsFrame, encodeWsFrame, ownedBytes, type WsFrame } from '@scrin/protocol';
import { describe, expect, it } from 'vitest';

import { gatewayUrls, UNKNOWN_KIND_CODE, wrapWebSocket } from './transport';

class FakeSocket {
  binaryType = 'blob';
  readyState = 0;
  sent: WsFrame[] = [];
  closedWith: [number | undefined, string | undefined] | null = null;
  private handlers = new Map<string, ((ev: never) => void)[]>();

  addEventListener(type: string, l: (ev: never) => void) {
    this.handlers.set(type, [...(this.handlers.get(type) ?? []), l]);
  }
  fire(type: string, ev: unknown) {
    for (const l of this.handlers.get(type) ?? []) l(ev as never);
  }
  send(b: Uint8Array) {
    this.sent.push(decodeWsFrame(b));
  }
  close(code?: number, reason?: string) {
    this.closedWith = [code, reason];
    this.readyState = 3;
  }
  open() {
    this.readyState = 1;
    this.fire('open', {});
  }
  receive(f: WsFrame | string) {
    const data = typeof f === 'string' ? f : ownedBytes(encodeWsFrame(f)).buffer;
    this.fire('message', { data });
  }
  remoteClose(code: number, reason = '') {
    this.readyState = 3;
    this.fire('close', { code, reason });
  }
}

async function open() {
  const ws = new FakeSocket();
  const p = wrapWebSocket(ws);
  ws.open();
  return { ws, t: await p };
}

describe('gateway URLs', () => {
  it('builds the WebTransport and WebSocket endpoints', () => {
    expect(gatewayUrls({ server: 'https://s.example:8443', scrinId: '123456789' })).toEqual({
      wt: 'https://s.example:8443/v1/gw?id=123456789',
      ws: 'wss://s.example:8443/v1/ws?id=123456789',
    });
    expect(gatewayUrls({ server: 'http://127.0.0.1:8787', scrinId: '1' }).ws).toBe(
      'ws://127.0.0.1:8787/v1/ws?id=1',
    );
  });
});

describe('WebSocket fallback transport (GATEWAY.md §3)', () => {
  it('numbers browser bidi streams 0, 4, 8 and frames writes as Data/Fin', async () => {
    const { ws, t } = await open();
    expect(ws.binaryType).toBe('arraybuffer');
    const a = await t.openBidi();
    const b = await t.openBidi();
    await a.write(Uint8Array.of(1, 2));
    await b.write(Uint8Array.of(3));
    await a.finish();
    expect(ws.sent).toEqual([
      { type: 'data', id: 0, payload: Uint8Array.of(1, 2) },
      { type: 'data', id: 4, payload: Uint8Array.of(3) },
      { type: 'fin', id: 0 },
    ]);
  });

  it('delivers Data and Fin to the right stream, and datagrams to listeners', async () => {
    const { ws, t } = await open();
    const s = await t.openBidi();
    const got: Uint8Array[] = [];
    t.onDatagram((d) => got.push(d));
    ws.receive({ type: 'data', id: 0, payload: Uint8Array.of(9, 8) });
    ws.receive({ type: 'dgram', payload: Uint8Array.of(7) });
    ws.receive({ type: 'fin', id: 0 });
    expect(await s.incoming.readExact(2)).toEqual(Uint8Array.of(9, 8));
    expect(await s.incoming.readExact(1)).toBeNull();
    expect(got).toEqual([Uint8Array.of(7)]);
    t.sendDatagram(Uint8Array.of(5));
    expect(ws.sent.at(-1)).toEqual({ type: 'dgram', payload: Uint8Array.of(5) });
  });

  it('stops host-opened streams it does not expect', async () => {
    const { ws } = await open();
    ws.receive({ type: 'data', id: 1, payload: Uint8Array.of(0) });
    expect(ws.sent).toEqual([
      { type: 'stop', id: 1, code: UNKNOWN_KIND_CODE },
      { type: 'reset', id: 1, code: UNKNOWN_KIND_CODE },
    ]);
  });

  it('closes with PROTOCOL on a text or malformed message', async () => {
    const a = await open();
    a.ws.receive('hello');
    expect(a.ws.closedWith?.[0]).toBe(4000 + 0x105);
    const b = await open();
    b.ws.fire('message', { data: Uint8Array.of(9).buffer });
    expect(b.ws.closedWith?.[0]).toBe(4000 + 0x105);
  });

  it('maps close codes and fails open streams on close', async () => {
    const { ws, t } = await open();
    const s = await t.openBidi();
    const pending = s.incoming.readExact(1);
    ws.remoteClose(4000 + 0x103, 'idle');
    await expect(pending).rejects.toThrow(/closed/);
    expect(await t.closed).toEqual({ code: 0x103, reason: 'idle' });
    const other = await open();
    other.t.close(0x02, 'pairing');
    expect(other.ws.closedWith).toEqual([4002, 'pairing']);
  });

  it('rejects when the socket errors before opening', async () => {
    const ws = new FakeSocket();
    const p = wrapWebSocket(ws);
    ws.fire('error', {});
    await expect(p).rejects.toThrow(/WebSocket failed/);
  });

  it('splits large writes under the 256 KiB message cap', async () => {
    const { ws, t } = await open();
    const s = await t.openBidi();
    await s.write(new Uint8Array(600 * 1024));
    const sizes = ws.sent.map((f) => (f.type === 'data' ? f.payload.length : -1));
    expect(sizes.length).toBe(3);
    expect(Math.max(...sizes)).toBeLessThan(256 * 1024);
    expect(sizes.reduce((a, b) => a + b, 0)).toBe(600 * 1024);
  });
});
