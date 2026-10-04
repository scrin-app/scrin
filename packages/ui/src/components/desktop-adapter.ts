import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import type { HostExtras, TrustedPeer, UpdateCheck } from './host-extras';
import { hostRoleActionsFromInvoke, pushHostEvent, setHostRoleActions } from './host-role';

type Invoke = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
type Listen = (event: string, handler: (payload: unknown) => void) => Promise<() => void>;

function toTrusted(v: unknown): TrustedPeer[] {
  if (!Array.isArray(v)) return [];
  const out: TrustedPeer[] = [];
  for (const item of v) {
    if (typeof item !== 'object' || item === null) continue;
    const device: unknown = Reflect.get(item, 'device');
    const fingerprint: unknown = Reflect.get(item, 'fingerprint');
    const label: unknown = Reflect.get(item, 'label');
    const addedAt: unknown = Reflect.get(item, 'addedAt');
    if (typeof device !== 'string') continue;
    out.push({
      device,
      fingerprint: typeof fingerprint === 'string' ? fingerprint : '',
      label: typeof label === 'string' && label !== '' ? label : device.slice(0, 8),
      addedAt: typeof addedAt === 'number' ? addedAt : 0,
    });
  }
  return out;
}

/** `plugin:updater|check` returns update metadata or `null` when up to date. */
function toUpdate(v: unknown): UpdateCheck {
  if (v === null || v === undefined) return { status: 'up-to-date' };
  const version: unknown = typeof v === 'object' ? Reflect.get(v, 'version') : undefined;
  return typeof version === 'string' ? { status: 'available', version } : { status: 'error' };
}

/**
 * Desktop implementation over the shell's Tauri commands
 * (`apps/desktop/src-tauri/src/lib.rs`). Also starts the host-role feed:
 * `incomingRequest` events go to the request dialog's store and its answers
 * go back through `scrin_accept` / `scrin_reject`.
 */
export function createDesktopExtras(
  bridge: { invoke: Invoke; listen: Listen } = {
    invoke: (cmd, args) => invoke(cmd, args),
    listen: async (event, handler) => listen(event, (e) => handler(e.payload)),
  },
): HostExtras {
  setHostRoleActions(hostRoleActionsFromInvoke(bridge.invoke));
  void bridge.listen('scrin://event', pushHostEvent);
  return {
    async setQuality(session, quality) {
      await bridge.invoke('scrin_set_quality', { session, quality });
    },
    async setPermission(session, permission, granted) {
      await bridge.invoke('scrin_set_permission', { session, permission, granted });
    },
    async getServer() {
      const v = await bridge.invoke('scrin_get_server');
      return typeof v === 'string' ? v : null;
    },
    async setServer(url) {
      await bridge.invoke('scrin_set_server', { server: url });
    },
    async listTrusted() {
      return toTrusted(await bridge.invoke('scrin_list_trusted'));
    },
    async removeTrusted(device) {
      await bridge.invoke('scrin_remove_trusted', { device });
    },
    async checkUpdate() {
      try {
        return toUpdate(await bridge.invoke('plugin:updater|check', {}));
      } catch {
        return { status: 'error' };
      }
    },
  };
}
