/**
 * Byte-level framing of the gateway path:
 *
 * - QUIC varints (RFC 9000 §16) and the WebSocket fallback frames of
 *   `crates/scrin-server/GATEWAY.md` §3;
 * - the 3-byte stream header and `u32 BE` length-prefixed frames of
 *   `docs/protocol/gateway-session.md` §2;
 * - the handshake messages (`scrin_net::handshake` + `Identify`/`Attest`).
 */

import { concat, type ByteQueue } from './bytes';

// ---- QUIC varint ------------------------------------------------------------

export function encodeVarint(v: number): Uint8Array {
  if (!Number.isSafeInteger(v) || v < 0) throw new RangeError(`varint out of range: ${v}`);
  if (v < 2 ** 6) return Uint8Array.of(v);
  if (v < 2 ** 14) return Uint8Array.of(0x40 | (v >>> 8), v & 0xff);
  if (v < 2 ** 30) {
    const b = new Uint8Array(4);
    new DataView(b.buffer).setUint32(0, (v | 0x8000_0000) >>> 0);
    return b;
  }
  const b = new Uint8Array(8);
  new DataView(b.buffer).setBigUint64(0, BigInt(v) | 0xc000_0000_0000_0000n);
  return b;
}

/** `[value, bytes used]`; throws on truncation. */
export function decodeVarint(buf: Uint8Array, at = 0): [number, number] {
  const first = buf[at];
  if (first === undefined) throw new Error('varint: truncated');
  const len = 1 << (first >> 6);
  if (at + len > buf.length) throw new Error('varint: truncated');
  let v = BigInt(first & 0x3f);
  for (let i = 1; i < len; i += 1) v = (v << 8n) | BigInt(buf[at + i] ?? 0);
  return [Number(v), len];
}

// ---- WebSocket gateway frames (GATEWAY.md §3) --------------------------------

export const WS_TAG = { data: 0x00, dgram: 0x01, fin: 0x02, reset: 0x03, stop: 0x04 } as const;
/** Largest WebSocket message the gateway accepts. */
export const WS_MAX_MESSAGE = 256 * 1024;
/** Offset of gateway/application close codes on WebSocket. */
export const WS_CLOSE_OFFSET = 4000;

export type WsFrame =
  | { type: 'data'; id: number; payload: Uint8Array }
  | { type: 'dgram'; payload: Uint8Array }
  | { type: 'fin'; id: number }
  | { type: 'reset'; id: number; code: number }
  | { type: 'stop'; id: number; code: number };

export function encodeWsFrame(f: WsFrame): Uint8Array {
  switch (f.type) {
    case 'data':
      return concat(Uint8Array.of(WS_TAG.data), encodeVarint(f.id), f.payload);
    case 'dgram':
      return concat(Uint8Array.of(WS_TAG.dgram), f.payload);
    case 'fin':
      return concat(Uint8Array.of(WS_TAG.fin), encodeVarint(f.id));
    default:
      return concat(
        Uint8Array.of(f.type === 'reset' ? WS_TAG.reset : WS_TAG.stop),
        encodeVarint(f.id),
        encodeVarint(f.code),
      );
  }
}

export function decodeWsFrame(msg: Uint8Array): WsFrame {
  const tag = msg[0];
  if (tag === undefined) throw new Error('ws frame: empty');
  if (tag === WS_TAG.dgram) return { type: 'dgram', payload: msg.subarray(1) };
  const [id, n] = decodeVarint(msg, 1);
  if (tag === WS_TAG.data) return { type: 'data', id, payload: msg.subarray(1 + n) };
  if (tag === WS_TAG.fin) {
    if (1 + n !== msg.length) throw new Error('ws frame: trailing bytes');
    return { type: 'fin', id };
  }
  if (tag === WS_TAG.reset || tag === WS_TAG.stop) {
    const [code, m] = decodeVarint(msg, 1 + n);
    if (1 + n + m !== msg.length) throw new Error('ws frame: trailing bytes');
    return { type: tag === WS_TAG.reset ? 'reset' : 'stop', id, code };
  }
  throw new Error(`ws frame: unknown tag ${tag}`);
}

