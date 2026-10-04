/**
 * `SessionEngine` for the browser against a real scrin server
 * (`/v1/info`, `/v1/resolve`, WebTransport/WebSocket gateway). The heavy part
 * (wasm, codecs) is loaded on the first `connect`, so the home screen stays
 * light. Browsers cannot host, so `getMyId`/`getCode` return placeholders.
 */
import type { Incoming } from '@scrin/protocol';

import type {
  ConnectErrorKind,
  ConnectHandle,
  EngineEvent,
  OneTimeCode,
  RemoteDisplay,
  SessionEngine,
  SpecialKey,
} from '../../platform';
import { retry } from './backoff';
import { SPECIAL_KEYS } from './hid';
import { getSession, registerSession, unregisterSession, type LiveSession } from './registry';
import type { WebRuntime } from './runtime';
import type { SessionFailure } from './session';
import type { GatewayEndpoints, GatewayTransport } from './transport';

export interface WebEngineOptions {
  /** Base URL of the scrin server, e.g. `https://scrin.example:8443`. */
  serverUrl: string;
  /** Name shown in the host's interstitial. */
  deviceName?: string;
  fetch?: typeof fetch;
  /** Loads the session runtime; defaults to the lazy chunk. */
  loadRuntime?: () => Promise<WebRuntime>;
  /** Dial attempts (WebTransport, then WebSocket fallbacks). */
  dialAttempts?: number;
}

interface ServerInfo {
  wt_cert_sha256: string | undefined;
  gateway: boolean;
}

const FAILURE: Record<SessionFailure, ConnectErrorKind> = {
  'wrong-code': 'wrong-code',
  rejected: 'rejected',
  timeout: 'timeout',
  protocol: 'network-blocked',
  'identity-mismatch': 'sas-mismatch',
  closed: 'offline',
};

/** `scrin.v1.SessionRejectReason.TIMEOUT`. */
const REJECT_TIMEOUT = 2;
/** `scrin.v1.SessionEndReason.REPORTED` and application close code 0x03. */
const END_REPORTED = 7;
const CLOSE_SAS_MISMATCH = 0x03;

class HttpError extends Error {
  constructor(readonly status: number) {
    super(`HTTP ${status}`);
  }
}

const hexToBytes = (h: string) =>
  Uint8Array.from(h.match(/../g) ?? [], (x) => Number.parseInt(x, 16));

function parseInfo(v: unknown): ServerInfo | null {
  if (typeof v !== 'object' || v === null || !('gateway' in v)) return null;
  const cert =
    'wt_cert_sha256' in v && typeof v.wt_cert_sha256 === 'string' ? v.wt_cert_sha256 : undefined;
  return { gateway: v.gateway === true, wt_cert_sha256: cert };
}

function parseDevicePub(v: unknown): Uint8Array | undefined {
  if (typeof v !== 'object' || v === null || !('device_pub' in v)) return undefined;
  const h = v.device_pub;
  return typeof h === 'string' && /^[0-9a-f]{64}$/i.test(h) ? hexToBytes(h) : undefined;
}

function placeholderCode(): OneTimeCode {
  const now = Date.now();
  return { code: '--------', issuedAt: now, expiresAt: now + 3_600_000 };
}

/** Read through a call so narrowing does not assume the flag is unchanged across awaits. */
const isCancelled = (t: { cancelled: boolean }): boolean => t.cancelled;

