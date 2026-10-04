import * as z from 'zod/mini';
import { create } from 'zustand';

import { host } from '../host';
import { DEVICES, TRUSTED, type TrustedDevice } from './mock-data';

const PREFS_KEY = 'scrin.prefs';
const RECENT_KEY = 'scrin.recent';

// Every field falls back on its own, so one corrupt value never resets the rest.
const bool = (d: boolean) => z.catch(z.boolean(), d);
const num = (min: number, max: number, d: number) =>
  z.catch(z.number().check(z.gte(min), z.lte(max)), d);
const text = (max: number, d: string) => z.catch(z.string().check(z.maxLength(max)), d);

const prefsSchema = z.object({
  deviceName: text(64, 'My browser'),
  startWithSystem: bool(true),
  minimizeToTray: bool(true),
  unattended: bool(false),
  relayUrl: text(256, ''),
  directConnections: bool(true),
  codec: z.catch(z.enum(['auto', 'av1', 'hevc', 'h264']), 'auto'),
  maxFps: z.catch(z.enum(['30', '60', '120', '144', '240']), '60'),
  hardwareDecode: bool(true),
  audio: bool(true),
  microphone: bool(false),
  volume: num(0, 100, 80),
  keyboardMode: z.catch(z.enum(['scancode', 'translate']), 'scancode'),
  relativeMouse: bool(false),
  scrollSpeed: num(1, 10, 5),
});

export type Prefs = z.infer<typeof prefsSchema>;

const recentSchema = z.catch(
  z
    .array(z.object({ id: z.string().check(z.regex(/^\d{9}$/)), name: z.string(), at: z.number() }))
    .check(z.maxLength(200)),
  [],
);

export type RecentConnection = z.infer<typeof recentSchema>[number];

function load<T>(key: string, schema: z.ZodMiniType<T>, fallback: unknown): T {
  const raw = host.storage.get(key);
  let parsed: unknown = fallback;
  if (raw) {
    try {
      parsed = JSON.parse(raw);
    } catch {
      parsed = fallback;
    }
  }
  return schema.parse(parsed);
}

const SEED_RECENT: RecentConnection[] = DEVICES.slice(0, 24).map((d) => ({
  id: d.id,
  name: d.name,
  at: d.lastSeen,
}));

interface PrefsState {
  prefs: Prefs;
  recent: RecentConnection[];
  trusted: TrustedDevice[];
  set: (patch: Partial<Prefs>) => void;
  addRecent: (entry: RecentConnection) => void;
  revokeTrusted: (id: string) => void;
}

export const usePrefs = create<PrefsState>()((set) => ({
  prefs: load(PREFS_KEY, prefsSchema, {}),
  recent: load(RECENT_KEY, recentSchema, SEED_RECENT),
  trusted: [...TRUSTED],
  set: (patch) => {
    set((s) => {
      const prefs = { ...s.prefs, ...patch };
      host.storage.set(PREFS_KEY, JSON.stringify(prefs));
      return { prefs };
    });
  },
  addRecent: (entry) => {
    set((s) => {
      const recent = [entry, ...s.recent.filter((r) => r.id !== entry.id)].slice(0, 200);
      host.storage.set(RECENT_KEY, JSON.stringify(recent));
      return { recent };
    });
  },
  revokeTrusted: (id) => {
    set((s) => ({ trusted: s.trusted.filter((d) => d.id !== id) }));
  },
}));

/**
 * The one-time code for an outgoing connection. Held in memory only — never
 * in the URL, history or storage (invariant 2: never log secrets).
 */
interface PendingState {
  code: string | null;
  setCode: (code: string | null) => void;
}

export const usePending = create<PendingState>()((set) => ({
  code: null,
  setCode: (code) => {
    set({ code });
  },
}));
