import { MotionProvider } from '@scrin/ui';
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from '@tanstack/react-router';
import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { makeDevices } from '../lib/mock-data';
import { DevicesPage, type DevicesPageProps } from './devices';

/** DevicesPage renders <Link>s, so it needs a router around it. */
function renderPage(props: Partial<DevicesPageProps>) {
  const onChange = vi.fn();
  const root = createRootRoute({
    component: () => (
      <MotionProvider>
        <DevicesPage query="" group="all" view="grid" onChange={onChange} {...props} />
      </MotionProvider>
    ),
  });
  const index = createRoute({ getParentRoute: () => root, path: '/' });
  const router = createRouter({
    routeTree: root.addChildren([index]),
    history: createMemoryHistory({ initialEntries: ['/'] }),
  });
  render(<RouterProvider router={router} />);
  return { onChange };
}

const devices = makeDevices(10, 3);

describe('DevicesPage', () => {
  it('lists every device by default', async () => {
    renderPage({ devices });
    expect(await screen.findAllByRole('heading', { level: 2 })).toHaveLength(10);
  });

  it('filters by name and ID digits', async () => {
    const target = devices[4]!;
    renderPage({ devices, query: target.id.slice(0, 6) });
    const headings = await screen.findAllByRole('heading', { level: 2 });
    expect(headings.map((h) => h.textContent)).toContain(target.name);
  });

  it('shows the no-match state with a clear-filters action', async () => {
    const { onChange } = renderPage({ devices, query: 'zzzz-nothing' });
    const clear = await screen.findByRole('button', { name: 'Clear filters' });
    clear.click();
    expect(onChange).toHaveBeenCalledWith({ q: undefined, group: undefined });
  });

  it('shows the empty state when there are no devices', async () => {
    renderPage({ devices: [] });
    expect(await screen.findByText('No devices yet')).toBeTruthy();
  });

  it('renders the list view', async () => {
    renderPage({ devices, view: 'list' });
    expect(await screen.findAllByRole('link', { name: /^Connect:/ })).toHaveLength(10);
  });
});
