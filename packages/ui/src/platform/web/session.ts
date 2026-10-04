/**
 * One controller session through the browser gateway
 * (`docs/protocol/gateway-session.md`): Control-stream handshake (Hello,
 * Identify, SPAKE2, confirm, Attest), then sealed `scrin.v1` envelopes on the
 * Control and Input streams and sealed FEC shards on datagrams.
 */
import {
  APP_CLOSE,
  DATAGRAM_LANE,
  decodeEnvelope,
  decodeHandshake,
  encodeEnvelope,
  encodeFrame,
  encodeHandshake,
  END_REASON,
  equalBytes,
  INTENT_PAIR,
  MAX_HANDSHAKE_MSG,
  negotiateVersion,
  peekShard,
  PERMISSION,
  PROTOCOL_VERSION_MAX,
  PROTOCOL_VERSION_MIN,
  readFrame,
  REJECT,
  STREAM_KIND,
  streamHeader,
  streamLane,
  type HandshakeMsg,
  type Incoming,
  type Outgoing,
} from '@scrin/protocol';

import { ArrivalLog } from './feedback';
import { decodeClipboard, encodeClipboard, type ClipboardMessage } from './clipboard-pb';
import type { BrowserIdentity } from './identity';
import type { Duplex, GatewayTransport } from './transport';

/** The subset of `@scrin/protocol/wasm` a session uses (injectable for tests). */
export interface WasmApi {
  Pairing: new (
    code: string,
    me: Uint8Array,
    host: Uint8Array,
    entropy: Uint8Array,
  ) => {
    message(): Uint8Array;
    finish(peer: Uint8Array): WasmPaired;
  };
  Reassembler: new () => {
    push(
      d: Uint8Array,
    ): { frameId: number; keyframe: boolean; audio?: boolean; takeData(): Uint8Array } | undefined;
    stats(): Float64Array;
    audioStats?(): Float64Array;
  };
  attestMessage(
    signerIsHost: boolean,
    host: Uint8Array,
    controller: Uint8Array,
    controllerTag: Uint8Array,
    hostTag: Uint8Array,
  ): Uint8Array;
  verifySignature(publicKey: Uint8Array, msg: Uint8Array, sig: Uint8Array): boolean;
}

interface WasmPaired {
  confirmation(): Uint8Array;
  verifyPeer(tag: Uint8Array): boolean;
  sas(): Uint8Array;
  channel(): WasmChannel;
}

interface WasmChannel {
  seal(lane: number, plaintext: Uint8Array): Uint8Array;
  open(lane: number, sealed: Uint8Array): Uint8Array;
  tryOpen(lane: number, sealed: Uint8Array): Uint8Array | undefined;
}

/** Why a session attempt failed, in UI terms. */
export type SessionFailure =
  'wrong-code' | 'rejected' | 'timeout' | 'protocol' | 'identity-mismatch' | 'closed';

export class SessionError extends Error {
  constructor(
    readonly failure: SessionFailure,
    message: string,
  ) {
    super(message);
    this.name = 'SessionError';
  }
}

interface SessionEvents {
  onSas(emoji: [number, number, number, number, number]): void;
  onMessage(m: Incoming): void;
  /** Reassembled video access unit. */
  onVideo(frameId: number, keyframe: boolean, data: Uint8Array): void;
  /** Reassembled Opus packet (media kind 1). */
  onAudio?(frameId: number, data: Uint8Array): void;
  /** Clipboard envelope from the host (Clipboard or Control stream). */
  onClipboard?(m: ClipboardMessage): void;
  onClosed(info: { code: number | null; reason: string }): void;
}

export interface SessionOptions {
  transport: GatewayTransport;
  wasm: WasmApi;
  identity: BrowserIdentity;
  code: string;
  /** Host key from `/v1/resolve`, when known; a mismatch aborts pairing. */
  expectedHost?: Uint8Array | undefined;
  controllerName: string;
  events: SessionEvents;
  /** 32 fresh random bytes per attempt. */
  entropy?: () => Uint8Array;
  /** Monotonic clock in µs. */
  nowUs?: () => number;
  handshakeTimeoutMs?: number;
}

