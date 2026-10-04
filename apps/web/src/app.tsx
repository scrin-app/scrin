import { HostProvider, MotionProvider, ThemeProvider } from '@scrin/ui';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createRouter, RouterProvider } from '@tanstack/react-router';

import { LazyToaster } from './components/lazy-toaster';
import { host } from './host';
import { routeTree } from './routeTree.gen';

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: 1, refetchOnWindowFocus: false } },
});

const router = createRouter({
  routeTree,
  context: { queryClient },
  defaultPreload: 'intent',
  defaultViewTransition: true,
  scrollRestoration: true,
});

declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router;
  }
}

export function App() {
  return (
    <HostProvider host={host}>
      <ThemeProvider storage={host.storage}>
        <MotionProvider>
          <QueryClientProvider client={queryClient}>
            <RouterProvider router={router} />
            <LazyToaster />
          </QueryClientProvider>
        </MotionProvider>
      </ThemeProvider>
    </HostProvider>
  );
}