export function createWebEngine(opts: WebEngineOptions): SessionEngine {
  const f = opts.fetch ?? globalThis.fetch.bind(globalThis);
  const base = opts.serverUrl.replace(/\/+$/, '');
  const listeners = new Set<(e: EngineEvent) => void>();
  const emit = (e: EngineEvent) => {
    for (const l of listeners) l(e);
  };
  let runtime: Promise<WebRuntime> | null = null;
  const loadRuntime = () =>
    (runtime ??= (opts.loadRuntime ?? (async () => (await import('./runtime')).loadRuntime()))());
  let seq = 0;

  const getJson = async (path: string): Promise<unknown> => {
    const r = await f(`${base}${path}`, { headers: { accept: 'application/json' } });
    if (!r.ok) throw new HttpError(r.status);
    return r.json();
  };

  const dial = (rt: WebRuntime, ep: GatewayEndpoints, token: { cancelled: boolean }) =>
    retry<GatewayTransport>(
      async (attempt) => {
        if (attempt === 0) {
          try {
            return await rt.connectWebTransport(ep);
          } catch {
            // UDP blocked or no WebTransport: fall through to the WebSocket.
          }
        }
        return rt.connectWebSocket(ep);
      },
      {
        attempts: opts.dialAttempts ?? 3,
        signal: {
          get aborted() {
            return token.cancelled;
          },
        },
      },
    );

  const run = async (
    sessionId: string,
    id: string,
    code: string,
    token: { cancelled: boolean; accepted: boolean },
  ) => {
    emit({ type: 'stage', sessionId, stage: 'locating' });
    const rt = await loadRuntime();
    let info: ServerInfo | null;
    try {
      info = parseInfo(await getJson('/v1/info'));
    } catch {
      info = null;
    }
    if (!info?.gateway) {
      emit({ type: 'error', sessionId, error: 'network-blocked' });
      return;
    }
    // Best effort: pins the host key to the directory entry when available.
    let expectedHost: Uint8Array | undefined;
    try {
      expectedHost = parseDevicePub(await getJson(`/v1/resolve/${encodeURIComponent(id)}`));
    } catch (e) {
      if (e instanceof HttpError && e.status === 404) {
        emit({ type: 'error', sessionId, error: 'offline' });
        return;
      }
    }
    if (token.cancelled) return;
    let transport: GatewayTransport;
    try {
      transport = await dial(
        rt,
        { server: base, scrinId: id, certSha256: info.wt_cert_sha256 },
        token,
      );
    } catch {
      emit({ type: 'error', sessionId, error: 'offline' });
      return;
    }
    if (isCancelled(token)) {
      transport.close(0);
      return;
    }
    // Every gateway session is relayed through the server.
    const route = 'relay' as const;
    emit({ type: 'stage', sessionId, stage: 'securing', route });
    let accepted = false;
    let live: LiveSession | null = null;
    const session = new rt.GatewaySession({
      transport,
      wasm: rt.wasm,
      identity: rt.identity,
      code,
      expectedHost,
      controllerName: opts.deviceName ?? 'Browser',
      events: {
        onSas: (emoji) => {
          emit({ type: 'sas', sessionId, emoji });
          emit({ type: 'stage', sessionId, stage: 'awaiting-approval', route });
        },
        onMessage: (m: Incoming) => {
          if (m.type === 'sessionAccept') {
            accepted = true;
            token.accepted = true;
            if (live) live.granted = m.granted;
            const displays: RemoteDisplay[] = m.displays.map((d) => ({
              id: d.id,
              name: d.name,
              width: d.width,
              height: d.height,
              primary: d.primary,
            }));
            emit({ type: 'stage', sessionId, stage: 'connected', route });
            emit({ type: 'displays', sessionId, displays });
          } else if (m.type === 'sessionReject') {
            emit({
              type: 'error',
              sessionId,
              error: m.reason === REJECT_TIMEOUT ? 'timeout' : 'rejected',
            });
          } else if (m.type === 'chat') {
            emit({ type: 'chat', sessionId, from: 'remote', text: m.text, at: Date.now() });
          } else if (m.type === 'videoConfig' && live) {
            live.videoConfig = m;
          } else if (m.type === 'permissionsUpdate' && live) {
            live.granted = m.granted;
          }
          if (live) for (const l of live.onMessage) l(m);
        },
        onVideo: (frameId, keyframe, data) => {
          if (live) for (const l of live.onVideo) l(frameId, keyframe, data);
        },
        onAudio: (frameId, data) => {
          if (live) for (const l of live.onAudio) l(frameId, data);
        },
        onClipboard: (m) => {
          if (live) for (const l of live.onClipboard) l(m);
        },
        onClosed: () => {
          unregisterSession(sessionId);
          if (accepted) emit({ type: 'ended', sessionId });
        },
      },
    });
    live = {
      id: sessionId,
      session,
      videoConfig: null,
      granted: [],
      onMessage: new Set(),
      onVideo: new Set(),
      onAudio: new Set(),
      onClipboard: new Set(),
      emit,
    };
    registerSession(live);
    try {
      await session.pair();
    } catch (e) {
      unregisterSession(sessionId);
      emit({
        type: 'error',
        sessionId,
        error: rt.isSessionError(e) ? FAILURE[e.failure] : 'offline',
      });
      return;
    }
    if (isCancelled(token)) session.end();
  };

  return {
    getMyId: () => Promise.resolve('000000000'),
    getCode: () => Promise.resolve(placeholderCode()),
    regenerateCode: () => Promise.resolve(placeholderCode()),
    async connect(id: string, code: string): Promise<ConnectHandle> {
      seq += 1;
      const sessionId = `w${seq}-${id}`;
      const token = { cancelled: false, accepted: false };
      await Promise.resolve();
      void run(sessionId, id.replace(/\D/g, ''), code, token).catch(() => {
        emit({ type: 'error', sessionId, error: 'network-blocked' });
      });
      return {
        sessionId,
        // Aborts the attempt only. Once the host accepted, the connect screen
        // unmounting (navigation to the session) must not end the session;
        // `endSession` does that (same contract as the mock engine).
        cancel: () => {
          if (token.accepted) return;
          token.cancelled = true;
          getSession(sessionId)?.session.end();
        },
      };
    },
    async confirmSas(sessionId: string, matches: boolean) {
      if (!matches) {
        getSession(sessionId)?.session.end(END_REPORTED, CLOSE_SAS_MISMATCH);
        emit({ type: 'error', sessionId, error: 'sas-mismatch' });
      }
      await Promise.resolve();
    },
    async sendKeys(sessionId: string, combo: SpecialKey) {
      const s = getSession(sessionId)?.session;
      for (const k of SPECIAL_KEYS[combo]) {
        s?.sendInput({
          type: 'keyEvent',
          hidUsage: k.usage,
          down: k.down,
          modifiers: 0,
          repeat: false,
        });
      }
      await Promise.resolve();
    },
    async sendChat(sessionId: string, text: string) {
      const at = Date.now();
      getSession(sessionId)?.session.send({ type: 'chat', id: at, text, sentUnixMs: at });
      emit({ type: 'chat', sessionId, from: 'local', text, at });
      await Promise.resolve();
    },
    async endSession(sessionId: string) {
      const s = getSession(sessionId);
      if (s) s.session.end();
      else emit({ type: 'ended', sessionId });
      await Promise.resolve();
    },
    onEvent(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}
