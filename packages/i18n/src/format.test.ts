import { describe, expect, it } from 'vitest';

import {
  formatBytes,
  formatClock,
  formatCode,
  formatDuration,
  formatNumber,
  formatRelative,
  formatScrinId,
} from './format';
import { detectLocale, resolveLocale, LOCALE_STORAGE_KEY } from './index';

describe('formatters', () => {
  it('groups numbers per locale', () => {
    expect(formatNumber('en', 1234.5)).toBe('1,234.5');
    // ro-RO groups with a dot and uses a decimal comma.
    expect(formatNumber('ro', 1234.5)).toBe('1.234,5');
  });

  it('scales bytes by 1024', () => {
    expect(formatBytes('en', 512)).toBe('512 byte');
    expect(formatBytes('en', 1536)).toBe('1.5 kB');
    expect(formatBytes('en', 5 * 1024 ** 3)).toBe('5 GB');
  });

  it('formats a countdown clock', () => {
    expect(formatClock(545)).toBe('9:05');
    expect(formatClock(3723)).toBe('1:02:03');
    expect(formatClock(-3)).toBe('0:00');
  });

  it('formats durations with unit names', () => {
    expect(formatDuration('en', 3900)).toBe('1 hr 5 mins');
    expect(formatDuration('en', 60)).toBe('1 min');
    expect(formatDuration('en', 0)).toBe('0 secs');
  });

  it('formats relative time in both locales', () => {
    const now = Date.UTC(2026, 9, 4, 12);
    expect(formatRelative('en', now - 3 * 60_000, now)).toBe('3 minutes ago');
    expect(formatRelative('ro', now - 3 * 60_000, now)).toBe('acum 3 minute');
    expect(formatRelative('en', now - 86_400_000, now)).toBe('yesterday');
  });

  it('groups scrin IDs and codes as typed', () => {
    expect(formatScrinId('123456789')).toBe('123 456 789');
    expect(formatScrinId('12345')).toBe('123 45');
    expect(formatScrinId('12a3 4567 8999')).toBe('123 456 789');
    expect(formatCode('abcd2345')).toBe('ABCD 2345');
    expect(formatCode('ab')).toBe('AB');
  });
});

describe('locale detection', () => {
  it('resolves the first supported base tag', () => {
    expect(resolveLocale(['fr-FR', 'ro-RO', 'en'])).toBe('ro');
    expect(resolveLocale(['de'])).toBe('en');
  });

  it('prefers a stored choice', () => {
    const storage = {
      get: (k: string) => (k === LOCALE_STORAGE_KEY ? 'ro' : null),
      set: () => undefined,
    };
    expect(detectLocale(storage)).toBe('ro');
  });

  it('ignores a corrupt stored value', () => {
    const storage = { get: () => 'klingon', set: () => undefined };
    expect(['en', 'ro']).toContain(detectLocale(storage));
  });
});
