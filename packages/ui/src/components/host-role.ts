import { useSyncExternalStore } from 'react';

import {
  defaultAllowed,
  isPermissionName,
  isSessionKind,
  type PermissionName,
  type SessionKindName,
} from './permissions';

/**
 * Host-role adapter: incoming connection requests and the accept / reject
 * answers. The platform host feeds native events in with `pushHostEvent`
 * (DesktopHost's `onHostEvent`) and registers the answer channel with
 * `setHostRoleActions`; the request dialog only talks to this module.
 */

export interface IncomingRequest {
  session: string;
  /** Display name or ID the controller announced. */
  peer: string;
  fingerprint: string;
  kind: SessionKindName;
  sas: readonly number[] | null;
  requested: readonly PermissionName[];
  /** Ceiling from the host policy; anything else is greyed out. */
  allowed: readonly PermissionName[];
  /** Epoch ms when Accept becomes clickable (anti-scam delay). */
  acceptEnabledAt: number;
  /** Epoch ms when the request is denied automatically. */
  expiresAt: number;
}

export interface HostRoleActions {
  accept(session: string, permissions: readonly PermissionName[]): Promise<void>;
  reject(session: string): Promise<void>;
}

type Invoke = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;

/** Answer channel over the desktop shell's `scrin_accept` / `scrin_reject` commands. */
export function hostRoleActionsFromInvoke(invoke: Invoke): HostRoleActions {
  return {
    accept: async (session, permissions) => {
      await invoke('scrin_accept', { session, permissions: [...permissions] });
    },
    reject: async (session) => {
      await invoke('scrin_reject', { session });
    },
  };
}

let actions: HostRoleActions | null = null;
let pending: readonly IncomingRequest[] = [];
const listeners = new Set<() => void>();

function publish(next: readonly IncomingRequest[]) {
  pending = next;
  for (const l of listeners) l();
}

export function setHostRoleActions(next: HostRoleActions | null): void {
  actions = next;
}

export function getHostRoleActions(): HostRoleActions | null {
  return actions;
}

function names(v: unknown): PermissionName[] {
  return Array.isArray(v) ? v.filter(isPermissionName) : [];
}

function num(v: unknown, fallback: number): number {
  return typeof v === 'number' && Number.isFinite(v) ? v : fallback;
}

function sas(v: unknown): number[] | null {
  if (!Array.isArray(v) || v.length !== 5) return null;
  return v.every((n) => typeof n === 'number' && Number.isInteger(n) && n >= 0 && n < 64)
    ? v.map(Number)
    : null;
}

function str(o: object, key: string): string | null {
  const v: unknown = Reflect.get(o, key);
  return typeof v === 'string' ? v : null;
}

/** Parses a native `incomingRequest` event; anything malformed is dropped. */
export function parseIncomingRequest(e: unknown, now = Date.now()): IncomingRequest | null {
  if (typeof e !== 'object' || e === null) return null;
  if (str(e, 'type') !== 'incomingRequest') return null;
  const session = str(e, 'session');
  const kindRaw = str(e, 'kind');
  if (!session || !isSessionKind(kindRaw)) return null;
  const allowedRaw = names(Reflect.get(e, 'allowed'));
  const allowed = allowedRaw.length > 0 ? allowedRaw : defaultAllowed(kindRaw);
  return {
    session,
    peer: str(e, 'peer') ?? '',
    fingerprint: str(e, 'fingerprint') ?? '',
    kind: kindRaw,
    sas: sas(Reflect.get(e, 'sas')),
    requested: names(Reflect.get(e, 'requested')),
    allowed,
    acceptEnabledAt: num(Reflect.get(e, 'acceptEnabledAt'), now),
    expiresAt: num(Reflect.get(e, 'expiresAt'), now + 60_000),
  };
}

/** Feed a host-role event (any shape; only incoming requests are kept). */
export function pushHostEvent(e: unknown): void {
  const req = parseIncomingRequest(e);
  if (!req) return;
  publish([...pending.filter((r) => r.session !== req.session), req]);
}

/** Drops a request once it was answered or expired. */
export function dismissIncomingRequest(session: string): void {
  if (pending.some((r) => r.session === session)) {
    publish(pending.filter((r) => r.session !== session));
  }
}

function subscribe(onChange: () => void) {
  listeners.add(onChange);
  return () => {
    listeners.delete(onChange);
  };
}

/** The oldest unanswered request, or `null`. */
export function useIncomingRequest(): IncomingRequest | null {
  return useSyncExternalStore(
    subscribe,
    () => pending[0] ?? null,
    () => null,
  );
}
