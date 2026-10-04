import type { ScrinHost } from '../platform';
import type { PermissionName } from './permissions';

/**
 * Host capabilities the `ScrinHost` interface does not model yet (settings,
 * host role, updater). Only the desktop shell has them today; on the web
 * `loadHostExtras` resolves to `null` and screens show the control disabled
 * with an explanation. The implementation is a lazy chunk, so the Tauri
 * bridge never lands in the browser's initial JS.
 */

export type StreamQuality = 'auto' | 'balanced' | 'sharp' | 'speed';

export interface TrustedPeer {
  device: string;
  fingerprint: string;
  label: string;
  addedAt: number;
}

export type UpdateCheck =
  { status: 'up-to-date' } | { status: 'available'; version: string } | { status: 'error' };

export interface HostExtras {
  setQuality(session: string, quality: StreamQuality): Promise<void>;
  setPermission(session: string, permission: PermissionName, granted: boolean): Promise<void>;
  getServer(): Promise<string | null>;
  setServer(url: string): Promise<void>;
  listTrusted(): Promise<TrustedPeer[]>;
  removeTrusted(device: string): Promise<void>;
  checkUpdate(): Promise<UpdateCheck>;
}

let pending: Promise<HostExtras | null> | null = null;

export function loadHostExtras(host: Pick<ScrinHost, 'platform'>): Promise<HostExtras | null> {
  if (host.platform.kind !== 'desktop') return Promise.resolve(null);
  pending ??= import('./desktop-adapter').then(
    (m) => m.createDesktopExtras(),
    () => null,
  );
  return pending;
}
