import { HostProvider, MotionProvider, createMockEngine, type ScrinHost } from '@scrin/ui';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from '@tanstack/react-router';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const WORDS = 'river magpie tiger meadow trout';

const mock = vi.hoisted(() => ({ host: null as ScrinHost | null }));
vi.mock('../host', () => ({
  get host() {
    return mock.host;
  },
}));

const setPassphrase = vi.fn((lang: string | null) =>
  Promise.resolve(lang ? { words: WORDS, expiresAt: Date.now() + 600_000 } : null),
);

function makeHost(passphrase: boolean): ScrinHost {
  const engine = createMockEngine({ myId: '123456789', latencyScale: 0 });
  let current: { words: string; expiresAt: number } | null = null;
  const withPhrase = passphrase
    ? {
        supportsPassphrase: true,
        setPassphrase: async (lang: string | null) => {
          current = await setPassphrase(lang);
          return current;
        },
        getPassphrase: () => Promise.resolve(current),
      }
    : {};
  return {
    platform: { kind: 'desktop', os: 'windows', version: '0', canHost: true, canShare: false },
    storage: { get: () => null, set: () => undefined, remove: () => undefined },
    engine: { ...engine, ...withPhrase },
    writeClipboard: vi.fn(() => Promise.resolve()),
    openExternal: () => Promise.resolve(),
    notify: () => Promise.resolve(),
  };
}

// lib/engine subscribes to the host when it loads: give it one first.
mock.host = makeHost(false);
const { HomePage } = await import('./home');
const { usePending } = await import('../lib/prefs');

function renderHome(host: ScrinHost) {
  mock.host = host;
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const root = createRootRoute({
    component: () => (
      <QueryClientProvider client={qc}>
        <HostProvider host={host}>
          <MotionProvider>
            <HomePage />
          </MotionProvider>
        </HostProvider>
      </QueryClientProvider>
    ),
  });
  const index = createRoute({ getParentRoute: () => root, path: '/' });
  const connect = createRoute({ getParentRoute: () => root, path: '/connect/$id' });
  const router = createRouter({
    routeTree: root.addChildren([index, connect]),
    history: createMemoryHistory({ initialEntries: ['/'] }),
  });
  render(<RouterProvider router={router} />);
  return router;
}

describe('HomePage passphrase (D24)', () => {
  beforeEach(() => {
    usePending.getState().setCode(null);
    setPassphrase.mockClear();
  });

  it('shows five words after the host turns the passphrase on', async () => {
    renderHome(makeHost(true));
    fireEvent.click(await screen.findByRole('button', { name: 'Show words' }));
    const list = await screen.findByRole('list', { name: 'Passphrase' });
    expect(list.querySelectorAll('li')).toHaveLength(5);
    expect(list.textContent).toBe(WORDS.replaceAll(' ', ''));
    expect(setPassphrase).toHaveBeenCalledWith('en');
    fireEvent.click(screen.getByRole('button', { name: 'Turn off' }));
    expect(await screen.findByRole('button', { name: 'Show words' })).toBeTruthy();
    expect(setPassphrase).toHaveBeenLastCalledWith(null);
  });

  it('connects with typed words without putting them in the URL', async () => {
    const router = renderHome(makeHost(true));
    fireEvent.click(await screen.findByRole('radio', { name: 'Words' }));
    const input = screen.getByLabelText('The five words');
    fireEvent.change(input, { target: { value: 'river magpie tiger' } });
    fireEvent.click(screen.getByRole('button', { name: 'Connect' }));
    expect(await screen.findByText('Type all five words, separated by spaces')).toBeTruthy();

    fireEvent.change(input, { target: { value: `  ${WORDS.toUpperCase()} ` } });
    fireEvent.click(screen.getByRole('button', { name: 'Connect' }));
    await waitFor(() => {
      expect(router.state.location.pathname).toBe('/connect/phrase');
    });
    expect(usePending.getState().code).toBe(WORDS.toUpperCase());
  });

  it('hides the passphrase UI where the engine cannot do it', async () => {
    renderHome(makeHost(false));
    expect(await screen.findByText('Your ID')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Show words' })).toBeNull();
    expect(screen.queryByRole('radio', { name: 'Words' })).toBeNull();
  });
});
