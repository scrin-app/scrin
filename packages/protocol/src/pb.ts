/**
 * Minimal protobuf (proto3) codec for the `scrin.v1.Envelope` messages the
 * browser client sends and receives. Hand-written instead of protobuf-es
 * because `buf` is not part of the toolchain yet; field numbers mirror
 * `proto/scrin/v1/*.proto` and are pinned by testvectors/gateway-session.json.
 * Unknown fields and payloads are skipped, as proto3 requires.
 */

const WT_VARINT = 0;
const WT_I64 = 1;
const WT_LEN = 2;
const WT_I32 = 5;

export class Writer {
  private buf = new Uint8Array(64);
  private len = 0;

  private ensure(n: number): void {
    if (this.len + n <= this.buf.length) return;
    let size = this.buf.length * 2;
    while (size < this.len + n) size *= 2;
    const next = new Uint8Array(size);
    next.set(this.buf.subarray(0, this.len));
    this.buf = next;
  }

  varint(v: number | bigint): this {
    let n = BigInt.asUintN(64, BigInt(v));
    this.ensure(10);
    while (n > 0x7fn) {
      this.buf[this.len++] = Number(n & 0x7fn) | 0x80;
      n >>= 7n;
    }
    this.buf[this.len++] = Number(n);
    return this;
  }

  tag(field: number, wire: number): this {
    return this.varint((field << 3) | wire);
  }

  bytes(b: Uint8Array): this {
    this.varint(b.length);
    this.ensure(b.length);
    this.buf.set(b, this.len);
    this.len += b.length;
    return this;
  }

  float(v: number): this {
    this.ensure(4);
    new DataView(this.buf.buffer).setFloat32(this.len, v, true);
    this.len += 4;
    return this;
  }

  finish(): Uint8Array {
    return this.buf.slice(0, this.len);
  }
}

export class Reader {
  pos = 0;
  constructor(readonly buf: Uint8Array) {}

  get done(): boolean {
    return this.pos >= this.buf.length;
  }

  varint(): bigint {
    let shift = 0n;
    let v = 0n;
    for (;;) {
      if (this.pos >= this.buf.length) throw new Error('protobuf: truncated varint');
      const b = this.buf[this.pos++] ?? 0;
      v |= BigInt(b & 0x7f) << shift;
      if ((b & 0x80) === 0) return v;
      shift += 7n;
      if (shift > 63n) throw new Error('protobuf: varint too long');
    }
  }

  u32(): number {
    return Number(BigInt.asUintN(32, this.varint()));
  }

  i32(): number {
    return Number(BigInt.asIntN(32, this.varint()));
  }

  u64(): number {
    return Number(BigInt.asUintN(64, this.varint()));
  }

  bool(): boolean {
    return this.varint() !== 0n;
  }

  bytes(): Uint8Array {
    const n = this.u32();
    const end = this.pos + n;
    if (end > this.buf.length) throw new Error('protobuf: truncated bytes');
    const out = this.buf.subarray(this.pos, end);
    this.pos = end;
    return out;
  }

  string(): string {
    return new TextDecoder().decode(this.bytes());
  }

  float(): number {
    if (this.pos + 4 > this.buf.length) throw new Error('protobuf: truncated float');
    const v = new DataView(this.buf.buffer, this.buf.byteOffset).getFloat32(this.pos, true);
    this.pos += 4;
    return v;
  }

  /** Field number + wire type of the next key. */
  key(): [number, number] {
    const k = this.u32();
    return [k >>> 3, k & 7];
  }

  skip(wire: number): void {
    if (wire === WT_VARINT) this.varint();
    else if (wire === WT_I64) this.pos += 8;
    else if (wire === WT_LEN) this.bytes();
    else if (wire === WT_I32) this.pos += 4;
    else throw new Error(`protobuf: unsupported wire type ${wire}`);
    if (this.pos > this.buf.length) throw new Error('protobuf: truncated field');
  }

  /** Repeated varints, packed or not. */
  repeatedU32(wire: number, into: number[]): void {
    if (wire === WT_LEN) {
      const sub = new Reader(this.bytes());
      while (!sub.done) into.push(sub.u32());
    } else into.push(this.u32());
  }
}

// ---- messages ---------------------------------------------------------------

export type MouseButtonKind = 'left' | 'right' | 'middle' | 'back' | 'forward';
const BUTTONS: readonly MouseButtonKind[] = ['left', 'right', 'middle', 'back', 'forward'];

export interface DisplayInfo {
  id: number;
  name: string;
  width: number;
  height: number;
  primary: boolean;
}

export interface VideoConfig {
  streamId: number;
  codec: number;
  width: number;
  height: number;
  fps: number;
  bitrateBps: number;
  displayId: number;
  codecConfig: Uint8Array;
}

