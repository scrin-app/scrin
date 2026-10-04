import { describe, expect, it } from 'vitest';

import { ByteQueue, concat, fromHex, toHex } from './bytes';
import {
  browserBidiId,
  decodeHandshake,
  decodeVarint,
  decodeWsFrame,
  encodeFrame,
  encodeHandshake,
  encodeVarint,
  encodeWsFrame,
  isHostStream,
  negotiateVersion,
  peekShard,
  readFrame,
  streamHeader,
  streamLane,
  wsCloseToCode,
  type WsFrame,
} from './framing';

describe('QUIC varint', () => {
  it('round-trips at every width', () => {
    for (const v of [0, 63, 64, 16_383, 16_384, 2 ** 30 - 1, 2 ** 30, 2 ** 40 + 5]) {
      const b = encodeVarint(v);
      expect(decodeVarint(b)).toEqual([v, b.length]);
    }
    expect(encodeVarint(Number.MAX_SAFE_INTEGER)).toHaveLength(8);
  });

  it('matches the RFC 9000 §A.1 examples', () => {
    expect(decodeVarint(Uint8Array.of(0x25))).toEqual([37, 1]);
    expect(decodeVarint(Uint8Array.of(0x7b, 0xbd))).toEqual([15_293, 2]);
    expect(decodeVarint(Uint8Array.of(0x9d, 0x7f, 0x3e, 0x7d))).toEqual([494_878_333, 4]);
    expect(toHex(encodeVarint(15_293))).toBe('7bbd');
    expect(toHex(encodeVarint(494_878_333))).toBe('9d7f3e7d');
  });

  it('rejects truncation and bad input', () => {
    expect(() => decodeVarint(new Uint8Array(0))).toThrow();
    expect(() => decodeVarint(Uint8Array.of(0x40))).toThrow();
    expect(() => encodeVarint(-1)).toThrow();
    expect(() => encodeVarint(1.5)).toThrow();
  });
});

describe('WebSocket gateway frames (GATEWAY.md §3)', () => {
  const frames: WsFrame[] = [
    { type: 'data', id: 0, payload: new TextEncoder().encode('hello') },
    { type: 'data', id: 1000, payload: new Uint8Array(0) },
    { type: 'dgram', payload: Uint8Array.of(1, 2, 3) },
    { type: 'fin', id: 4 },
    { type: 'reset', id: 7, code: 300 },
    { type: 'stop', id: 9, code: 1 },
  ];

  it('round-trips every frame type', () => {
    for (const f of frames) expect(decodeWsFrame(encodeWsFrame(f))).toEqual(f);
  });

  it('matches the server byte layout', () => {
    expect(toHex(encodeWsFrame(frames[0]!))).toBe('000068656c6c6f');
    expect(toHex(encodeWsFrame(frames[1]!))).toBe('0043e8');
    expect(toHex(encodeWsFrame(frames[4]!))).toBe('0307412c');
    expect(toHex(encodeWsFrame(frames[2]!))).toBe('01010203');
  });

  it('rejects malformed frames like the server', () => {
    expect(() => decodeWsFrame(new Uint8Array(0))).toThrow(/empty/);
    expect(() => decodeWsFrame(Uint8Array.of(9, 0))).toThrow(/unknown tag/);
    expect(() => decodeWsFrame(Uint8Array.of(0, 0x40))).toThrow(/truncated/);
    expect(() => decodeWsFrame(Uint8Array.of(2, 1, 0))).toThrow(/trailing/);
  });

  it('numbers streams per QUIC and maps close codes', () => {
    expect([0, 1, 2].map(browserBidiId)).toEqual([0, 4, 8]);
    expect(isHostStream(5)).toBe(true);
    expect(isHostStream(4)).toBe(false);
    expect(wsCloseToCode(4259)).toBe(0x103);
    expect(wsCloseToCode(4000)).toBe(0);
    expect(wsCloseToCode(1006)).toBeNull();
  });
});

describe('stream header, lanes and frames', () => {
  it('encodes the 3-byte header and lane ids', () => {
    expect(toHex(streamHeader(0, 0))).toBe('000000');
    expect(toHex(streamHeader(1, 258))).toBe('010102');
    expect(streamLane(false, 0, 0)).toBe(0);
    expect(streamLane(false, 1, 0)).toBe(0x0001_0000);
    expect(streamLane(true, 3, 2)).toBe(0x0103_0002);
  });

  it('reads frames split across arbitrary chunks', async () => {
    const q = new ByteQueue();
    const bytes = concat(encodeFrame(Uint8Array.of(1, 2, 3)), encodeFrame(new Uint8Array(0)));
    for (const b of bytes) q.push(Uint8Array.of(b));
    q.end();
    expect(await readFrame(q)).toEqual(Uint8Array.of(1, 2, 3));
    expect(await readFrame(q)).toEqual(new Uint8Array(0));
    expect(await readFrame(q)).toBeNull();
  });

  it('caps the frame length before allocating and detects truncation', async () => {
    const big = new ByteQueue();
    big.push(fromHex('00100000'));
    await expect(readFrame(big, 1024)).rejects.toThrow(/over the cap/);
    const cut = new ByteQueue();
    cut.push(fromHex('0000000501'));
    cut.end();
    await expect(readFrame(cut)).rejects.toThrow(/mid-frame/);
  });

  it('wakes a pending read when data arrives later', async () => {
    const q = new ByteQueue();
    const pending = readFrame(q);
    q.push(fromHex('00000001'));
    q.push(Uint8Array.of(42));
    expect(await pending).toEqual(Uint8Array.of(42));
    const failed = readFrame(q);
    q.fail(new Error('reset'));
    await expect(failed).rejects.toThrow('reset');
  });
});

describe('handshake messages', () => {
  it('round-trips and validates lengths', () => {
    const msgs = [
      { type: 'hello', min: 1, max: 3, intent: 0 },
      { type: 'pairStart', msg: Uint8Array.of(1, 2, 3) },
      { type: 'pairConfirm', tag: new Uint8Array(32).fill(9) },
      { type: 'result', reason: null },
      { type: 'result', reason: 4 },
      { type: 'identify', id: new Uint8Array(32).fill(1) },
      { type: 'attest', sig: new Uint8Array(64).fill(2) },
    ] as const;
    for (const m of msgs) expect(decodeHandshake(encodeHandshake(m))).toEqual(m);
    expect(toHex(encodeHandshake({ type: 'hello', min: 1, max: 1, intent: 0 }))).toBe(
      '010001000100',
    );
    expect(() => decodeHandshake(Uint8Array.of(1, 0, 1))).toThrow();
    expect(() => decodeHandshake(Uint8Array.of(2))).toThrow();
    expect(() => decodeHandshake(Uint8Array.of(3, 1, 2))).toThrow();
    expect(() => decodeHandshake(Uint8Array.of(99))).toThrow(/unknown/);
  });

  it('negotiates the highest common version', () => {
    expect(negotiateVersion(1, 5)).toBe(1);
    expect(negotiateVersion(2, 5)).toBeNull();
  });
});

describe('shard header peek', () => {
  it('reads the fec header fields', () => {
    // version 1, video, keyframe | frame 42 | shard 2 of 5 | 3 data | 16 B
    const h = fromHex('010001000000002a0002000500030010');
    expect(peekShard(h)).toEqual({
      kind: 0,
      keyframe: true,
      frameId: 42,
      shardIndex: 2,
      shardCount: 5,
    });
    expect(peekShard(h.subarray(0, 15))).toBeNull();
    expect(peekShard(Uint8Array.of(2, ...h.subarray(1)))).toBeNull();
  });
});
