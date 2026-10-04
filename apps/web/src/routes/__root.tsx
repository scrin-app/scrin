import type { QueryClient } from '@tanstack/react-query';
import { useHost } from '@scrin/ui';
import { createRootRouteWithContext, lazyRouteComponent, Outlet } from '@tanstack/react-router';
import { lazy, Suspense } from 'react';

export interface RouterContext {
  queryClient: QueryClient;
}

const IncomingRequestHost = lazy(() =>
  import('../screens/incoming-request').then((m) => ({ default: m.IncomingRequestHost })),
);

export const Route = createRootRouteWithContext<RouterContext>()({
  component: Root,
  notFoundComponent: lazyRouteComponent(() => import('../screens/not-found'), 'NotFound'),
});

function Root() {
  const host = useHost();
  return (
    <>
      <Outlet />
      {host.platform.canHost ? (
        <Suspense fallback={null}>
          <IncomingRequestHost />
        </Suspense>
      ) : null}
    </>
  );
}
