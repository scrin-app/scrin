import { describe, expect, it } from 'vitest';

import { LOCALES } from '../types';
import { en } from './en';
import { ro } from './ro';

const TABLES: Record<(typeof LOCALES)[number], unknown> = { en, ro };

function leaves(obj: unknown, prefix = ''): Map<string, string> {
  const out = new Map<string, string>();
  if (typeof obj === 'string') {
    out.set(prefix, obj);
    return out;
  }
  if (typeof obj !== 'object' || obj === null) return out;
  for (const [k, v] of Object.entries(obj)) {
    for (const [kk, vv] of leaves(v, prefix ? `${prefix}.${k}` : k)) out.set(kk, vv);
  }
  return out;
}

function placeholders(s: string): string[] {
  return [...s.matchAll(/\{\{\s*(\w+)\s*\}\}/g)].map((m) => m[1] ?? '').sort();
}

const EN = leaves(en);

describe('locale drift', () => {
  it('has a meaningful number of keys', () => {
    // Guards the walker: a broken walker would make every test below vacuous.
    expect(EN.size).toBeGreaterThan(200);
  });

  for (const locale of LOCALES) {
    const table = leaves(TABLES[locale]);

    it(`${locale}: has exactly the en key set`, () => {
      const missing = [...EN.keys()].filter((k) => !table.has(k));
      const extra = [...table.keys()].filter((k) => !EN.has(k));
      expect({ missing, extra }).toEqual({ missing: [], extra: [] });
    });

    it(`${locale}: every value is non-empty and keeps the en placeholders`, () => {
      const bad: string[] = [];
      for (const [key, source] of EN) {
        const value = table.get(key) ?? '';
        if (value.trim() === '') bad.push(`${key}: empty`);
        else if (placeholders(value).join() !== placeholders(source).join())
          bad.push(`${key}: ${placeholders(value).join()} ≠ ${placeholders(source).join()}`);
      }
      expect(bad).toEqual([]);
    });
  }

  it('ro uses comma-below ș/ț, never the cedilla forms', () => {
    const cedilla = [...leaves(ro)].filter(([, v]) => /[şţŞŢ]/.test(v)).map(([k]) => k);
    expect(cedilla).toEqual([]);
  });

  it('ro is actually translated (not a copy of en)', () => {
    const same = [...leaves(ro)].filter(([k, v]) => EN.get(k) === v).length;
    // Brand names, codes and shortcuts legitimately stay identical.
    expect(same / EN.size).toBeLessThan(0.15);
  });
});
