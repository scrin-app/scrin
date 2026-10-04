import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import { createWebStorage } from '../host/web-host';
import type {
  ConnectErrorKind,
  ConnectHandle,
  ConnectStage,
  EngineEvent,
  OneTimeCode,
  Passphrase,
  ScrinHost,
  SessionEngine,
  SpecialKey,
} from '../platform';

/**
 * Status as `scrin_status` returns it (`scrin_engine::Status`, camelCase).
 */
export interface DesktopStatus {
  deviceId: string;
  fingerprint: string;
  scrinId: string;
  ticket: string;
  code: string;
  codeIssuedAt: number;
  codeExpiresAt: number;
  /** Five-word passphrase or `''` (D24). */
  phrase: string;
  phraseExpiresAt: number;
  online: boolean;
  backend: string;
}

type Sas = [number, number, number, number, number];

/** `scrin_engine::Event` as serialised on `scrin://event`. */
export type NativeEvent =
  | ({ type: 'status' } & DesktopStatus)
  | {
      type: 'incomingRequest';
      session: string;
      peer: string;
      fingerprint: string;
      kind: string;
      sas: Sas | null;
      requested: string[];
      allowed: string[];
      acceptEnabledAt: number;
      expiresAt: number;
    }
  | { type: 'sas'; session: string; emoji: Sas }
  | {
      type: 'stateChanged';
      session: string;
      role: 'host' | 'controller';
      state: 'connecting' | 'pairing' | 'awaitingAccept' | 'active' | 'ended';
      peer: string | null;
      reason: string | null;
    }
  | { type: 'permissionsChanged'; session: string; granted: string[] }
  | { type: 'permissionRequested'; session: string; permission: string }
  | {
      type: 'stats';
      session: string;
      stats: {
        rttMs: number;
        fps: number;
        bitrateBps: number;
        loss: number;
        decodeMs: number;
        width: number;
        height: number;
        framesTotal: number;
      };
    }
  | { type: 'error'; session: string | null; code: string; message: string };

/** Host-role events the shared screens do not model yet. */
export type HostRoleEvent = Extract<
  NativeEvent,
  { type: 'incomingRequest' | 'permissionRequested' | 'status' }
>;

/** The `invoke`/`listen` surface DesktopHost needs; injectable for tests. */
export interface TauriBridge {
  invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown>;
  listen(event: string, handler: (payload: unknown) => void): Promise<() => void>;
}

const defaultBridge: TauriBridge = {
  invoke: (cmd, args) => invoke(cmd, args),
  listen: async (event, handler) => listen(event, (e) => handler(e.payload)),
};

function isNativeEvent(v: unknown): v is NativeEvent {
  return typeof v === 'object' && v !== null && 'type' in v && typeof v.type === 'string';
}

function isStatus(v: unknown): v is DesktopStatus {
  return (
    typeof v === 'object' &&
    v !== null &&
    'scrinId' in v &&
    typeof v.scrinId === 'string' &&
    'code' in v &&
    typeof v.code === 'string' &&
    'codeExpiresAt' in v &&
    typeof v.codeExpiresAt === 'number'
  );
}

async function invokeStatus(
  bridge: TauriBridge,
  cmd: string,
  args?: Record<string, unknown>,
): Promise<DesktopStatus> {
  const v = await bridge.invoke(cmd, args);
  if (!isStatus(v)) throw new Error(`${cmd}: unexpected reply`);
  return v;
}

async function invokeSession(bridge: TauriBridge, args: Record<string, unknown>): Promise<string> {
  const v = await bridge.invoke('scrin_connect', args);
  if (typeof v === 'object' && v !== null && 'sessionId' in v && typeof v.sessionId === 'string') {
    return v.sessionId;
  }
  throw new Error('scrin_connect: unexpected reply');
}

const STAGE: Partial<Record<string, ConnectStage>> = {
  connecting: 'locating',
  pairing: 'securing',
  awaitingAccept: 'awaiting-approval',
  active: 'connected',
};

const ERROR: Partial<Record<string, ConnectErrorKind>> = {
  offline: 'offline',
  'wrong-code': 'wrong-code',
  'code-unavailable': 'wrong-code',
  rejected: 'rejected',
  busy: 'rejected',
  untrusted: 'rejected',
  timeout: 'timeout',
  'sas-mismatch': 'sas-mismatch',
  version: 'network-blocked',
};

/** Reasons that mean the attempt failed rather than a session ending. */
const FAILED_REASONS = new Set(['pairing-failed', 'connect-failed', 'timeout']);