const HANDSHAKE_TIMEOUT_MS = 30_000;
const CONTROL_LANE = 0;
const INPUT_LANE = streamLane(false, STREAM_KIND.input, 0);
const CLIPBOARD_LANE = streamLane(false, STREAM_KIND.clipboard, 0);

function rejection(reason: number): SessionError {
  if (reason === REJECT.wrongCode || reason === REJECT.codeUnavailable)
    return new SessionError('wrong-code', 'the host rejected the code');
  if (reason === REJECT.badSignature) return new SessionError('identity-mismatch', 'bad signature');
  return new SessionError('rejected', `the host refused the handshake (${reason})`);
}

function isType<T extends HandshakeMsg['type']>(
  m: HandshakeMsg,
  type: T,
): m is Extract<HandshakeMsg, { type: T }> {
  return m.type === type;
}

export class GatewaySession {
  private control: Duplex | null = null;
  /** Controller-opened streams by lane (Input, Clipboard), opened on first use. */
  private readonly streams = new Map<number, Promise<Duplex>>();
  private channel: WasmChannel | null = null;
  private reassembler: InstanceType<WasmApi['Reassembler']> | null = null;
  readonly arrivals = new ArrivalLog();
  readonly counters = { datagrams: 0, bytes: 0, badDatagrams: 0 };
  private closed = false;
  hostId: Uint8Array | null = null;
  readonly nowUs: () => number;

  constructor(private readonly o: SessionOptions) {
    this.nowUs = o.nowUs ?? (() => performance.now() * 1000);
    void o.transport.closed.then((info) => {
      this.closed = true;
      o.events.onClosed(info);
    });
  }

  private async sendHs(m: HandshakeMsg): Promise<void> {
    await this.control?.write(encodeFrame(encodeHandshake(m)));
  }

  private async recvHs(): Promise<HandshakeMsg> {
    if (!this.control) throw new SessionError('protocol', 'no control stream');
    const f = await readFrame(this.control.incoming, MAX_HANDSHAKE_MSG);
    if (!f) throw new SessionError('closed', 'the host closed the control stream');
    try {
      return decodeHandshake(f);
    } catch {
      throw new SessionError('protocol', 'malformed handshake message');
    }
  }

  private async expect<T extends HandshakeMsg['type']>(
    type: T,
  ): Promise<Extract<HandshakeMsg, { type: T }>> {
    const m = await this.recvHs();
    if (isType(m, type)) return m;
    if (m.type === 'result' && m.reason !== null) throw rejection(m.reason);
    throw new SessionError('protocol', `expected ${type}, got ${m.type}`);
  }

  /** Runs the pairing handshake; resolves once the inner channel is up. */
  async pair(): Promise<void> {
    const timeout = this.o.handshakeTimeoutMs ?? HANDSHAKE_TIMEOUT_MS;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const deadline = new Promise<never>((_, reject) => {
      timer = setTimeout(() => {
        reject(new SessionError('timeout', 'handshake timed out'));
      }, timeout);
    });
    try {
      await Promise.race([this.handshake(), deadline]);
    } catch (e) {
      const code =
        e instanceof SessionError &&
        (e.failure === 'wrong-code' || e.failure === 'identity-mismatch')
          ? APP_CLOSE.pairingFailed
          : e instanceof SessionError && e.failure === 'timeout'
            ? APP_CLOSE.timeout
            : APP_CLOSE.protocol;
      this.close(code);
      throw e;
    } finally {
      clearTimeout(timer);
    }
  }

