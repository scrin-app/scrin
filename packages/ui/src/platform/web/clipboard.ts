/**
 * Clipboard sync for the browser client (WB-004), text only.
 *
 * Privacy rules (the policy the tests pin):
 * - Nothing moves unless the host granted `PERMISSION_CLIPBOARD`.
 * - Browser → host: the local clipboard is read only (a) on focus, and only
 *   when the `clipboard-read` permission is already `granted` (never
 *   prompts by itself), or (b) from a `paste` event the user caused.
 * - Host → browser: written with `navigator.clipboard.writeText` when the
 *   host sends text; while the page is unfocused the write waits for focus
 *   (browsers reject background writes).
 * - Text only, at most {@link MAX_TEXT_BYTES}; other formats are ignored.
 * - Echo suppression: text that just came from the other side is not offered back.
 *
 * Wire: lazy `ClipboardOffer` → `ClipboardRequest` → `ClipboardData`
 * (`proto/scrin/v1/transfer.proto`).
 */
import { TEXT_MIME, type ClipboardMessage } from './clipboard-pb';

export const MAX_TEXT_BYTES = 1024 * 1024;

export type ReadPermission = 'granted' | 'denied' | 'prompt' | 'unknown';

export interface ClipboardEnv {
  readText(): Promise<string>;
  writeText(text: string): Promise<void>;
  readPermission(): Promise<ReadPermission>;
  hasFocus(): boolean;
}

/** Whether a focus event may read the local clipboard. */
export function mayReadOnFocus(enabled: boolean, permission: ReadPermission): boolean {
  return enabled && permission === 'granted';
}

export interface ClipboardState {
  /** The host granted clipboard access. */
  enabled: boolean;
  /** Last known `clipboard-read` permission. */
  readPermission: ReadPermission;
  sent: number;
  received: number;
  /** Host text waiting for focus to be written. */
  pendingWrite: boolean;
}

export interface ClipboardControls {
  getState(): ClipboardState;
  subscribe(listener: (s: ClipboardState) => void): () => void;
  /** Reads the local clipboard now (call from a click: may prompt) and offers it. */
  syncNow(): Promise<void>;
}

const bytesOf = (t: string) => new TextEncoder().encode(t);

export class ClipboardSync implements ClipboardControls {
  private nextOfferId = 1;
  /** Our outstanding offer. */
  private local: { id: number; bytes: Uint8Array } | null = null;
  /** The host offer we requested. */
  private requested: number | null = null;
  /** Last text seen in either direction (echo suppression). */
  private last: string | null = null;
  private pending: string | null = null;
  private readonly listeners = new Set<(s: ClipboardState) => void>();
  private state: ClipboardState = {
    enabled: false,
    readPermission: 'unknown',
    sent: 0,
    received: 0,
    pendingWrite: false,
  };

  constructor(
    private readonly send: (m: ClipboardMessage) => void,
    private readonly env: ClipboardEnv,
  ) {}

  getState(): ClipboardState {
    return { ...this.state };
  }

  subscribe(listener: (s: ClipboardState) => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  private update(patch: Partial<ClipboardState>): void {
    this.state = { ...this.state, ...patch };
    const s = this.getState();
    for (const l of this.listeners) l(s);
  }

  setEnabled(enabled: boolean): void {
    if (!enabled) {
      this.local = null;
      this.requested = null;
      this.pending = null;
    }
    this.update({ enabled, pendingWrite: this.pending !== null });
  }

  /** Offers `text` to the host (from a paste event or a permitted read). */
  offer(text: string): boolean {
    if (!this.state.enabled || text === '' || text === this.last) return false;
    const bytes = bytesOf(text);
    if (bytes.length > MAX_TEXT_BYTES) return false;
    this.last = text;
    const id = this.nextOfferId;
    this.nextOfferId += 1;
    this.local = { id, bytes };
    this.send({
      type: 'clipboardOffer',
      offerId: id,
      formats: [{ mime: TEXT_MIME, size: bytes.length }],
    });
    return true;
  }

  /** Page or canvas gained focus: flush a pending host write, then maybe read. */
  async onFocus(): Promise<void> {
    if (!this.state.enabled) return;
    await this.flush();
    const permission = await this.env.readPermission().catch((): ReadPermission => 'unknown');
    this.update({ readPermission: permission });
    if (!mayReadOnFocus(this.state.enabled, permission)) return;
    await this.readAndOffer();
  }

  /** A `paste` the user caused (no permission needed). */
  onPaste(text: string): void {
    this.offer(text);
  }

  async syncNow(): Promise<void> {
    if (!this.state.enabled) return;
    await this.readAndOffer();
    const permission = await this.env.readPermission().catch((): ReadPermission => 'unknown');
    this.update({ readPermission: permission });
  }

  private async readAndOffer(): Promise<void> {
    let text: string;
    try {
      text = await this.env.readText();
    } catch {
      return;
    }
    this.offer(text);
  }

  private async flush(): Promise<void> {
    const text = this.pending;
    if (text === null || !this.env.hasFocus()) return;
    try {
      await this.env.writeText(text);
      this.pending = null;
      this.update({ pendingWrite: false });
    } catch {
      // Still not allowed; retried on the next focus.
    }
  }

  /** Handles a clipboard message from the host. */
  onMessage(m: ClipboardMessage): void {
    if (!this.state.enabled) return;
    if (m.type === 'clipboardRequest') {
      if (this.local?.id === m.offerId && m.mime === TEXT_MIME) {
        this.send({
          type: 'clipboardData',
          offerId: m.offerId,
          mime: TEXT_MIME,
          data: this.local.bytes,
        });
        this.update({ sent: this.state.sent + 1 });
      }
      return;
    }
    if (m.type === 'clipboardOffer') {
      const text = m.formats.find((f) => f.mime === TEXT_MIME);
      if (!text || text.size > MAX_TEXT_BYTES) return;
      this.requested = m.offerId;
      this.send({ type: 'clipboardRequest', offerId: m.offerId, mime: TEXT_MIME });
      return;
    }
    if (m.offerId !== this.requested || m.mime !== TEXT_MIME || m.data.length > MAX_TEXT_BYTES)
      return;
    this.requested = null;
    let text: string;
    try {
      text = new TextDecoder('utf-8', { fatal: true }).decode(m.data);
    } catch {
      return;
    }
    this.last = text;
    this.pending = text;
    this.update({ received: this.state.received + 1, pendingWrite: true });
    void this.flush();
  }
}

/** `navigator.clipboard` + Permissions API; `null` without a clipboard API. */
export function webClipboard(): ClipboardEnv | null {
  if (typeof navigator === 'undefined' || typeof navigator.clipboard === 'undefined') return null;
  const clip = navigator.clipboard;
  // `clipboard-read` is missing from lib.dom's PermissionName; a method-typed
  // view of `permissions` accepts any name (Chromium supports it, others throw).
  const perms: { query(d: { name: string }): Promise<PermissionStatus> } | undefined =
    typeof navigator.permissions === 'undefined' ? undefined : navigator.permissions;
  return {
    readText: () => clip.readText(),
    writeText: (t) => clip.writeText(t),
    async readPermission() {
      if (!perms) return 'unknown';
      try {
        return (await perms.query({ name: 'clipboard-read' })).state;
      } catch {
        return 'unknown';
      }
    },
    hasFocus: () => document.hasFocus(),
  };
}
