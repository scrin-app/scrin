import { describe, expect, it, vi } from 'vitest';

import type { EngineEvent } from '../platform';
import {
  createDesktopHost,
  deepLinkPath,
  mapNativeEvent,
  type HostRoleEvent,
  type NativeEvent,
  type TauriBridge,
} from './desktop';

function fakeBridge() {
  let handler: ((p: unknown) => void) | null = null;
  const calls: { cmd: string; args: Record<string, unknown> | undefined }[] = [];
  const responses = new Map<string, unknown>();
  const bridge: TauriBridge = {
    invoke: (cmd: string, args?: Record<string, unknown>) => {
      calls.push({ cmd, args });
      return Promise.resolve(responses.get(cmd));
    },
    listen: (_event: string, h: (payload: unknown) => void) => {
      handler = h;
      return Promise.resolve(() => undefined);
    },
  };
  return {
    bridge,
    calls,
    responses,
    fire: (e: NativeEvent) => handler?.(e),
  };
}

const STATUS = {
  deviceId: 'ab'.repeat(32),
  fingerprint: 'abcd-efgh-ijkl-mnop',
  scrinId: '123456789',
  ticket: 'scrin:00',
  code: 'ABCD-EFGH',
  codeIssuedAt: 1,
  codeExpiresAt: 2,
  phrase: '',
  phraseExpiresAt: 0,
  online: true,
  backend: 'synthetic',
};

describe('DesktopHost', () => {
  it('enables a passphrase and resolves with the words from the next status', async () => {
    const f = fakeBridge();
    f.responses.set('scrin_enable_phrase', STATUS);
    const host = createDesktopHost({ bridge: f.bridge });
    const pending = host.engine.setPassphrase?.('ro');
    await Promise.resolve();
    expect(f.calls.at(-1)).toEqual({ cmd: 'scrin_enable_phrase', args: { lang: 'ro' } });
    f.fire({
      type: 'status',
      ...STATUS,
      phrase: 'casă pădure lămâie zid ponei',
      phraseExpiresAt: 9,
    });
    await expect(pending).resolves.toEqual({
      words: 'casă pădure lămâie zid ponei',
      expiresAt: 9,
    });
  });

  it('disables the passphrase', async () => {
    const f = fakeBridge();
    const host = createDesktopHost({ bridge: f.bridge });
    await expect(host.engine.setPassphrase?.(null)).resolves.toBeNull();
    expect(f.calls.at(-1)?.cmd).toBe('scrin_disable_phrase');
  });

  it('gives up waiting for a passphrase after the timeout', async () => {
    vi.useFakeTimers();
    try {
      const f = fakeBridge();
      f.responses.set('scrin_enable_phrase', STATUS);
      const host = createDesktopHost({ bridge: f.bridge });
      const pending = host.engine.setPassphrase?.('en');
      await vi.advanceTimersByTimeAsync(10_001);
      await expect(pending).resolves.toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it('maps controller states to connect stages and ended', () => {
    const base = {
      type: 'stateChanged',
      session: 'c1',
      role: 'controller',
      peer: null,
      reason: null,
    } as const;
    expect(mapNativeEvent({ ...base, state: 'pairing' })).toEqual([
      { type: 'stage', sessionId: 'c1', stage: 'securing' },
    ]);
    expect(mapNativeEvent({ ...base, state: 'ended' })).toEqual([
      { type: 'ended', sessionId: 'c1' },
    ]);
    expect(mapNativeEvent({ ...base, role: 'host', state: 'active' })).toEqual([]);
    expect(
      mapNativeEvent({ type: 'error', session: 'c1', code: 'wrong-code', message: '' }),
    ).toEqual([{ type: 'error', sessionId: 'c1', error: 'wrong-code' }]);
  });

  it('calls engine commands and strips the code dash', async () => {
    const f = fakeBridge();
    f.responses.set('scrin_status', STATUS);
    f.responses.set('scrin_connect', { sessionId: 'c7' });
    const host = createDesktopHost({ bridge: f.bridge });
    expect(host.platform.canHost).toBe(true);
    expect(await host.engine.getMyId()).toBe('123456789');
    expect((await host.engine.getCode()).code).toBe('ABCDEFGH');
    const handle = await host.engine.connect('123456789', 'abcdefgh');
    expect(handle.sessionId).toBe('c7');
    expect(f.calls.at(-1)).toEqual({
      cmd: 'scrin_connect',
      args: { target: '123456789', code: 'abcdefgh' },
    });
    await host.engine.sendKeys('c7', 'ctrl-alt-del');
    expect(f.calls.at(-1)?.cmd).toBe('scrin_send_keys');
  });

  it('a failed attempt reports its error once and does not end a session', () => {
    const f = fakeBridge();
    const hostEvents: HostRoleEvent[] = [];
    const host = createDesktopHost({ bridge: f.bridge, onHostEvent: (e) => hostEvents.push(e) });
    const seen: EngineEvent[] = [];
    host.engine.onEvent((e) => seen.push(e));
    f.fire({ type: 'error', session: 'c2', code: 'offline', message: 'x' });
    f.fire({
      type: 'stateChanged',
      session: 'c2',
      role: 'controller',
      state: 'ended',
      peer: null,
      reason: 'connect-failed',
    });
    expect(seen).toEqual([{ type: 'error', sessionId: 'c2', error: 'offline' }]);
    f.fire({ type: 'status', ...STATUS });
    expect(hostEvents).toHaveLength(1);
  });

  it('only opens https links', async () => {
    const host = createDesktopHost({ bridge: fakeBridge().bridge });
    const spy = vi.fn();
    vi.doMock('@tauri-apps/plugin-opener', () => ({ openUrl: spy }));
    await host.openExternal('javascript:alert(1)');
    expect(spy).not.toHaveBeenCalled();
  });

  it('accepts only scrin://connect/<9 digits> deep links', () => {
    expect(deepLinkPath('scrin://connect/123456789')).toBe('/connect/123456789');
    expect(deepLinkPath('scrin://connect/12345')).toBeNull();
    expect(deepLinkPath('scrin://connect/123456789/../settings')).toBeNull();
    expect(deepLinkPath('https://evil.example/connect/123456789')).toBeNull();
  });
});
