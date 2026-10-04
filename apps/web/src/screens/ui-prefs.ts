import type { KeyValueStorage } from '@scrin/ui';
import * as z from 'zod/mini';
import { create, type StoreApi, type UseBoundStore } from 'zustand';

import { host } from '../host';

/**
 * Preferences added with the session toolbar and the full settings screen.
 * Kept apart from `lib/prefs.ts` (connection prefs) under their own key;
 * same rules: every field falls back on its own.
 */
export const UI_PREFS_KEY = 'scrin.ui-prefs';

const schema = z.object({
  relayMode: z.catch(z.enum(['auto', 'always', 'direct']), 'auto'),
  defaultQuality: z.catch(z.enum(['auto', 'balanced', 'sharp', 'speed']), 'auto'),
  hardwareEncode: z.catch(z.boolean(), true),
  updateChannel: z.catch(z.enum(['stable', 'beta']), 'stable'),
  toolbarEdge: z.catch(z.enum(['top', 'bottom', 'left', 'right']), 'top'),
  toolbarAutoHide: z.catch(z.boolean(), false),
  toolbarExpanded: z.catch(z.boolean(), true),
  showStats: z.catch(z.boolean(), true),
});

export type UiPrefs = z.infer<typeof schema>;

function read(storage: KeyValueStorage): UiPrefs {
  const raw = storage.get(UI_PREFS_KEY);
  let parsed: unknown = {};
  if (raw) {
    try {
      parsed = JSON.parse(raw);
    } catch {
      parsed = {};
    }
  }
  return schema.parse(typeof parsed === 'object' && parsed !== null ? parsed : {});
}

interface UiPrefsState {
  prefs: UiPrefs;
  set: (patch: Partial<UiPrefs>) => void;
}

/** A store bound to `storage`; tests pass their own. */
export function createUiPrefs(storage: KeyValueStorage): UseBoundStore<StoreApi<UiPrefsState>> {
  return create<UiPrefsState>()((set) => ({
    prefs: read(storage),
    set: (patch) => {
      set((s) => {
        const prefs = schema.parse({ ...s.prefs, ...patch });
        storage.set(UI_PREFS_KEY, JSON.stringify(prefs));
        return { prefs };
      });
    },
  }));
}

export const useUiPrefs = createUiPrefs(host.storage);

/** `[value, setter]` for one field. */
export function useUiPref<K extends keyof UiPrefs>(key: K): [UiPrefs[K], (v: UiPrefs[K]) => void] {
  const value = useUiPrefs((s) => s.prefs[key]);
  const set = useUiPrefs((s) => s.set);
  return [
    value,
    (v) => {
      const patch: Partial<UiPrefs> = {};
      patch[key] = v;
      set(patch);
    },
  ];
}
