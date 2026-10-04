/**
 * Drives `GatewaySession` against a scripted host that replays
 * testvectors/gateway-session.json over the WebSocket fallback framing: the
 * browser must produce the controller bytes of the vector exactly and accept
 * the host bytes, then open sealed control frames and rebuild the keyframe
 * from sealed datagrams.
 */
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  ByteQueue,
  decodeWsFrame,
  encodeWsFrame,
  fromHex,
  readFrame,
  toHex,
  type Incoming,
  type WsFrame,
} from '@scrin/protocol';
import * as wasm from '@scrin/protocol/wasm';
import { beforeAll, describe, expect, it } from 'vitest';

import type { BrowserIdentity } from './identity';
import { GatewaySession, SessionError } from './session';
import { wrapWebSocket } from './transport';

interface Vectors {
  inputs: { code: string; controllerSeed: string; controllerEntropy: string };
  ids: { host: string; controller: string };
  handshake: { controllerControlBytes: string; hostMessages: string[] };
  channel: { hostControl: { sealed: string[] } };
  video: { accessUnit: string; frameId: number; sealedDatagramsDelivered: string[] };
}

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../../../..');
const root = (p: string) => resolve(repo, p);
const v = JSON.parse(readFileSync(root('testvectors/gateway-session.json'), 'utf8')) as Vectors;

beforeAll(() => {
  wasm.loadWasmSync(
    new Uint8Array(readFileSync(root('packages/protocol/wasm/scrin_wasm_bg.wasm'))),
  );
});

const frame = (hex: string) => {
  const b = fromHex(hex);
  const out = new Uint8Array(4 + b.length);
  new DataView(out.buffer).setUint32(0, b.length);
  out.set(b, 4);
  return out;
};

/** A WebSocket stand-in whose remote end is a scripted host. */
class ScriptedHost {
  binaryType = 'blob';
  readyState = 1;
  /** Bytes the browser wrote on stream 0 (Control). */
  readonly control = new ByteQueue();
  readonly sent: WsFrame[] = [];
  closeCode: number | undefined;
  private handlers = new Map<string, ((ev: never) => void)[]>();

  addEventListener(type: string, l: (ev: never) => void) {
    this.handlers.set(type, [...(this.handlers.get(type) ?? []), l]);
  }
  fire(type: string, ev: unknown) {
    for (const l of this.handlers.get(type) ?? []) l(ev as never);
  }
  send(b: Uint8Array) {
    const f = decodeWsFrame(b);
    this.sent.push(f);
    if (f.type === 'data' && f.id === 0) this.control.push(f.payload.slice());
  }
  close(code?: number) {
    this.closeCode = code;
    this.readyState = 3;
    this.fire('close', { code, reason: '' });
  }
  deliver(f: WsFrame) {
    this.fire('message', { data: encodeWsFrame(f).slice().buffer });
  }
  hostSays(hexMsgs: string[]) {
    for (const h of hexMsgs) this.deliver({ type: 'data', id: 0, payload: frame(h) });
  }
}

const identity: BrowserIdentity = {
  kind: 'ephemeral',
  publicKey: fromHex(v.ids.controller),
  sign: (msg) => Promise.resolve(wasm.seedSign(fromHex(v.inputs.controllerSeed), msg)),
};

async function start(opts: { code?: string; expectedHost?: Uint8Array; timeoutMs?: number } = {}) {
  const host = new ScriptedHost();
  const transport = await wrapWebSocket(host);
  const messages: Incoming[] = [];
  const video: { frameId: number; keyframe: boolean; data: Uint8Array }[] = [];
  const sas: number[][] = [];
  const session = new GatewaySession({
    transport,
    wasm,
    identity,
    code: opts.code ?? v.inputs.code,
    expectedHost: opts.expectedHost,
    controllerName: 'Browser',
    entropy: () => fromHex(v.inputs.controllerEntropy),
    handshakeTimeoutMs: opts.timeoutMs ?? 2000,
    events: {
      onSas: (e) => sas.push(e),
      onMessage: (m) => messages.push(m),
      onVideo: (frameId, keyframe, data) => video.push({ frameId, keyframe, data }),
      onClosed: () => undefined,
    },
  });
  return { host, session, messages, video, sas };
}