export interface DatagramArrival {
  frameId: number;
  shardIndex: number;
  receiveDeltaUs: number;
  sizeBytes: number;
}

export interface BitrateFeedback {
  streamId: number;
  baseReceiveUs: number;
  arrivals: DatagramArrival[];
  datagramsReceived: number;
  datagramsLost: number;
  framesDropped: number;
  estimatedBps: number;
}

/** Messages the browser sends. */
export type Outgoing =
  | { type: 'sessionRequest'; requested: number[]; controllerName: string }
  | { type: 'keyEvent'; hidUsage: number; down: boolean; modifiers: number; repeat: boolean }
  | { type: 'mouseAbsolute'; displayId: number; x: number; y: number }
  | { type: 'mouseRelative'; dx: number; dy: number }
  | { type: 'mouseButton'; button: MouseButtonKind; down: boolean }
  | { type: 'mouseWheel'; dx: number; dy: number }
  | { type: 'keyframeRequest'; streamId: number; lastGoodFrameId: number }
  | ({ type: 'bitrateFeedback' } & BitrateFeedback)
  | { type: 'ping'; seq: number; t1Us: number }
  | { type: 'sessionEnd'; reason: number; message: string }
  | { type: 'chat'; id: number; text: string; sentUnixMs: number };

/** Messages the browser understands from the host. */
export type Incoming =
  | { type: 'sessionAccept'; granted: number[]; displays: DisplayInfo[]; maxDurationS: number }
  | { type: 'sessionReject'; reason: number; message: string }
  | { type: 'permissionsUpdate'; granted: number[] }
  | { type: 'sessionEnd'; reason: number; message: string }
  | ({ type: 'videoConfig' } & VideoConfig)
  | { type: 'pong'; seq: number; t1Us: number; t2Us: number; t3Us: number }
  | { type: 'chat'; id: number; text: string; sentUnixMs: number }
  | { type: 'unknown'; field: number };

/** `scrin.v1.Permission` values. */
export const PERMISSION = { view: 1, input: 2, clipboard: 3, chat: 14 } as const;
/** `scrin.v1.SessionEndReason` values used by the browser. */
export const END_REASON = {
  closedByController: 1,
  closedByHost: 2,
  timeLimit: 6,
  reported: 7,
  protocolError: 8,
} as const;
/** `scrin.v1.Codec`. */
export const CODEC_H264 = 1;

const sub = (w: Writer, field: number, inner: Writer) => w.tag(field, WT_LEN).bytes(inner.finish());
const u = (w: Writer, field: number, v: number | bigint) => {
  if (v !== 0 && v !== 0n) w.tag(field, WT_VARINT).varint(v);
};
const b = (w: Writer, field: number, v: boolean) => {
  if (v) w.tag(field, WT_VARINT).varint(1);
};
const s = (w: Writer, field: number, v: string) => {
  if (v) w.tag(field, WT_LEN).bytes(new TextEncoder().encode(v));
};
const f = (w: Writer, field: number, v: number) => {
  // proto3 omits default values (prost treats -0.0 as default too).
  if (v !== 0) w.tag(field, WT_I32).float(v);
};

function body(m: Outgoing): [number, Writer] {
  const w = new Writer();
  switch (m.type) {
    case 'sessionRequest': {
      if (m.requested.length > 0) {
        const p = new Writer();
        for (const r of m.requested) p.varint(r);
        sub(w, 1, p);
      }
      s(w, 2, m.controllerName);
      return [10, w];
    }
    case 'keyEvent':
      u(w, 1, m.hidUsage);
      b(w, 2, m.down);
      u(w, 3, m.modifiers);
      b(w, 5, m.repeat);
      return [40, w];
    case 'mouseAbsolute': {
      const a = new Writer();
      u(a, 1, m.displayId);
      f(a, 2, m.x);
      f(a, 3, m.y);
      sub(w, 1, a);
      return [41, w];
    }
    case 'mouseRelative': {
      const r = new Writer();
      u(r, 1, m.dx);
      u(r, 2, m.dy);
      sub(w, 2, r);
      return [41, w];
    }
    case 'mouseButton':
      u(w, 1, BUTTONS.indexOf(m.button) + 1);
      b(w, 2, m.down);
      return [42, w];
    case 'mouseWheel':
      u(w, 1, m.dx);
      u(w, 2, m.dy);
      return [43, w];
    case 'keyframeRequest':
      u(w, 1, m.streamId);
      u(w, 2, m.lastGoodFrameId);
      return [21, w];
    case 'bitrateFeedback': {
      u(w, 1, m.streamId);
      u(w, 2, BigInt(Math.round(m.baseReceiveUs)));
      for (const a of m.arrivals) {
        const x = new Writer();
        u(x, 1, a.frameId);
        u(x, 2, a.shardIndex);
        u(x, 3, a.receiveDeltaUs);
        u(x, 4, a.sizeBytes);
        sub(w, 3, x);
      }
      u(w, 4, m.datagramsReceived);
      u(w, 5, m.datagramsLost);
      u(w, 7, m.framesDropped);
      u(w, 8, m.estimatedBps);
      return [22, w];
    }
    case 'ping':
      u(w, 1, m.seq);
      u(w, 2, BigInt(Math.round(m.t1Us)));
      return [23, w];
    case 'sessionEnd':
      u(w, 1, m.reason);
      s(w, 2, m.message);
      return [81, w];
    default:
      u(w, 1, m.id);
      s(w, 2, m.text);
      u(w, 3, BigInt(m.sentUnixMs));
      return [80, w];
  }
}

