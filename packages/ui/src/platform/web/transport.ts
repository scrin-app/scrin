/**
 * Browser ↔ gateway transports (crates/scrin-server/GATEWAY.md): WebTransport
 * maps streams and datagrams 1:1; the WebSocket fallback multiplexes them with
 * the §3 frames. Both expose the same small `GatewayTransport` surface.
 */
import {
  browserBidiId,
  ByteQueue,
  decodeWsFrame,
  encodeWsFrame,
  fromHex,
  isHostStream,
  ownedBytes,
  WS_CLOSE_OFFSET,
  WS_MAX_MESSAGE,
  wsCloseToCode,
} from '@scrin/protocol';

/** `scrin_net::framing::UNKNOWN_KIND_CODE`: stop streams we did not ask for. */
export const UNKNOWN_KIND_CODE = 0x5c01;

export interface Duplex {
  readonly incoming: ByteQueue;
  write(bytes: Uint8Array): Promise<void>;
  finish(): Promise<void>;
}

interface CloseInfo {
  /** Gateway or application close code (GATEWAY.md §4); `null` when unknown. */
  code: number | null;
  reason: string;
}

export interface GatewayTransport {
  readonly kind: 'webtransport' | 'websocket';
  openBidi(): Promise<Duplex>;
  /** Best effort; silently dropped when over size or the transport is gone. */
  sendDatagram(bytes: Uint8Array): void;
  onDatagram(listener: (bytes: Uint8Array) => void): void;
  readonly closed: Promise<CloseInfo>;
  close(code: number, reason?: string): void;
}

export interface GatewayEndpoints {
  /** `https://host:port` of the scrin server. */
  server: string;
  scrinId: string;
  /** Hex SHA-256 of a self-signed WebTransport certificate (dev). */
  certSha256?: string | undefined;
}

export function gatewayUrls(e: GatewayEndpoints): { wt: string; ws: string } {
  const base = new URL(e.server);
  const id = encodeURIComponent(e.scrinId);
  const wt = new URL(`/v1/gw?id=${id}`, base);
  wt.protocol = 'https:';
  const ws = new URL(`/v1/ws?id=${id}`, base);
  ws.protocol = base.protocol === 'http:' ? 'ws:' : 'wss:';
  return { wt: wt.toString(), ws: ws.toString() };
}

// ---- WebTransport -------------------------------------------------------------

/** Reads a byte stream; non-`Uint8Array` chunks (never sent by browsers) are ignored. */
async function pump(reader: ReadableStreamDefaultReader<unknown>, q: ByteQueue): Promise<void> {
  try {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      if (value instanceof Uint8Array) q.push(value);
    }
    q.end();
  } catch (e) {
    q.fail(e instanceof Error ? e : new Error(String(e)));
  }
}

function wtDuplex(s: WebTransportBidirectionalStream): Duplex {
  const incoming = new ByteQueue();
  void pump(s.readable.getReader(), incoming);
  const writer = s.writable.getWriter();
  return {
    incoming,
    write: (b) => writer.write(b),
    finish: () => writer.close(),
  };
}

export async function connectWebTransport(e: GatewayEndpoints): Promise<GatewayTransport> {
  if (typeof WebTransport === 'undefined') throw new Error('WebTransport unavailable');
  const opts: WebTransportOptions = { requireUnreliable: true };
  if (e.certSha256) {
    opts.serverCertificateHashes = [
      { algorithm: 'sha-256', value: ownedBytes(fromHex(e.certSha256)) },
    ];
  }
  const wt = new WebTransport(gatewayUrls(e).wt, opts);
  await wt.ready;
  const listeners = new Set<(b: Uint8Array) => void>();
  const dgWriter = wt.datagrams.writable.getWriter();
  const dgReader: ReadableStreamDefaultReader<unknown> = wt.datagrams.readable.getReader();
  void (async () => {
    try {
      for (;;) {
        const { value, done } = await dgReader.read();
        if (done) return;
        if (value instanceof Uint8Array) for (const l of listeners) l(value);
      }
    } catch {
      // Session closed; `closed` reports why.
    }
  })();
  // v1 hosts never open streams to the browser; stop any that arrive.
  void (async () => {
    const r: ReadableStreamDefaultReader<unknown> = wt.incomingBidirectionalStreams.getReader();
    try {
      for (;;) {
        const { value, done } = await r.read();
        if (done) return;
        if (value instanceof WebTransportBidirectionalStream) {
          void value.readable.cancel(UNKNOWN_KIND_CODE).catch(() => undefined);
          void value.writable.abort(UNKNOWN_KIND_CODE).catch(() => undefined);
        }
      }
    } catch {
      // Session closed.
    }
  })();
  const closed = wt.closed.then(
    (i) => ({ code: i.closeCode ?? null, reason: i.reason ?? '' }),
    (err: unknown) => ({ code: null, reason: err instanceof Error ? err.message : 'closed' }),
  );
  return {
    kind: 'webtransport',
    async openBidi() {
      return wtDuplex(await wt.createBidirectionalStream());
    },
    sendDatagram(b) {
      void dgWriter.write(b).catch(() => undefined);
    },
    onDatagram(l) {
      listeners.add(l);
    },
    closed,
    close(code, reason = '') {
      try {
        wt.close({ closeCode: code, reason });
      } catch {
        // Already closed.
      }
    },
  };
}

