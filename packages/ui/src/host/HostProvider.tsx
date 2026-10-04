import { createContext, use, type ReactNode } from 'react';

import type { ScrinHost } from '../platform';

const HostContext = createContext<ScrinHost | null>(null);

export function HostProvider({ host, children }: { host: ScrinHost; children: ReactNode }) {
  return <HostContext value={host}>{children}</HostContext>;
}

export function useHost(): ScrinHost {
  const host = use(HostContext);
  if (!host) throw new Error('useHost must be used inside <HostProvider>');
  return host;
}

export function useOptionalHost(): ScrinHost | null {
  return use(HostContext);
}