function toCode(s: DesktopStatus): OneTimeCode {
  return { code: s.code.replace('-', ''), issuedAt: s.codeIssuedAt, expiresAt: s.codeExpiresAt };
}

function toPhrase(s: DesktopStatus): Passphrase | null {
  return s.phrase ? { words: s.phrase, expiresAt: s.phraseExpiresAt } : null;
}

/** How long `setPassphrase` waits for the server to allocate a locator. */
const PHRASE_WAIT_MS = 10_000;

/**
 * Maps native events onto the UI's `EngineEvent` contract. Controller
 * sessions map one-to-one; host-role events go to `onHostEvent`.
 */
export function mapNativeEvent(e: NativeEvent): EngineEvent[] {
  switch (e.type) {
    case 'sas':
      return [{ type: 'sas', sessionId: e.session, emoji: e.emoji }];
    case 'stateChanged': {
      if (e.role !== 'controller') return [];
      if (e.state === 'ended') return [{ type: 'ended', sessionId: e.session }];
      const stage = STAGE[e.state];
      return stage ? [{ type: 'stage', sessionId: e.session, stage }] : [];
    }
    case 'stats':
      return [
        {
          type: 'stats',
          sessionId: e.session,
          stats: {
            latencyMs: Math.round(e.stats.rttMs),
            bitrateBps: e.stats.bitrateBps,
            fps: Math.round(e.stats.fps),
            lossRatio: e.stats.loss,
            codec: 'H.264',
            width: e.stats.width,
            height: e.stats.height,
            route: 'direct',
          },
        },
      ];
    case 'error': {
      const kind = ERROR[e.code];
      return e.session && kind ? [{ type: 'error', sessionId: e.session, error: kind }] : [];
    }
    default:
      return [];
  }
}

export interface DesktopHostOptions {
  version?: string;
  bridge?: TauriBridge;
  /** Host-role events (incoming request, status) until screens consume them. */
  onHostEvent?: (e: HostRoleEvent) => void;
}

