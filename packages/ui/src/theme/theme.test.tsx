import { act, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { THEME_STORAGE_KEY, ThemeProvider, useTheme } from './ThemeProvider';
import { DEFAULT_THEME, parseThemeSettings, themeToCssVars } from './tokens';

function memoryStorage(initial: Record<string, string> = {}) {
  const m = new Map(Object.entries(initial));
  return {
    get: (k: string) => m.get(k) ?? null,
    set: vi.fn((k: string, v: string) => {
      m.set(k, v);
    }),
    remove: (k: string) => {
      m.delete(k);
    },
  };
}

function mockMatchMedia(matches: Record<string, boolean>) {
  window.matchMedia = ((q: string) => ({
    matches: matches[q] ?? false,
    media: q,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  })) as unknown as typeof window.matchMedia;
}

function Probe() {
  const { resolvedMode, motionScale, toggleMode, update } = useTheme();
  return (
    <div>
      <span data-testid="mode">{resolvedMode}</span>
      <span data-testid="motion">{motionScale}</span>
      <button type="button" onClick={toggleMode}>
        toggle
      </button>
      <button type="button" onClick={() => update({ density: 'compact' })}>
        compact
      </button>
    </div>
  );
}

describe('ThemeProvider', () => {
  it('resolves system mode from the OS and writes tokens on <html>', () => {
    mockMatchMedia({ '(prefers-color-scheme: dark)': true });
    render(
      <ThemeProvider>
        <Probe />
      </ThemeProvider>,
    );
    expect(screen.getByTestId('mode').textContent).toBe('dark');
    const root = document.documentElement;
    expect(root.getAttribute('data-mode')).toBe('dark');
    expect(root.style.getPropertyValue('--scrin-bg')).toMatch(/^oklch\(/);
  });

  it('persists changes through the storage adapter', () => {
    mockMatchMedia({});
    const storage = memoryStorage();
    render(
      <ThemeProvider storage={storage}>
        <Probe />
      </ThemeProvider>,
    );
    act(() => screen.getByText('compact').click());
    expect(document.documentElement.getAttribute('data-density')).toBe('compact');
    const saved = JSON.parse(storage.get(THEME_STORAGE_KEY) ?? '{}') as { density?: string };
    expect(saved.density).toBe('compact');
  });

  it('toggles from the painted mode', () => {
    mockMatchMedia({});
    render(
      <ThemeProvider>
        <Probe />
      </ThemeProvider>,
    );
    expect(screen.getByTestId('mode').textContent).toBe('light');
    act(() => screen.getByText('toggle').click());
    expect(screen.getByTestId('mode').textContent).toBe('dark');
  });

  it('reduced motion forces the motion scale to 0', () => {
    mockMatchMedia({ '(prefers-reduced-motion: reduce)': true });
    render(
      <ThemeProvider initial={{ ...DEFAULT_THEME, motion: 'full' }}>
        <Probe />
      </ThemeProvider>,
    );
    expect(screen.getByTestId('motion').textContent).toBe('0');
    expect(document.documentElement.getAttribute('data-motion')).toBe('off');
  });

  it('restores stored settings and ignores corrupt values', () => {
    mockMatchMedia({});
    const storage = memoryStorage({ [THEME_STORAGE_KEY]: '{"mode":"dark","density":"weird"}' });
    render(
      <ThemeProvider storage={storage}>
        <Probe />
      </ThemeProvider>,
    );
    expect(screen.getByTestId('mode').textContent).toBe('dark');
    expect(document.documentElement.getAttribute('data-density')).toBe('comfortable');
  });
});

describe('tokens', () => {
  it('parseThemeSettings falls back field by field', () => {
    expect(parseThemeSettings(null)).toEqual(DEFAULT_THEME);
    expect(parseThemeSettings({ accent: { hue: 999, chroma: 0.1 } }).accent).toEqual({
      hue: DEFAULT_THEME.accent.hue,
      chroma: 0.1,
    });
  });

  it('writes every colour token', () => {
    const vars = themeToCssVars(DEFAULT_THEME, { systemPrefersDark: false, reducedMotion: false });
    for (const k of [
      'bg',
      'surface',
      'surface-2',
      'fg',
      'muted',
      'outline',
      'accent',
      'accent-fg',
      'success',
      'warning',
      'danger',
    ]) {
      expect(vars[`--scrin-${k}`]).toMatch(/^oklch\(/);
    }
    expect(vars['--scrin-motion']).toBe('1');
  });
});
