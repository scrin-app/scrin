/**
 * A `SessionEngine` that loads the real browser engine on first use, so the
 * initial bundle only carries this proxy. Listeners registered before the
 * engine loads are attached when it arrives.
 */
import type { EngineEvent, SessionEngine } from '../../platform';
import type { WebEngineOptions } from './engine';

export function createLazyWebEngine(opts: WebEngineOptions): SessionEngine {
  let loaded: Promise<SessionEngine> | null = null;
  const listeners = new Set<(e: EngineEvent) => void>();
  const forward = (e: EngineEvent) => {
    for (const l of listeners) l(e);
  };
  const engine = () =>
    (loaded ??= import('./engine').then(({ createWebEngine }) => {
      const real = createWebEngine(opts);
      real.onEvent(forward);
      return real;
    }));
  return {
    getMyId: async () => (await engine()).getMyId(),
    getCode: async () => (await engine()).getCode(),
    regenerateCode: async () => (await engine()).regenerateCode(),
    connect: async (id, code) => (await engine()).connect(id, code),
    confirmSas: async (s, m) => (await engine()).confirmSas(s, m),
    sendKeys: async (s, k) => (await engine()).sendKeys(s, k),
    sendChat: async (s, t) => (await engine()).sendChat(s, t),
    endSession: async (s) => (await engine()).endSession(s),
    onEvent(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}