  private async handshake(): Promise<void> {
    const { wasm, identity } = this.o;
    const me = identity.publicKey;
    this.control = await this.o.transport.openBidi();
    await this.control.write(streamHeader(STREAM_KIND.control, 0));
    await this.sendHs({
      type: 'hello',
      min: PROTOCOL_VERSION_MIN,
      max: PROTOCOL_VERSION_MAX,
      intent: INTENT_PAIR,
    });
    await this.sendHs({ type: 'identify', id: me });

    const hello = await this.expect('hello');
    if (negotiateVersion(hello.min, hello.max) === null) {
      await this.sendHs({ type: 'result', reason: REJECT.versionMismatch });
      throw new SessionError('rejected', 'no common protocol version');
    }
    const host = (await this.expect('identify')).id;
    if (this.o.expectedHost && !equalBytes(host, this.o.expectedHost)) {
      throw new SessionError('identity-mismatch', 'the host key differs from the directory');
    }
    this.hostId = host;

    const entropy = this.o.entropy?.() ?? crypto.getRandomValues(new Uint8Array(32));
    let pairing;
    try {
      pairing = new wasm.Pairing(this.o.code, me, host, entropy);
    } catch {
      throw new SessionError('wrong-code', 'malformed code');
    } finally {
      entropy.fill(0);
    }
    await this.sendHs({ type: 'pairStart', msg: pairing.message() });
    const peerStart = await this.expect('pairStart');
    let paired: WasmPaired;
    try {
      paired = pairing.finish(peerStart.msg);
    } catch {
      throw new SessionError('wrong-code', 'pairing failed');
    }
    const tagC = paired.confirmation();
    await this.sendHs({ type: 'pairConfirm', tag: tagC });
    const tagH = (await this.expect('pairConfirm')).tag;
    if (!paired.verifyPeer(tagH)) {
      await this.sendHs({ type: 'result', reason: REJECT.wrongCode });
      throw new SessionError('wrong-code', 'confirmation mismatch');
    }
    const sig = await identity.sign(wasm.attestMessage(false, host, me, tagC, tagH));
    await this.sendHs({ type: 'attest', sig });
    const hostSig = (await this.expect('attest')).sig;
    if (!wasm.verifySignature(host, wasm.attestMessage(true, host, me, tagC, tagH), hostSig)) {
      await this.sendHs({ type: 'result', reason: REJECT.badSignature });
      throw new SessionError('identity-mismatch', 'host signature invalid');
    }
    await this.sendHs({ type: 'result', reason: null });

    this.channel = paired.channel();
    this.reassembler = new wasm.Reassembler();
    const sas = [...paired.sas()];
    this.o.events.onSas([sas[0] ?? 0, sas[1] ?? 0, sas[2] ?? 0, sas[3] ?? 0, sas[4] ?? 0]);
    this.o.transport.onDatagram((d) => {
      this.onDatagram(d);
    });
    void this.readControl();
    this.send({
      type: 'sessionRequest',
      requested: [PERMISSION.view, PERMISSION.input, PERMISSION.clipboard, PERMISSION.chat],
      controllerName: this.o.controllerName,
    });
  }

  private async readControl(): Promise<void> {
    const ctl = this.control;
    const ch = this.channel;
    if (!ctl || !ch) return;
    try {
      for (;;) {
        const f = await readFrame(ctl.incoming);
        if (!f) break;
        this.dispatch(ch.open(CONTROL_LANE, f));
      }
      this.close(APP_CLOSE.normal);
    } catch {
      this.close(APP_CLOSE.protocol);
    }
  }

  private dispatch(plain: Uint8Array): void {
    const m = decodeEnvelope(plain);
    if (m?.type === 'unknown') {
      const c = decodeClipboard(plain);
      if (c) {
        this.o.events.onClipboard?.(c);
        return;
      }
    }
    if (m) this.o.events.onMessage(m);
  }

  onDatagram(d: Uint8Array): void {
    const ch = this.channel;
    const r = this.reassembler;
    if (!ch || !r) return;
    this.counters.datagrams += 1;
    this.counters.bytes += d.length;
    const shard = ch.tryOpen(DATAGRAM_LANE, d);
    if (!shard) {
      this.counters.badDatagrams += 1;
      return;
    }
    const h = peekShard(shard);
    // Bandwidth feedback is about the video stream only.
    if (h?.kind === 0) this.arrivals.record(h, this.nowUs(), d.length);
    const f = r.push(shard);
    if (!f) return;
    const { frameId, keyframe } = f;
    if (f.audio === true) this.o.events.onAudio?.(frameId, f.takeData());
    else this.o.events.onVideo(frameId, keyframe, f.takeData());
  }

