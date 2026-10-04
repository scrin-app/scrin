import type {
  ConnectErrorKind,
  ConnectHandle,
  EngineEvent,
  KeyValueStorage,
  OneTimeCode,
  RemoteDisplay,
  ScrinHost,
  SessionEngine,
  SpecialKey,
} from '../platform';
import { createLazyWebEngine } from '../platform/web/lazy-engine';

const CODE_ALPHABET = 'ABCDEFGHJKMNPQRSTVWXYZ23456789';
const CODE_TTL_MS = 10 * 60 * 1000;

function randomInt(max: number): number {
  const buf = new Uint32Array(1);
  crypto.getRandomValues(buf);
  return (buf[0] ?? 0) % max;
}

function makeCode(now: number): OneTimeCode {
  let code = '';
  for (let i = 0; i < 8; i += 1) code += CODE_ALPHABET[randomInt(CODE_ALPHABET.length)] ?? 'A';
  return { code, issuedAt: now, expiresAt: now + CODE_TTL_MS };
}

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

function failureFor(id: string, code: string): ConnectErrorKind | null {
  if (id.startsWith('000')) return 'offline';
  const c = code.toUpperCase();
  if (c.startsWith('W')) return 'wrong-code';
  if (c.startsWith('R')) return 'rejected';
  if (c.startsWith('T')) return 'timeout';
  if (c.startsWith('B')) return 'network-blocked';
  return null;
}

/**
 * `localStorage` with a memory fallback (private mode, sandboxed iframes, tests).
 */
export function createWebStorage(): KeyValueStorage {
  const memory = new Map<string, string>();
  const ls = (() => {
    try {
      const s = globalThis.localStorage;
      const probe = '__scrin_probe__';
      s.setItem(probe, probe);
      s.removeItem(probe);
      return s;
    } catch {
      return null;
    }
  })();
  return {
    get: (k) => (ls ? ls.getItem(k) : (memory.get(k) ?? null)),
    set: (k, v) => {
      if (ls) ls.setItem(k, v);
      else memory.set(k, v);
    },
    remove: (k) => {
      if (ls) ls.removeItem(k);
      else memory.delete(k);
    },
  };
}

export interface MockEngineOptions {
  /** Multiplies every simulated delay; 0 makes tests instant. */
  latencyScale?: number;
  myId?: string;
}

/**
 * A realistic stand-in for the session engine so the UI can be built before
 * the WebTransport client exists. Behaviour is driven by the code typed in, so
 * every error screen is reachable by hand:
 *
 *  - ID starting with `000` → offline
 *  - code `WRONGXXX`-like (starts with `W`) → wrong code
 *  - code starting with `R` → rejected
 *  - code starting with `T` → timeout
 *  - code starting with `B` → network blocked
 *  - anything else → success via a relay or direct route
 */