/** Browser-initiated bidi stream ids on the WebSocket: 0, 4, 8, … */
export const browserBidiId = (n: number): number => n * 4;
/** Whether a WS stream id was opened by the host side (bit 0). */
export const isHostStream = (id: number): boolean => id % 2 === 1;

/** Maps a WebSocket close code back to the gateway/application code. */
export function wsCloseToCode(wsCode: number): number | null {
  return wsCode >= WS_CLOSE_OFFSET && wsCode < WS_CLOSE_OFFSET + 1000
    ? wsCode - WS_CLOSE_OFFSET
    : null;
}

/** Gateway close codes (GATEWAY.md §4). */
export const GW_CLOSE = {
  normal: 0x000,
  hostUnreachable: 0x101,
  sessionLimit: 0x102,
  idle: 0x103,
  quota: 0x104,
  protocol: 0x105,
  shutdown: 0x106,
  browserGone: 0x107,
  hostGone: 0x108,
} as const;

/** Application close codes (gateway-session.md §6). */
export const APP_CLOSE = {
  normal: 0x00,
  protocol: 0x01,
  pairingFailed: 0x02,
  sasMismatch: 0x03,
  timeout: 0x04,
} as const;

// ---- streams and frames (gateway-session.md §2) ------------------------------

/** `scrin_net::framing::StreamKind`. */
export const STREAM_KIND = {
  control: 0,
  input: 1,
  clipboard: 2,
  file: 3,
  chat: 4,
  tunnel: 5,
} as const;

/** Hard ceiling on one frame (`scrin_net::framing::MAX_FRAME_LEN`). */
export const MAX_FRAME_LEN = 4 * 1024 * 1024;

export function streamHeader(kind: number, ordinal: number): Uint8Array {
  return Uint8Array.of(kind, (ordinal >>> 8) & 0xff, ordinal & 0xff);
}

/** Inner-channel lane of a stream: `opener << 24 | kind << 16 | ordinal`. */
export function streamLane(hostOpened: boolean, kind: number, ordinal: number): number {
  return ((hostOpened ? 1 : 0) * 0x0100_0000 + kind * 0x1_0000 + ordinal) >>> 0;
}

/** Lane of every datagram. */
export const DATAGRAM_LANE = 0xffff_ffff;

export function encodeFrame(body: Uint8Array): Uint8Array {
  if (body.length > MAX_FRAME_LEN) throw new RangeError('frame too large');
  const out = new Uint8Array(4 + body.length);
  new DataView(out.buffer).setUint32(0, body.length);
  out.set(body, 4);
  return out;
}

/** Reads one length-prefixed frame; `null` on a clean end of stream. */
export async function readFrame(q: ByteQueue, max = MAX_FRAME_LEN): Promise<Uint8Array | null> {
  const head = await q.readExact(4);
  if (!head) return null;
  const len = new DataView(head.buffer, head.byteOffset).getUint32(0);
  if (len > Math.min(max, MAX_FRAME_LEN)) throw new Error(`frame of ${len} bytes over the cap`);
  if (len === 0) return new Uint8Array(0);
  const body = await q.readExact(len);
  if (!body) throw new Error('stream ended mid-frame');
  return body;
}

// ---- handshake messages -------------------------------------------------------

export const HS_TAG = {
  hello: 1,
  pairStart: 2,
  pairConfirm: 3,
  result: 4,
  authChallenge: 5,
  authProof: 6,
  identify: 7,
  attest: 8,
} as const;

export const PROTOCOL_VERSION_MIN = 1;
export const PROTOCOL_VERSION_MAX = 1;
export const INTENT_PAIR = 0;
/** Largest handshake message accepted (`MAX_HANDSHAKE_MSG`). */
export const MAX_HANDSHAKE_MSG = 4096;

