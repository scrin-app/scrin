import { lazy, Suspense, useEffect, useState } from 'react';

// A subpath, not the barrel: a dynamic import of the barrel would keep every
// export alive. Sonner keeps its queue in module state, so `toast()` calls
// made before this chunk loads still show once the Toaster mounts.
const Toaster = lazy(() => import('@scrin/ui/toaster').then((m) => ({ default: m.Toaster })));

/** Mounts the toast region after first paint, keeping sonner out of the entry chunk. */
export function LazyToaster() {
  const [ready, setReady] = useState(false);
  useEffect(() => {
    const id = requestIdleCallback(() => setReady(true), { timeout: 1500 });
    return () => cancelIdleCallback(id);
  }, []);
  return ready ? (
    <Suspense fallback={null}>
      <Toaster />
    </Suspense>
  ) : null;
}
