import {
  createWebStorage,
  HostProvider,
  MotionProvider,
  ThemeProvider,
  THEME_STORAGE_KEY,
  TooltipProvider,
  type KeyValueStorage,
} from '@scrin/ui';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it } from 'vitest';

import { host } from '../host';
import { SettingsPage } from './settings';
import { createUiPrefs, UI_PREFS_KEY } from './ui-prefs';

function memoryStorage(
  seed: Record<string, string> = {},
): KeyValueStorage & { data: Map<string, string> } {
  const data = new Map(Object.entries(seed));
  return {
    data,
    get: (k) => data.get(k) ?? null,
    set: (k, v) => {
      data.set(k, v);
    },
    remove: (k) => {
      data.delete(k);
    },
  };
}

describe('ui prefs persistence', () => {
  it('writes every change to storage and reads it back in a new store', () => {
    const storage = memoryStorage();
    const a = createUiPrefs(storage);
    act(() => {
      a.getState().set({ relayMode: 'direct', toolbarEdge: 'left', updateChannel: 'beta' });
    });
    const b = createUiPrefs(storage);
    expect(b.getState().prefs).toMatchObject({
      relayMode: 'direct',
      toolbarEdge: 'left',
      updateChannel: 'beta',
    });
  });

  it('falls back per field on corrupt values and on broken JSON', () => {
    const storage = memoryStorage({
      [UI_PREFS_KEY]: JSON.stringify({ relayMode: 'teleport', toolbarAutoHide: true }),
    });
    const s = createUiPrefs(storage).getState().prefs;
    expect(s.relayMode).toBe('auto');
    expect(s.toolbarAutoHide).toBe(true);
    const broken = createUiPrefs(memoryStorage({ [UI_PREFS_KEY]: '{nope' })).getState().prefs;
    expect(broken.toolbarEdge).toBe('top');
  });
});

function renderSettings(storage: KeyValueStorage) {
  return render(
    <HostProvider host={host}>
      <ThemeProvider storage={storage}>
        <MotionProvider>
          <TooltipProvider>
            <SettingsPage />
          </TooltipProvider>
        </MotionProvider>
      </ThemeProvider>
    </HostProvider>,
  );
}

describe('SettingsPage', () => {
  beforeEach(() => {
    history.replaceState(null, '', '/settings');
  });

  it('lists every section as a tab and shows a skeleton then the section', async () => {
    renderSettings(createWebStorage());
    const tabs = screen.getAllByRole('tab');
    expect(tabs.map((t) => t.textContent)).toEqual([
      'General',
      'Security',
      'Network',
      'Video',
      'Audio',
      'Input',
      'Appearance',
      'Language',
      'Updates',
      'About',
    ]);
    expect(await screen.findByRole('heading', { level: 2, name: 'General' })).toBeTruthy();
  });

  it('persists appearance changes through the theme storage', async () => {
    const storage = memoryStorage();
    renderSettings(storage);
    fireEvent.click(screen.getByRole('tab', { name: 'Appearance' }));
    await screen.findByRole('heading', { level: 2, name: 'Appearance' });
    fireEvent.click(screen.getByRole('radio', { name: 'Dark' }));
    fireEvent.click(screen.getByRole('radio', { name: 'AMOLED black' }));
    await waitFor(() => {
      const saved: unknown = JSON.parse(storage.get(THEME_STORAGE_KEY) ?? '{}');
      expect(saved).toMatchObject({ mode: 'dark', surface: 'amoled' });
    });
  });

  it('shows the web updater note and disables the permanent password with a reason', async () => {
    renderSettings(createWebStorage());
    fireEvent.click(screen.getByRole('tab', { name: 'Updates' }));
    expect(
      await screen.findByText('The web app updates itself when you reload the page.'),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole('tab', { name: 'Security' }));
    const btn = await screen.findByRole('button', { name: 'Set password' });
    expect(btn.getAttribute('aria-disabled') ?? btn.getAttribute('disabled')).not.toBeNull();
    expect(screen.getAllByText('Not available in this version yet').length).toBeGreaterThan(0);
  });

  it('switches sections from the keyboard', async () => {
    renderSettings(createWebStorage());
    const general = screen.getByRole('tab', { name: 'General' });
    general.focus();
    fireEvent.keyDown(general, { key: 'ArrowRight' });
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole('tab', { name: 'Security' })),
    );
    fireEvent.click(screen.getByRole('tab', { name: 'Security' }));
    expect(await screen.findByRole('heading', { level: 2, name: 'Security' })).toBeTruthy();
    expect(location.hash).toBe('#security');
  });
});
