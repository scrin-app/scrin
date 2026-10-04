import { afterEach, describe, expect, it } from 'vitest';

import { createWebHost } from '../../host/web-host';

afterEach(() => {
  localStorage.clear();
  document.head.querySelector('meta[name="scrin-server"]')?.remove();
  history.replaceState(null, '', '/');
});

/** The real engine answers getMyId with the non-host placeholder; the mock with a 9-digit id. */
const isReal = async (h: ReturnType<typeof createWebHost>) =>
  (await h.engine.getMyId()) === '000000000';

describe('WebHost engine selection', () => {
  it('uses the mock engine when no server is configured', async () => {
    expect(await isReal(createWebHost({ latencyScale: 0 }))).toBe(false);
  });

  it('uses the real engine for an explicit server URL', async () => {
    expect(await isReal(createWebHost({ serverUrl: 'https://s.example' }))).toBe(true);
    expect(await isReal(createWebHost({ serverUrl: undefined, latencyScale: 0 }))).toBe(false);
  });

  it('reads ?server=, remembers it, and ?server=demo forgets it', async () => {
    history.replaceState(null, '', '/?server=https://s.example:8443/path');
    expect(await isReal(createWebHost())).toBe(true);
    expect(localStorage.getItem('scrin.server')).toBe('https://s.example:8443');
    history.replaceState(null, '', '/');
    expect(await isReal(createWebHost())).toBe(true);
    history.replaceState(null, '', '/?server=demo');
    expect(await isReal(createWebHost({ latencyScale: 0 }))).toBe(false);
    expect(localStorage.getItem('scrin.server')).toBeNull();
  });

  it('falls back to <meta name="scrin-server"> and ignores invalid URLs', async () => {
    localStorage.setItem('scrin.server', 'javascript:alert(1)');
    expect(await isReal(createWebHost({ latencyScale: 0 }))).toBe(false);
    const meta = document.createElement('meta');
    meta.name = 'scrin-server';
    meta.content = 'https://meta.example';
    document.head.append(meta);
    expect(await isReal(createWebHost())).toBe(true);
  });
});