// ---- WebSocket fallback ---------------------------------------------------------

/** The parts of `WebSocket` the fallback uses (a fake socket implements it in tests). */
export interface WsLike {
  binaryType: string;
  readyState: number;
  send(data: Uint8Array<ArrayBuffer>): void;
  close(code?: number, reason?: string): void;
  addEventListener(type: 'message', listener: (ev: MessageEvent) => void): void;
  addEventListener(type: 'close', listener: (ev: CloseEvent) => void): void;
  addEventListener(type: 'open' | 'error', listener: () => void): void;
}

export type WebSocketFactory = (url: string) => WsLike;

const OPEN = 1;

/**
 * Multiplexes streams and datagrams over one WebSocket. Exported for tests,
 * which drive it with a fake socket.
 */
export function wrapWebSocket(ws: WsLike): Promise<GatewayTransport> {
  ws.binaryType = 'arraybuffer';
  const streams = new Map<number, ByteQueue>();
  const dgram = new Set<(b: Uint8Array) => void>();
  let nextBidi = 0;
  const { promise: closed, resolve: resolveClosed } = Promise.withResolvers<CloseInfo>();
  const send = (b: Uint8Array) => {
    if (ws.readyState === OPEN) ws.send(ownedBytes(b));
  };
  const failAll = (why: string) => {
    for (const q of streams.values()) q.fail(new Error(why));
    streams.clear();
  };

  ws.addEventListener('message', (ev: MessageEvent) => {
    if (!(ev.data instanceof ArrayBuffer)) {
      ws.close(WS_CLOSE_OFFSET + 0x105, 'text frame');
      return;
    }
    let f;
    try {
      f = decodeWsFrame(new Uint8Array(ev.data));
    } catch {
      ws.close(WS_CLOSE_OFFSET + 0x105, 'bad frame');
      return;
    }
    if (f.type === 'dgram') {
      for (const l of dgram) l(f.payload);
      return;
    }
    const q = streams.get(f.id);
    if (!q) {
      if (isHostStream(f.id) && f.type === 'data') {
        send(encodeWsFrame({ type: 'stop', id: f.id, code: UNKNOWN_KIND_CODE }));
        send(encodeWsFrame({ type: 'reset', id: f.id, code: UNKNOWN_KIND_CODE }));
      }
      return;
    }
    if (f.type === 'data') q.push(f.payload.slice());
    else if (f.type === 'fin') q.end();
    else if (f.type === 'reset') q.fail(new Error(`stream reset (${f.code})`));
  });
  ws.addEventListener('close', (ev: CloseEvent) => {
    failAll('connection closed');
    resolveClosed({ code: wsCloseToCode(ev.code), reason: ev.reason });
  });

  const transport: GatewayTransport = {
    kind: 'websocket',
    openBidi() {
      const id = browserBidiId(nextBidi);
      nextBidi += 1;
      const incoming = new ByteQueue();
      streams.set(id, incoming);
      return Promise.resolve({
        incoming,
        write(b: Uint8Array) {
          // Stay under the gateway's message cap.
          const chunk = WS_MAX_MESSAGE - 16;
          for (let at = 0; at < b.length || at === 0; at += chunk) {
            send(encodeWsFrame({ type: 'data', id, payload: b.subarray(at, at + chunk) }));
            if (b.length === 0) break;
          }
          return Promise.resolve();
        },
        finish() {
          send(encodeWsFrame({ type: 'fin', id }));
          return Promise.resolve();
        },
      });
    },
    sendDatagram(b) {
      send(encodeWsFrame({ type: 'dgram', payload: b }));
    },
    onDatagram(l) {
      dgram.add(l);
    },
    closed,
    close(code, reason = '') {
      if (ws.readyState <= OPEN) ws.close(WS_CLOSE_OFFSET + code, reason);
    },
  };

  return new Promise((resolve, reject) => {
    if (ws.readyState === OPEN) {
      resolve(transport);
      return;
    }
    ws.addEventListener('open', () => resolve(transport));
    ws.addEventListener('error', () => reject(new Error('WebSocket failed')));
  });
}

export function connectWebSocket(
  e: GatewayEndpoints,
  factory: WebSocketFactory = (url) => new WebSocket(url),
): Promise<GatewayTransport> {
  return wrapWebSocket(factory(gatewayUrls(e).ws));
}