/** `scrin_net::handshake::RejectReason` bytes. */
export const REJECT = {
  wrongCode: 1,
  codeUnavailable: 2,
  versionMismatch: 3,
  untrusted: 4,
  badSignature: 5,
  staleTimestamp: 6,
  wrongMode: 7,
} as const;

export type HandshakeMsg =
  | { type: 'hello'; min: number; max: number; intent: number }
  | { type: 'pairStart'; msg: Uint8Array }
  | { type: 'pairConfirm'; tag: Uint8Array }
  | { type: 'result'; reason: number | null }
  | { type: 'identify'; id: Uint8Array }
  | { type: 'attest'; sig: Uint8Array };

export function encodeHandshake(m: HandshakeMsg): Uint8Array {
  switch (m.type) {
    case 'hello':
      return Uint8Array.of(
        HS_TAG.hello,
        m.min >> 8,
        m.min & 0xff,
        m.max >> 8,
        m.max & 0xff,
        m.intent,
      );
    case 'pairStart':
      return concat(Uint8Array.of(HS_TAG.pairStart), m.msg);
    case 'pairConfirm':
      return concat(Uint8Array.of(HS_TAG.pairConfirm), m.tag);
    case 'result':
      return Uint8Array.of(HS_TAG.result, m.reason ?? 0);
    case 'identify':
      return concat(Uint8Array.of(HS_TAG.identify), m.id);
    default:
      return concat(Uint8Array.of(HS_TAG.attest), m.sig);
  }
}

export function decodeHandshake(buf: Uint8Array): HandshakeMsg {
  const tag = buf[0];
  const body = buf.subarray(1);
  const need = (n: number, what: string) => {
    if (body.length !== n) throw new Error(`handshake: malformed ${what}`);
  };
  switch (tag) {
    case HS_TAG.hello:
      need(5, 'hello');
      return {
        type: 'hello',
        min: ((body[0] ?? 0) << 8) | (body[1] ?? 0),
        max: ((body[2] ?? 0) << 8) | (body[3] ?? 0),
        intent: body[4] ?? 0,
      };
    case HS_TAG.pairStart:
      if (body.length === 0) throw new Error('handshake: malformed pairStart');
      return { type: 'pairStart', msg: body.slice() };
    case HS_TAG.pairConfirm:
      need(32, 'pairConfirm');
      return { type: 'pairConfirm', tag: body.slice() };
    case HS_TAG.result:
      need(1, 'result');
      return { type: 'result', reason: body[0] === 0 ? null : (body[0] ?? null) };
    case HS_TAG.identify:
      need(32, 'identify');
      return { type: 'identify', id: body.slice() };
    case HS_TAG.attest:
      need(64, 'attest');
      return { type: 'attest', sig: body.slice() };
    default:
      throw new Error('handshake: unknown message');
  }
}

/** Highest version both ranges contain, or `null`. */
export function negotiateVersion(peerMin: number, peerMax: number): number | null {
  const hi = Math.min(peerMax, PROTOCOL_VERSION_MAX);
  const lo = Math.max(peerMin, PROTOCOL_VERSION_MIN);
  return lo <= hi ? hi : null;
}

// ---- media shard header (scrin_media::fec) ------------------------------------

export const SHARD_HEADER_LEN = 16;

export interface ShardInfo {
  kind: number;
  keyframe: boolean;
  frameId: number;
  shardIndex: number;
  shardCount: number;
}

/** Peeks at a shard header for arrival feedback; `null` if malformed. */
export function peekShard(d: Uint8Array): ShardInfo | null {
  if (d.length < SHARD_HEADER_LEN || d[0] !== 1) return null;
  const v = new DataView(d.buffer, d.byteOffset, d.byteLength);
  return {
    kind: d[1] ?? 0,
    keyframe: ((d[2] ?? 0) & 1) === 1,
    frameId: v.getUint32(4),
    shardIndex: v.getUint16(8),
    shardCount: v.getUint16(10),
  };
}