/** Encodes one `scrin.v1.Envelope` (no length prefix). */
export function encodeEnvelope(m: Outgoing): Uint8Array {
  const [field, inner] = body(m);
  return sub(new Writer(), field, inner).finish();
}

function displayInfo(r: Reader): DisplayInfo {
  const d: DisplayInfo = { id: 0, name: '', width: 0, height: 0, primary: false };
  while (!r.done) {
    const [field, wire] = r.key();
    if (field === 1) d.id = r.u32();
    else if (field === 2) d.name = r.string();
    else if (field === 5) d.width = r.u32();
    else if (field === 6) d.height = r.u32();
    else if (field === 9) d.primary = r.bool();
    else r.skip(wire);
  }
  return d;
}

function decodePayload(field: number, r: Reader): Incoming {
  switch (field) {
    case 11: {
      const m = {
        type: 'sessionAccept' as const,
        granted: [] as number[],
        displays: [] as DisplayInfo[],
        maxDurationS: 0,
      };
      while (!r.done) {
        const [k, wire] = r.key();
        if (k === 1) r.repeatedU32(wire, m.granted);
        else if (k === 2) m.displays.push(displayInfo(new Reader(r.bytes())));
        else if (k === 3) m.maxDurationS = r.u32();
        else r.skip(wire);
      }
      return m;
    }
    case 12:
    case 81: {
      let reason = 0;
      let message = '';
      while (!r.done) {
        const [k, wire] = r.key();
        if (k === 1) reason = r.u32();
        else if (k === 2) message = r.string();
        else r.skip(wire);
      }
      return { type: field === 12 ? 'sessionReject' : 'sessionEnd', reason, message };
    }
    case 13: {
      const granted: number[] = [];
      while (!r.done) {
        const [k, wire] = r.key();
        if (k === 1) r.repeatedU32(wire, granted);
        else r.skip(wire);
      }
      return { type: 'permissionsUpdate', granted };
    }
    case 20: {
      const v: Incoming = {
        type: 'videoConfig',
        streamId: 0,
        codec: 0,
        width: 0,
        height: 0,
        fps: 0,
        bitrateBps: 0,
        displayId: 0,
        codecConfig: new Uint8Array(0),
      };
      while (!r.done) {
        const [k, wire] = r.key();
        if (k === 1) v.streamId = r.u32();
        else if (k === 2) v.codec = r.u32();
        else if (k === 3) v.width = r.u32();
        else if (k === 4) v.height = r.u32();
        else if (k === 5) v.fps = r.u32();
        else if (k === 6) v.bitrateBps = r.u32();
        else if (k === 9) v.displayId = r.u32();
        else if (k === 10) v.codecConfig = r.bytes().slice();
        else r.skip(wire);
      }
      return v;
    }
    case 24: {
      const p = { type: 'pong' as const, seq: 0, t1Us: 0, t2Us: 0, t3Us: 0 };
      while (!r.done) {
        const [k, wire] = r.key();
        if (k === 1) p.seq = r.u64();
        else if (k === 2) p.t1Us = r.u64();
        else if (k === 3) p.t2Us = r.u64();
        else if (k === 4) p.t3Us = r.u64();
        else r.skip(wire);
      }
      return p;
    }
    case 80: {
      const c = { type: 'chat' as const, id: 0, text: '', sentUnixMs: 0 };
      while (!r.done) {
        const [k, wire] = r.key();
        if (k === 1) c.id = r.u64();
        else if (k === 2) c.text = r.string();
        else if (k === 3) c.sentUnixMs = Number(BigInt.asIntN(64, r.varint()));
        else r.skip(wire);
      }
      return c;
    }
    default:
      return { type: 'unknown', field };
  }
}

/** Decodes one `scrin.v1.Envelope`; `null` when it carries no payload. */
export function decodeEnvelope(bytes: Uint8Array): Incoming | null {
  const r = new Reader(bytes);
  let out: Incoming | null = null;
  while (!r.done) {
    const [field, wire] = r.key();
    if (wire === WT_LEN) out = decodePayload(field, new Reader(r.bytes()));
    else r.skip(wire);
  }
  return out;
}