export function createMockEngine(opts: MockEngineOptions = {}): SessionEngine {
  const scale = opts.latencyScale ?? 1;
  const myId = opts.myId ?? String(100_000_000 + randomInt(899_999_999));
  let current = makeCode(Date.now());
  const listeners = new Set<(e: EngineEvent) => void>();
  const timers = new Map<string, ReturnType<typeof setInterval>>();
  let seq = 0;

  const emit = (e: EngineEvent) => {
    for (const l of listeners) l(e);
  };
  const wait = (ms: number) => sleep(ms * scale);

  const displays: RemoteDisplay[] = [
    { id: 1, name: 'DELL U3423WE', width: 3440, height: 1440, primary: true },
    { id: 2, name: 'Built-in display', width: 2560, height: 1600, primary: false },
  ];

  const startStats = (sessionId: string, route: 'direct' | 'relay') => {
    let t = 0;
    const timer = setInterval(() => {
      t += 1;
      emit({
        type: 'stats',
        sessionId,
        stats: {
          latencyMs: Math.round(
            (route === 'direct' ? 14 : 38) + Math.sin(t / 3) * 4 + randomInt(4),
          ),
          bitrateBps: Math.round(18_000_000 + Math.sin(t / 5) * 4_000_000),
          fps: 60 - randomInt(3),
          lossRatio: randomInt(10) / 1000,
          codec: 'AV1',
          width: 3440,
          height: 1440,
          route,
        },
      });
    }, 1000);
    timers.set(sessionId, timer);
  };

  return {
    async getMyId() {
      await wait(250);
      return myId;
    },
    async getCode() {
      await wait(150);
      if (current.expiresAt <= Date.now()) current = makeCode(Date.now());
      return current;
    },
    async regenerateCode() {
      await wait(300);
      current = makeCode(Date.now());
      return current;
    },
    async connect(id: string, code: string): Promise<ConnectHandle> {
      seq += 1;
      const sessionId = `s${seq}-${id}`;
      // An object, not a `let`: the flag is flipped from outside while the
      // async flow is suspended, which local narrowing cannot see.
      const token = { cancelled: false };
      const live = () => !token.cancelled;
      const failure = failureFor(id, code);
      const route = randomInt(2) === 0 ? 'direct' : 'relay';
      // Return the handle before the first event, like the real engine.
      await wait(40);
      void (async () => {
        await sleep(0);
        emit({ type: 'stage', sessionId, stage: 'locating' });
        await wait(900);
        if (!live()) return;
        if (failure === 'offline' || failure === 'network-blocked') {
          emit({ type: 'error', sessionId, error: failure });
          return;
        }
        emit({ type: 'stage', sessionId, stage: 'securing', route });
        await wait(1100);
        if (!live()) return;
        if (failure === 'wrong-code') {
          emit({ type: 'error', sessionId, error: failure });
          return;
        }
        const emoji: [number, number, number, number, number] = [
          randomInt(64),
          randomInt(64),
          randomInt(64),
          randomInt(64),
          randomInt(64),
        ];
        emit({ type: 'sas', sessionId, emoji });
        emit({ type: 'stage', sessionId, stage: 'awaiting-approval', route });
        await wait(failure === 'timeout' ? 4000 : 1800);
        if (!live()) return;
        if (failure) {
          emit({ type: 'error', sessionId, error: failure });
          return;
        }
        emit({ type: 'stage', sessionId, stage: 'connected', route });
        emit({ type: 'displays', sessionId, displays });
        startStats(sessionId, route);
      })();
      return {
        sessionId,
        cancel: () => {
          token.cancelled = true;
        },
      };
    },
    async confirmSas(sessionId: string, matches: boolean) {
      await wait(50);
      if (!matches) emit({ type: 'error', sessionId, error: 'sas-mismatch' });
    },
    async sendKeys(_sessionId: string, _combo: SpecialKey) {
      await wait(80);
    },
    async sendChat(sessionId: string, text: string) {
      emit({ type: 'chat', sessionId, from: 'local', text, at: Date.now() });
      await wait(1200);
      emit({
        type: 'chat',
        sessionId,
        from: 'remote',
        text: `👍 ${text.slice(0, 40)}`,
        at: Date.now(),
      });
    },
    async endSession(sessionId: string) {
      const timer = timers.get(sessionId);
      if (timer) clearInterval(timer);
      timers.delete(sessionId);
      await wait(100);
      emit({ type: 'ended', sessionId });
    },
    onEvent(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

function detectOs(): string {
  const ua = typeof navigator === 'undefined' ? '' : navigator.userAgent;
  if (/Windows/i.test(ua)) return 'Windows';
  if (/Android/i.test(ua)) return 'Android';
  if (/iPhone|iPad/i.test(ua)) return 'iOS';
  if (/Mac OS X/i.test(ua)) return 'macOS';
  if (/Linux/i.test(ua)) return 'Linux';
  return 'Unknown';
}

export interface WebHostOptions extends MockEngineOptions {
  version?: string;
  /**
   * Base URL of a scrin server with the browser gateway. When set, sessions
   * run for real (WebTransport/WebSocket + wasm + WebCodecs); when absent the
   * mock engine drives the UI (tests, demo). Defaults to
   * {@link configuredServerUrl}.
   */
  serverUrl?: string | undefined;
  deviceName?: string;
}

/** Storage key of the configured scrin server URL. */
const SERVER_URL_KEY = 'scrin.server';

function validServerUrl(v: string | null | undefined): string | null {
  if (!v) return null;
  try {
    const u = new URL(v);
    return u.protocol === 'https:' || u.protocol === 'http:' ? u.origin : null;
  } catch {
    return null;
  }
}

/**
 * The scrin server this SPA talks to, from (in order) a `?server=` query
 * parameter (remembered), `localStorage['scrin.server']`, or
 * `<meta name="scrin-server" content="…">`. `null` = demo mode (mock engine).
 */
function configuredServerUrl(storage: KeyValueStorage): string | null {
  if (typeof location !== 'undefined') {
    const q = new URLSearchParams(location.search).get('server');
    if (q === '' || q === 'demo') {
      storage.remove(SERVER_URL_KEY);
      return null;
    }
    const fromQuery = validServerUrl(q);
    if (fromQuery) {
      storage.set(SERVER_URL_KEY, fromQuery);
      return fromQuery;
    }
  }
  const stored = validServerUrl(storage.get(SERVER_URL_KEY));
  if (stored) return stored;
  if (typeof document === 'undefined') return null;
  return validServerUrl(
    document.querySelector('meta[name="scrin-server"]')?.getAttribute('content'),
  );
}

export function createWebHost(opts: WebHostOptions = {}): ScrinHost {
  const storage = createWebStorage();
  const canShare = typeof navigator !== 'undefined' && typeof navigator.share === 'function';
  const serverUrl = 'serverUrl' in opts ? opts.serverUrl : configuredServerUrl(storage);
  const engine = serverUrl
    ? createLazyWebEngine({
        serverUrl,
        ...(opts.deviceName ? { deviceName: opts.deviceName } : {}),
      })
    : createMockEngine(opts);
  return {
    platform: {
      kind: 'web',
      os: detectOs(),
      version: opts.version ?? '0.1.0',
      canHost: false,
      canShare,
    },
    storage,
    engine,
    async writeClipboard(text) {
      await navigator.clipboard.writeText(text);
    },
    async openExternal(url) {
      window.open(url, '_blank', 'noopener,noreferrer');
      await Promise.resolve();
    },
    async notify(title, body) {
      if (typeof Notification === 'undefined') return;
      const permission =
        Notification.permission === 'default'
          ? await Notification.requestPermission()
          : Notification.permission;
      if (permission === 'granted') {
        const notification = new Notification(title, { body });
        notification.addEventListener('click', () => window.focus());
      }
    },
    ...(canShare
      ? {
          async share(data: { title: string; text: string }) {
            await navigator.share(data);
          },
        }
      : {}),
  };
}