describe('GatewaySession against the shared vectors', () => {
  it('pairs byte-exactly, then speaks sealed envelopes and video', async () => {
    const { host, session, messages, video, sas } = await start();
    const paired = session.pair();
    host.hostSays(v.handshake.hostMessages);
    await paired;

    // Everything the browser wrote on Control up to Result(0) is the vector.
    const expected = fromHex(v.handshake.controllerControlBytes);
    const got = await host.control.readExact(expected.length);
    expect(toHex(got!)).toBe(v.handshake.controllerControlBytes);
    expect(sas).toHaveLength(1);
    expect(toHex(session.hostId!)).toBe(v.ids.host);

    // The first sealed frame after pairing is the SessionRequest (vector controllerControl[0]).
    const req = await readFrame(host.control);
    expect(req).not.toBeNull();

    // Host → browser: sealed SessionAccept, VideoConfig, Pong.
    host.hostSays(v.channel.hostControl.sealed);
    await new Promise((r) => setTimeout(r, 0));
    expect(messages.map((m) => m.type)).toEqual(['sessionAccept', 'videoConfig', 'pong']);

    // Sealed datagrams (reordered, one lost) → keyframe via FEC.
    for (const d of v.video.sealedDatagramsDelivered)
      host.deliver({ type: 'dgram', payload: fromHex(d) });
    expect(video).toHaveLength(1);
    expect(video[0]!.frameId).toBe(v.video.frameId);
    expect(video[0]!.keyframe).toBe(true);
    expect(toHex(video[0]!.data)).toBe(v.video.accessUnit);
    expect(session.arrivals.takeReport()?.datagramsReceived).toBe(
      v.video.sealedDatagramsDelivered.length,
    );

    // Input goes on its own stream (id 4) with the 01 00 00 header first.
    session.sendInput({ type: 'mouseWheel', dx: 0, dy: 120 });
    await new Promise((r) => setTimeout(r, 0));
    const input = host.sent.filter((f) => f.type === 'data' && f.id === 4);
    expect(input[0]).toEqual({ type: 'data', id: 4, payload: Uint8Array.of(1, 0, 0) });
    expect(input).toHaveLength(2);
  });

  it('fails with wrong-code when the confirmation does not verify', async () => {
    const { host, session } = await start({ code: 'K7QX-M2PB' });
    const paired = session.pair();
    host.hostSays(v.handshake.hostMessages.slice(0, 4));
    await expect(paired).rejects.toMatchObject({ failure: 'wrong-code' });
    expect(host.closeCode).toBe(4000 + 0x02);
  });

  it('refuses a host whose key differs from the directory', async () => {
    const { host, session } = await start({ expectedHost: new Uint8Array(32).fill(1) });
    const paired = session.pair();
    host.hostSays(v.handshake.hostMessages.slice(0, 2));
    await expect(paired).rejects.toBeInstanceOf(SessionError);
    await expect(paired).rejects.toMatchObject({ failure: 'identity-mismatch' });
  });

  it('maps a host Result(reason) to a failure', async () => {
    const { host, session } = await start();
    const paired = session.pair();
    host.hostSays([v.handshake.hostMessages[0]!, v.handshake.hostMessages[1]!, '0402']);
    await expect(paired).rejects.toMatchObject({ failure: 'wrong-code' });
  });

  it('times out a silent host', async () => {
    const { session } = await start({ timeoutMs: 20 });
    await expect(session.pair()).rejects.toMatchObject({ failure: 'timeout' });
  });

  it('drops datagrams that fail to open', async () => {
    const { host, session } = await start();
    const paired = session.pair();
    host.hostSays(v.handshake.hostMessages);
    await paired;
    host.deliver({ type: 'dgram', payload: new Uint8Array(40) });
    expect(session.counters.badDatagrams).toBe(1);
  });
});
