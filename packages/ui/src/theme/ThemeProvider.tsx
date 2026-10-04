import {
  createContext,
  use,
  useCallback,
  useEffect,
  useMemo,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from 'react';

import type { KeyValueStorage } from '../platform';
import {
  DEFAULT_THEME,
  motionScale,
  parseThemeSettings,
  resolveMode,
  themeToCssVars,
  themeToDataAttrs,
  type ResolvedMode,
  type ThemeSettings,
} from './tokens';

export const THEME_STORAGE_KEY = 'scrin.theme';

export interface ThemeContextValue {
  settings: ThemeSettings;
  resolvedMode: ResolvedMode;
  reducedMotion: boolean;
  /** 0 when animation is off or the OS asks for reduced motion. */
  motionScale: number;
  update: (patch: Partial<ThemeSettings>) => void;
  /** Flips between light and dark from whatever is currently painted. */
  toggleMode: () => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

export function useTheme(): ThemeContextValue {
  const ctx = use(ThemeContext);
  if (!ctx) throw new Error('useTheme must be used inside <ThemeProvider>');
  return ctx;
}

/** Non-throwing variant for components that also render outside a provider (tests, previews). */
export function useOptionalTheme(): ThemeContextValue | null {
  return use(ThemeContext);
}

export interface ThemeProviderProps {
  storage?: KeyValueStorage;
  /** Overrides the stored settings (tests, previews). */
  initial?: ThemeSettings;
  children: ReactNode;
}

function readStored(storage: KeyValueStorage | undefined): ThemeSettings {
  const raw = storage?.get(THEME_STORAGE_KEY);
  if (!raw) return DEFAULT_THEME;
  try {
    return parseThemeSettings(JSON.parse(raw));
  } catch {
    return DEFAULT_THEME;
  }
}

/**
 * Writes the theme as CSS custom properties and data attributes on `<html>`,
 * not on a wrapper div: portals (dialogs, toasts, the command palette) render
 * outside the React tree and must be themed too.
 */
export function ThemeProvider({ storage, initial, children }: ThemeProviderProps) {
  const [settings, setSettings] = useState<ThemeSettings>(() => initial ?? readStored(storage));
  const systemPrefersDark = useMediaQuery('(prefers-color-scheme: dark)', false);
  const reducedMotion = useMediaQuery('(prefers-reduced-motion: reduce)', false);

  const update = useCallback(
    (patch: Partial<ThemeSettings>) => {
      setSettings((prev) => {
        const next = { ...prev, ...patch };
        storage?.set(THEME_STORAGE_KEY, JSON.stringify(next));
        return next;
      });
    },
    [storage],
  );

  const resolvedMode = resolveMode(settings.mode, systemPrefersDark);

  const toggleMode = useCallback(() => {
    update({ mode: resolvedMode === 'dark' ? 'light' : 'dark' });
  }, [resolvedMode, update]);

  useEffect(() => {
    const root = document.documentElement;
    const env = { systemPrefersDark, reducedMotion };
    const vars = themeToCssVars(settings, env);
    for (const [k, v] of Object.entries(vars)) root.style.setProperty(k, v);
    for (const [k, v] of Object.entries(themeToDataAttrs(settings, env))) root.setAttribute(k, v);
    root.style.colorScheme = resolveMode(settings.mode, systemPrefersDark);
  }, [settings, systemPrefersDark, reducedMotion]);

  const value = useMemo<ThemeContextValue>(
    () => ({
      settings,
      resolvedMode,
      reducedMotion,
      motionScale: motionScale(settings.motion, reducedMotion),
      update,
      toggleMode,
    }),
    [settings, resolvedMode, reducedMotion, update, toggleMode],
  );

  return <ThemeContext value={value}>{children}</ThemeContext>;
}

function useMediaQuery(query: string, serverValue: boolean): boolean {
  return useSyncExternalStore(
    (onChange) => {
      if (typeof window === 'undefined' || typeof window.matchMedia !== 'function')
        return () => undefined;
      const mql = window.matchMedia(query);
      mql.addEventListener('change', onChange);
      return () => {
        mql.removeEventListener('change', onChange);
      };
    },
    () =>
      typeof window !== 'undefined' && typeof window.matchMedia === 'function'
        ? window.matchMedia(query).matches
        : serverValue,
    () => serverValue,
  );
}
