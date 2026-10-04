import type { QueryClient } from '@tanstack/react-query';
import { createRootRouteWithContext, lazyRouteComponent, Outlet } from '@tanstack/react-router';

export interface RouterContext {
  queryClient: QueryClient;
}

export const Route = createRootRouteWithContext<RouterContext>()({
  component: () => <Outlet />,
  notFoundComponent: lazyRouteComponent(() => import('../screens/not-found'), 'NotFound'),
});
