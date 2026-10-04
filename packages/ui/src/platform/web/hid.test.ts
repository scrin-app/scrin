import { describe, expect, it } from 'vitest';

import { hidUsage, MOD, modifiers, SPECIAL_KEYS } from './hid';

describe('KeyboardEvent.code → HID usage', () => {
  it('maps letters, digits and function keys by range', () => {
    expect(hidUsage('KeyA')).toBe(0x04);
    expect(hidUsage('KeyZ')).toBe(0x1d);
    expect(hidUsage('Digit1')).toBe(0x1e);
    expect(hidUsage('Digit9')).toBe(0x26);
    expect(hidUsage('Digit0')).toBe(0x27);
    expect(hidUsage('F1')).toBe(0x3a);
    expect(hidUsage('F12')).toBe(0x45);
    expect(hidUsage('F13')).toBe(0x68);
    expect(hidUsage('F24')).toBe(0x73);
    expect(hidUsage('F25')).toBeNull();
    expect(hidUsage('Numpad1')).toBe(0x59);
    expect(hidUsage('Numpad9')).toBe(0x61);
    expect(hidUsage('Numpad0')).toBe(0x62);
  });

  it('maps editing, navigation, modifier and international keys', () => {
    const cases: [string, number][] = [
      ['Enter', 0x28],
      ['Escape', 0x29],
      ['Backspace', 0x2a],
      ['Tab', 0x2b],
      ['Space', 0x2c],
      ['Backquote', 0x35],
      ['CapsLock', 0x39],
      ['PrintScreen', 0x46],
      ['Delete', 0x4c],
      ['ArrowUp', 0x52],
      ['NumpadEnter', 0x58],
      ['IntlBackslash', 0x64],
      ['ContextMenu', 0x65],
      ['IntlRo', 0x87],
      ['IntlYen', 0x89],
      ['ControlLeft', 0xe0],
      ['ShiftLeft', 0xe1],
      ['AltLeft', 0xe2],
      ['MetaLeft', 0xe3],
      ['OSLeft', 0xe3],
      ['AltRight', 0xe6],
      ['MetaRight', 0xe7],
    ];
    for (const [code, usage] of cases) expect(hidUsage(code), code).toBe(usage);
  });

  it('puts media keys on the consumer page', () => {
    expect(hidUsage('AudioVolumeUp')).toBe(0x000c_00e9);
    expect(hidUsage('MediaPlayPause')).toBe(0x000c_00cd);
  });

  it('returns null for unknown or empty codes', () => {
    expect(hidUsage('')).toBeNull();
    expect(hidUsage('Unidentified')).toBeNull();
    expect(hidUsage('Keya')).toBeNull();
  });

  it('every usage in the table is unique except legacy aliases', () => {
    const codes = [
      ...'ABCDEFGHIJKLMNOPQRSTUVWXYZ'.split('').map((c) => `Key${c}`),
      ...'0123456789'.split('').map((d) => `Digit${d}`),
      ...Array.from({ length: 24 }, (_, i) => `F${i + 1}`),
    ];
    const usages = codes.map(hidUsage);
    expect(new Set(usages).size).toBe(codes.length);
    expect(usages.every((u) => u !== null)).toBe(true);
  });
});

describe('modifier bits', () => {
  it('combines the flags', () => {
    const e = {
      shiftKey: true,
      ctrlKey: true,
      altKey: false,
      metaKey: true,
      getModifierState: (k: string) => k === 'CapsLock',
    };
    expect(modifiers(e)).toBe(MOD.shift | MOD.ctrl | MOD.meta | MOD.capsLock);
  });
});

describe('special key sequences', () => {
  it('press in order and release in reverse', () => {
    expect(SPECIAL_KEYS['ctrl-alt-del']).toEqual([
      { usage: 0xe0, down: true },
      { usage: 0xe2, down: true },
      { usage: 0x4c, down: true },
      { usage: 0x4c, down: false },
      { usage: 0xe2, down: false },
      { usage: 0xe0, down: false },
    ]);
    for (const seq of Object.values(SPECIAL_KEYS)) {
      const downs = seq.filter((k) => k.down).length;
      expect(downs * 2).toBe(seq.length);
    }
  });
});