  /** Sends one envelope on the sealed Control stream. */
  send(m: Outgoing): void {
    const ctl = this.control;
    const ch = this.channel;
    if (!ctl || !ch || this.closed) return;
    void ctl.write(encodeFrame(ch.seal(CONTROL_LANE, encodeEnvelope(m)))).catch(() => undefined);
  }

  /**
   * Writes one sealed frame on a controller-opened stream, opening it (header
   * first) on first use. Sealing happens now so the counter order equals the
   * call order; writes are chained on the open promise in call order.
   */
  private stream(kind: number, lane: number): Promise<Duplex> {
    let s = this.streams.get(lane);
    if (s) return s;
    s = this.o.transport.openBidi().then(async (d) => {
      await d.write(streamHeader(kind, 0));
      if (lane === CLIPBOARD_LANE) void this.readClipboard(d);
      return d;
    });
    this.streams.set(lane, s);
    return s;
  }

  private sendOn(kind: number, lane: number, body: Uint8Array): void {
    const ch = this.channel;
    if (!ch || this.closed) return;
    const frame = encodeFrame(ch.seal(lane, body));
    const s = this.stream(kind, lane);
    void s.then((d) => d.write(frame)).catch(() => undefined);
  }

  /** Sends one input envelope on the sealed Input stream (opened on first use). */
  sendInput(m: Outgoing): void {
    this.sendOn(STREAM_KIND.input, INPUT_LANE, encodeEnvelope(m));
  }

  /**
   * Sends one clipboard envelope on the sealed Clipboard stream (kind 2,
   * lane `0x00020000`). The host answers on the same stream.
   */
  sendClipboard(m: ClipboardMessage): void {
    this.sendOn(STREAM_KIND.clipboard, CLIPBOARD_LANE, encodeClipboard(m));
  }

  /**
   * Opens the Clipboard stream without sending (header only) so the host
   * can push offers on it: browsers do not accept host-opened streams in v1.
   */
  openClipboard(): void {
    if (!this.channel || this.closed) return;
    void this.stream(STREAM_KIND.clipboard, CLIPBOARD_LANE).catch(() => undefined);
  }

  private async readClipboard(d: Duplex): Promise<void> {
    const ch = this.channel;
    if (!ch) return;
    for (;;) {
      let f: Uint8Array | null;
      try {
        f = await readFrame(d.incoming);
      } catch {
        // Reset by a host without clipboard support: not fatal. The lane
        // stays used (a lane is never reopened); later writes are dropped.
        return;
      }
      if (!f) return;
      let plain: Uint8Array;
      try {
        plain = ch.open(CLIPBOARD_LANE, f);
      } catch {
        // A frame that fails to open is fatal (gateway-session.md §4).
        this.close(APP_CLOSE.protocol);
        return;
      }
      const c = decodeClipboard(plain);
      if (c) this.o.events.onClipboard?.(c);
    }
  }

  reassemblyStats(): number[] {
    return this.reassembler ? [...this.reassembler.stats()] : [];
  }

  audioReassemblyStats(): number[] {
    const s = this.reassembler?.audioStats?.();
    return s ? [...s] : [];
  }

  /** Ends the session politely (SessionEnd, then close). */
  end(reason: number = END_REASON.closedByController, code: number = APP_CLOSE.normal): void {
    this.send({ type: 'sessionEnd', reason, message: '' });
    // Let the frame leave before the transport goes.
    setTimeout(() => {
      this.close(code);
    }, 50);
  }

  close(code: number): void {
    if (this.closed) return;
    this.closed = true;
    this.o.transport.close(code);
  }
}
