/**
 * `KeyboardEvent.code` (physical key, UI Events KeyboardEvent code values) →
 * USB HID usage, as `scrin.v1.KeyEvent.hid_usage` expects: page 0x07
 * (keyboard) usages are plain; other pages carry the page in the high 16 bits
 * (0x000C_xxxx = consumer page).
 */

const KEYBOARD: Record<string, number> = {
  Enter: 0x28,
  Escape: 0x29,
  Backspace: 0x2a,
  Tab: 0x2b,
  Space: 0x2c,
  Minus: 0x2d,
  Equal: 0x2e,
  BracketLeft: 0x2f,
  BracketRight: 0x30,
  Backslash: 0x31,
  Semicolon: 0x33,
  Quote: 0x34,
  Backquote: 0x35,
  Comma: 0x36,
  Period: 0x37,
  Slash: 0x38,
  CapsLock: 0x39,
  PrintScreen: 0x46,
  ScrollLock: 0x47,
  Pause: 0x48,
  Insert: 0x49,
  Home: 0x4a,
  PageUp: 0x4b,
  Delete: 0x4c,
  End: 0x4d,
  PageDown: 0x4e,
  ArrowRight: 0x4f,
  ArrowLeft: 0x50,
  ArrowDown: 0x51,
  ArrowUp: 0x52,
  NumLock: 0x53,
  NumpadDivide: 0x54,
  NumpadMultiply: 0x55,
  NumpadSubtract: 0x56,
  NumpadAdd: 0x57,
  NumpadEnter: 0x58,
  Numpad0: 0x62,
  NumpadDecimal: 0x63,
  IntlBackslash: 0x64,
  ContextMenu: 0x65,
  Power: 0x66,
  NumpadEqual: 0x67,
  Help: 0x75,
  NumpadComma: 0x85,
  IntlRo: 0x87,
  KanaMode: 0x88,
  IntlYen: 0x89,
  Convert: 0x8a,
  NonConvert: 0x8b,
  Lang1: 0x90,
  Lang2: 0x91,
  ControlLeft: 0xe0,
  ShiftLeft: 0xe1,
  AltLeft: 0xe2,
  MetaLeft: 0xe3,
  ControlRight: 0xe4,
  ShiftRight: 0xe5,
  AltRight: 0xe6,
  MetaRight: 0xe7,
  // Legacy names (Firefox < 118) for the Windows/Command keys.
  OSLeft: 0xe3,
  OSRight: 0xe7,
};

const CONSUMER = 0x000c_0000;
const MEDIA: Record<string, number> = {
  AudioVolumeMute: CONSUMER | 0xe2,
  AudioVolumeUp: CONSUMER | 0xe9,
  AudioVolumeDown: CONSUMER | 0xea,
  MediaPlayPause: CONSUMER | 0xcd,
  MediaStop: CONSUMER | 0xb7,
  MediaTrackNext: CONSUMER | 0xb5,
  MediaTrackPrevious: CONSUMER | 0xb6,
  BrowserSearch: CONSUMER | 0x221,
  BrowserHome: CONSUMER | 0x223,
  BrowserBack: CONSUMER | 0x224,
  BrowserForward: CONSUMER | 0x225,
  BrowserRefresh: CONSUMER | 0x227,
  LaunchMail: CONSUMER | 0x18a,
  LaunchApp2: CONSUMER | 0x192,
};

/** HID usage for a `KeyboardEvent.code`, or `null` for keys with no mapping. */
export function hidUsage(code: string): number | null {
  const m = /^Key([A-Z])$/.exec(code);
  if (m?.[1]) return 0x04 + m[1].charCodeAt(0) - 65;
  const d = /^Digit([0-9])$/.exec(code);
  if (d?.[1]) return d[1] === '0' ? 0x27 : 0x1e + Number(d[1]) - 1;
  const np = /^Numpad([1-9])$/.exec(code);
  if (np?.[1]) return 0x59 + Number(np[1]) - 1;
  const f = /^F([0-9]{1,2})$/.exec(code);
  if (f?.[1]) {
    const n = Number(f[1]);
    if (n >= 1 && n <= 12) return 0x3a + n - 1;
    if (n >= 13 && n <= 24) return 0x68 + n - 13;
    return null;
  }
  return KEYBOARD[code] ?? MEDIA[code] ?? null;
}

/** `scrin.v1.KeyEvent.modifiers` bits. */
export const MOD = {
  shift: 1,
  ctrl: 2,
  alt: 4,
  meta: 8,
  capsLock: 16,
  numLock: 32,
  altGr: 64,
} as const;

interface ModifierSource {
  shiftKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  metaKey: boolean;
  getModifierState(key: string): boolean;
}

export function modifiers(e: ModifierSource): number {
  let m = 0;
  if (e.shiftKey) m |= MOD.shift;
  if (e.ctrlKey) m |= MOD.ctrl;
  if (e.altKey) m |= MOD.alt;
  if (e.metaKey) m |= MOD.meta;
  if (e.getModifierState('CapsLock')) m |= MOD.capsLock;
  if (e.getModifierState('NumLock')) m |= MOD.numLock;
  if (e.getModifierState('AltGraph')) m |= MOD.altGr;
  return m;
}

/** A key press (`down`) or release on the remote, in order. */
export interface KeyStroke {
  usage: number;
  down: boolean;
}

const tap = (...usages: number[]): KeyStroke[] => [
  ...usages.map((usage) => ({ usage, down: true })),
  ...usages.toReversed().map((usage) => ({ usage, down: false })),
];

/** Key sequences of the toolbar's special-key menu (`SpecialKey`). */
export const SPECIAL_KEYS = {
  'ctrl-alt-del': tap(0xe0, 0xe2, 0x4c),
  win: tap(0xe3),
  'alt-tab': tap(0xe2, 0x2b),
  'print-screen': tap(0x46),
  lock: tap(0xe3, 0x0f),
} as const satisfies Record<string, readonly KeyStroke[]>;
