import { HostProvider, MotionProvider, ThemeProvider, DEFAULT_THEME } from '@scrin/ui';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { host } from '../host';
import type { SessionState } from '../lib/engine';
import { SessionToolbar, StatsOverlay } from './session-toolbar';
import { useUiPrefs } from './ui-prefs';

const STATS = {
  latencyMs: 12,
  bitrateBps: 8_400_000,
  fps: 60,
  lossRatio: 0.004,
  codec: 'H.264' as const,
  width: 2560,
  height: 1440,
  route: 'direct' as const,
};

const SESSION: SessionState = {
  stats: STATS,
  displays: [{ id: 1, name: 'A', width: 2560, height: 1440, primary: true }],
  chat: [],
  ended: false,
};

function renderToolbar(motion: 'full' | 'off' = 'full', onEnd = vi.fn()) {
  render(
    <HostProvider host={host}>
      <ThemeProvider initial={{ ...DEFAULT_THEME, motion }}>
        <MotionProvider>
          <SessionToolbar
            sessionId="s1"
            session={SESSION}
            fullscreen={false}
            onToggleFullscreen={vi.fn()}
            recording={false}
            onToggleRecording={vi.fn()}
            onEnd={onEnd}
          />
        </MotionProvider>
      </ThemeProvider>
    </HostProvider>,
  );
  return { onEnd };
}

describe('SessionToolbar', () => {
  it('reaches every tool with the keyboard and ends the session', async () => {
    useUiPrefs.getState().set({ toolbarExpanded: true, toolbarEdge: 'top' });
    const { onEnd } = renderToolbar();
    const toolbar = screen.getByRole('toolbar', { name: 'Session toolbar' });
    const names = [...toolbar.querySelectorAll('button')].map((b) => b.getAttribute('aria-label'));
    for (const n of [
      'Quality',
      'Display',
      'Special keys',
      'Files',
      'Chat',
      'Record',
      'Statistics',
      'Full screen',
      'End session',
    ]) {
      expect(names).toContain(n);
    }
    const quality = screen.getByRole('button', { name: 'Quality' });
    quality.focus();
    // Arrow keys move along the toolbar; Left from the first wraps to the collapse toggle.
    fireEvent.keyDown(quality, { key: 'ArrowRight' });
    await waitFor(() => expect(document.activeElement?.getAttribute('aria-label')).toBe('Display'));
    fireEvent.click(screen.getByRole('button', { name: 'End session' }));
    expect(onEnd).toHaveBeenCalledTimes(1);
  });

  it('sends Ctrl+Alt+Del from the special keys menu', async () => {
    const sendKeys = vi.spyOn(host.engine, 'sendKeys');
    renderToolbar();
    fireEvent.click(screen.getByRole('button', { name: 'Special keys' }));
    const item = await screen.findByRole('menuitem', { name: 'Ctrl + Alt + Del' });
    fireEvent.click(item);
    await waitFor(() => expect(sendKeys).toHaveBeenCalled());
    expect(sendKeys).toHaveBeenCalledWith('s1', 'ctrl-alt-del');
  });

  it('remembers the edge it was moved to', () => {
    useUiPrefs.getState().set({ toolbarEdge: 'top' });
    renderToolbar();
    fireEvent.keyDown(screen.getByRole('button', { name: /Move the toolbar/ }), {
      key: 'ArrowDown',
    });
    expect(useUiPrefs.getState().prefs.toolbarEdge).toBe('bottom');
    expect(host.storage.get('scrin.ui-prefs')).toContain('"toolbarEdge":"bottom"');
  });

  it('honours reduced motion', () => {
    renderToolbar('off');
    expect(document.querySelector('[data-reduced-motion]')).not.toBeNull();
  });
});

describe('StatsOverlay', () => {
  it('shows fps, bitrate, rtt, loss and n/a for a missing decode time', () => {
    render(<StatsOverlay stats={STATS} edge="top" />);
    const text = screen.getByRole('complementary', { name: 'Statistics' }).textContent;
    expect(text).toContain('60 fps');
    expect(text).toContain('12 ms');
    expect(text).toContain('Decode time');
    expect(text).toContain('n/a');
    expect(text).toMatch(/8\.4\s?Mb/);
  });
});