function createDesktopEngine(bridge: TauriBridge, opts: DesktopHostOptions): SessionEngine {
  const listeners = new Set<(e: EngineEvent) => void>();
  /** Sessions whose failure already produced an `error`; their `ended` is not a session end. */
  const failed = new Set<string>();
  const emit = (e: EngineEvent) => {
    for (const l of listeners) l(e);
  };
  /** Resolvers waiting for the first status that carries a phrase. */
  const phraseWaiters = new Set<(p: Passphrase) => void>();

  void bridge.listen('scrin://event', (native) => {
    if (!isNativeEvent(native)) return;
    if (
      native.type === 'incomingRequest' ||
      native.type === 'permissionRequested' ||
      native.type === 'status'
    ) {
      if (native.type === 'status') {
        const p = toPhrase(native);
        if (p) {
          for (const w of phraseWaiters) w(p);
          phraseWaiters.clear();
        }
      }
      opts.onHostEvent?.(native);
      return;
    }
    if (native.type === 'error' && native.session) failed.add(native.session);
    if (
      native.type === 'stateChanged' &&
      native.state === 'ended' &&
      native.role === 'controller' &&
      (failed.has(native.session) || FAILED_REASONS.has(native.reason ?? ''))
    ) {
      // The connect screen shows the error; do not also navigate away.
      if (!failed.has(native.session)) {
        emit({ type: 'error', sessionId: native.session, error: 'timeout' });
      }
      failed.delete(native.session);
      return;
    }
    for (const e of mapNativeEvent(native)) emit(e);
  });

  let myId: string | null = null;
  const status = async () => {
    const s = await invokeStatus(bridge, 'scrin_status');
    myId = s.scrinId;
    return s;
  };

  return {
    async getMyId() {
      return myId ?? (await status()).scrinId;
    },
    async getCode() {
      return toCode(await status());
    },
    async regenerateCode() {
      return toCode(await invokeStatus(bridge, 'scrin_regenerate_code'));
    },
    async setPassphrase(lang: string | null) {
      if (lang === null) {
        await bridge.invoke('scrin_disable_phrase');
        return null;
      }
      // Enabling always asks the server for a new locator: wait for the
      // status that carries the new words.
      const arrived = new Promise<Passphrase | null>((resolve) => {
        const done = (p: Passphrase) => {
          clearTimeout(timer);
          resolve(p);
        };
        const timer = setTimeout(() => {
          phraseWaiters.delete(done);
          resolve(null);
        }, PHRASE_WAIT_MS);
        phraseWaiters.add(done);
      });
      await invokeStatus(bridge, 'scrin_enable_phrase', { lang });
      return arrived;
    },
    async getPassphrase() {
      return toPhrase(await status());
    },
    supportsPassphrase: true,
    async connect(id: string, code: string): Promise<ConnectHandle> {
      const sessionId = await invokeSession(bridge, { target: id, code });
      return {
        sessionId,
        cancel: () => {
          void bridge.invoke('scrin_end_session', { session: sessionId });
        },
      };
    },
    async confirmSas(sessionId: string, matches: boolean) {
      await bridge.invoke('scrin_confirm_sas', { session: sessionId, matches });
    },
    async sendKeys(sessionId: string, combo: SpecialKey) {
      await bridge.invoke('scrin_send_keys', { session: sessionId, combo });
    },
    async sendChat(sessionId: string, text: string) {
      // Chat travels with the chat stream (not in the engine yet); echo locally.
      emit({ type: 'chat', sessionId, from: 'local', text, at: Date.now() });
      await Promise.resolve();
    },
    async endSession(sessionId: string) {
      await bridge.invoke('scrin_end_session', { session: sessionId });
    },
    onEvent(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

/** Rectangle of the remote-screen canvas in CSS px, or `null` to hide video. */
export function setVideoRect(
  rect: { x: number; y: number; width: number; height: number } | null,
  bridge: TauriBridge = defaultBridge,
): Promise<void> {
  return bridge.invoke('scrin_video_rect', { rect }).then(() => undefined);
}

/**
 * Keeps the native video surface on top of the element marked
 * `data-scrin-video` (the session screen's canvas), and hides it when that
 * element is gone. Returns a stop function.
 */
export function trackVideoSurface(bridge: TauriBridge = defaultBridge): () => void {
  if (typeof document === 'undefined' || typeof ResizeObserver === 'undefined') {
    return () => undefined;
  }
  let el: Element | null = null;
  let last = '';
  const push = () => {
    const r = el?.getBoundingClientRect();
    const rect = r && r.width > 0 ? { x: r.x, y: r.y, width: r.width, height: r.height } : null;
    const key = JSON.stringify(rect);
    if (key !== last) {
      last = key;
      void setVideoRect(rect, bridge);
    }
  };
  const resize = new ResizeObserver(push);
  const attach = () => {
    const next = document.querySelector('[data-scrin-video]');
    if (next === el) return;
    if (el) resize.unobserve(el);
    el = next;
    if (el) resize.observe(el);
    push();
  };
  const mutations = new MutationObserver(attach);
  mutations.observe(document.body, { childList: true, subtree: true });
  window.addEventListener('resize', push);
  attach();
  return () => {
    mutations.disconnect();
    resize.disconnect();
    window.removeEventListener('resize', push);
    void setVideoRect(null, bridge);
  };
}

/** `scrin://connect/123456789` → `/connect/123456789`; anything else → `null`. */
export function deepLinkPath(url: string): string | null {
  const m = /^scrin:\/\/connect\/(\d{9})\/?$/.exec(url.trim());
  return m ? `/connect/${m[1] ?? ''}` : null;
}

function followDeepLinks(bridge: TauriBridge) {
  void bridge.listen('scrin://deep-link', (url) => {
    if (typeof url !== 'string') return;
    const path = deepLinkPath(url);
    if (!path || typeof window === 'undefined') return;
    window.history.pushState({}, '', path);
    window.dispatchEvent(new PopStateEvent('popstate'));
  });
}

/** `ScrinHost` backed by the native engine through Tauri commands and events. */
export function createDesktopHost(opts: DesktopHostOptions = {}): ScrinHost {
  const bridge = opts.bridge ?? defaultBridge;
  if (!opts.bridge) {
    trackVideoSurface(bridge);
    followDeepLinks(bridge);
  }
  return {
    platform: {
      kind: 'desktop',
      os: 'Windows',
      version: opts.version ?? '0.1.0',
      canHost: true,
      canShare: false,
    },
    storage: createWebStorage(),
    engine: createDesktopEngine(bridge, opts),
    async writeClipboard(text) {
      const { writeText } = await import('@tauri-apps/plugin-clipboard-manager');
      await writeText(text);
    },
    async openExternal(url) {
      if (!url.startsWith('https://')) return;
      const { openUrl } = await import('@tauri-apps/plugin-opener');
      await openUrl(url);
    },
    async notify(title, body) {
      const n = await import('@tauri-apps/plugin-notification');
      let granted = await n.isPermissionGranted();
      if (!granted) granted = (await n.requestPermission()) === 'granted';
      if (granted) n.sendNotification({ title, body });
    },
  };
}
