/**
 * The one seam between the shared UI and each platform (ARCHITECTURE §7).
 * `WebHost` (browser, WebTransport gateway) and `DesktopHost` (Tauri → native
 * Rust engine) implement it; screens only ever call `useHost()`.
 */

export type PlatformKind = 'web' | 'desktop' | 'android';

export interface PlatformInfo {
  kind: PlatformKind;
  os: string;
  version: string;
  /** Can this device be controlled (host role)? Browsers cannot. */
  canHost: boolean;
  canShare: boolean;
}

export interface KeyValueStorage {
  get(key: string): string | null;
  set(key: string, value: string): void;
  remove(key: string): void;
}

export interface OneTimeCode {
  code: string;
  /** Epoch ms. */
  issuedAt: number;
  expiresAt: number;
}

/** Five dictated words (D24): two locate the host, three are the secret. */
export interface Passphrase {
  /** Space-separated words in the requested language. Never log it. */
  words: string;
  /** Epoch ms when the locator expires (the engine renews it before). */
  expiresAt: number;
}

export type ConnectStage = 'locating' | 'securing' | 'awaiting-approval' | 'connected';
export type ConnectErrorKind =
  'offline' | 'wrong-code' | 'rejected' | 'timeout' | 'network-blocked' | 'sas-mismatch';
export type Route = 'direct' | 'relay';

export interface SessionStats {
  latencyMs: number;
  bitrateBps: number;
  fps: number;
  lossRatio: number;
  codec: 'AV1' | 'HEVC' | 'H.264';
  width: number;
  height: number;
  route: Route;
}

export interface RemoteDisplay {
  id: number;
  name: string;
  width: number;
  height: number;
  primary: boolean;
}

export type EngineEvent =
  | { type: 'stage'; sessionId: string; stage: ConnectStage; route?: Route }
  | { type: 'sas'; sessionId: string; emoji: [number, number, number, number, number] }
  | { type: 'error'; sessionId: string; error: ConnectErrorKind }
  | { type: 'stats'; sessionId: string; stats: SessionStats }
  | { type: 'displays'; sessionId: string; displays: RemoteDisplay[] }
  | { type: 'chat'; sessionId: string; from: 'remote' | 'local'; text: string; at: number }
  | { type: 'ended'; sessionId: string };

export interface ConnectHandle {
  sessionId: string;
  cancel(): void;
}

export interface SessionEngine {
  getMyId(): Promise<string>;
  getCode(): Promise<OneTimeCode>;
  regenerateCode(): Promise<OneTimeCode>;
  /**
   * Host only (D24): show a passphrase in `lang` (`en`, `ro`), or `null` to
   * stop. Resolves with the current phrase once the server allocated it;
   * absent where this device cannot host or has no rendezvous server.
   */
  setPassphrase?(lang: string | null): Promise<Passphrase | null>;
  /** Host only: the passphrase on screen now (re-drawn after each use). */
  getPassphrase?(): Promise<Passphrase | null>;
  /** Whether `connect` accepts a five-word passphrase as `id`. */
  readonly supportsPassphrase?: boolean;
  /**
   * Starts the connect flow; `id` may also be a five-word passphrase (then
   * `code` is ignored). Progress and errors arrive through `onEvent`.
   */
  connect(id: string, code: string): Promise<ConnectHandle>;
  confirmSas(sessionId: string, matches: boolean): Promise<void>;
  sendKeys(sessionId: string, combo: SpecialKey): Promise<void>;
  sendChat(sessionId: string, text: string): Promise<void>;
  endSession(sessionId: string): Promise<void>;
  onEvent(listener: (event: EngineEvent) => void): () => void;
}

export type SpecialKey = 'ctrl-alt-del' | 'win' | 'alt-tab' | 'print-screen' | 'lock';

export interface ScrinHost {
  readonly platform: PlatformInfo;
  readonly storage: KeyValueStorage;
  readonly engine: SessionEngine;
  writeClipboard(text: string): Promise<void>;
  openExternal(url: string): Promise<void>;
  notify(title: string, body: string): Promise<void>;
  share?(data: { title: string; text: string }): Promise<void>;
}
