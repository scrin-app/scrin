/**
 * Live browser sessions, shared between the engine (which creates them) and
 * the canvas client (which renders and sends input). One SPA, one engine, so
 * a module-level registry is enough.
 */
import type { Incoming, VideoConfig } from '@scrin/protocol';

import type { EngineEvent } from '../../platform';
import type { GatewaySession } from './session';

export interface LiveSession {
  readonly id: string;
  readonly session: GatewaySession;
  /** Latest `VideoConfig` (it may arrive before the canvas mounts). */
  videoConfig: VideoConfig | null;
  readonly onMessage: Set<(m: Incoming) => void>;
  readonly onVideo: Set<(frameId: number, keyframe: boolean, data: Uint8Array) => void>;
  /** Publishes an engine event (stats) to the UI. */
  emit(e: EngineEvent): void;
}

const live = new Map<string, LiveSession>();
const waiters = new Map<string, Set<(s: LiveSession) => void>>();

export function registerSession(s: LiveSession): void {
  live.set(s.id, s);
  for (const w of waiters.get(s.id) ?? []) w(s);
  waiters.delete(s.id);
}

export function unregisterSession(id: string): void {
  live.delete(id);
}

export function getSession(id: string): LiveSession | undefined {
  return live.get(id);
}

/** Calls `cb` with the session now or once it registers; returns an unsubscribe. */
export function whenSession(id: string, cb: (s: LiveSession) => void): () => void {
  const s = live.get(id);
  if (s) {
    cb(s);
    return () => undefined;
  }
  const set = waiters.get(id) ?? new Set();
  set.add(cb);
  waiters.set(id, set);
  return () => {
    set.delete(cb);
  };
}
