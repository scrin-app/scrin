import { LOCALE_TAGS, type Locale } from './types';

/**
 * Locale-aware formatters on top of `Intl`. Formatter construction is
 * expensive relative to formatting, so instances are cached per locale and
 * option set.
 */
function memo<T>(cache: Map<string, T>, key: string, make: () => T): T {
  const hit = cache.get(key);
  if (hit !== undefined) return hit;
  const made = make();
  cache.set(key, made);
  return made;
}

const numberCache = new Map<string, Intl.NumberFormat>();
const dateCache = new Map<string, Intl.DateTimeFormat>();
const relativeCache = new Map<string, Intl.RelativeTimeFormat>();

export function formatNumber(locale: Locale, value: number, opts: Intl.NumberFormatOptions = {}) {
  const tag = LOCALE_TAGS[locale];
  return memo(
    numberCache,
    `${tag}|${JSON.stringify(opts)}`,
    () => new Intl.NumberFormat(tag, opts),
  ).format(value);
}

export function formatPercent(locale: Locale, ratio: number, fractionDigits = 1) {
  return formatNumber(locale, ratio, {
    style: 'percent',
    maximumFractionDigits: fractionDigits,
  });
}

export function formatDate(
  locale: Locale,
  date: Date | number,
  opts: Intl.DateTimeFormatOptions = { dateStyle: 'medium' },
) {
  const tag = LOCALE_TAGS[locale];
  return memo(
    dateCache,
    `${tag}|${JSON.stringify(opts)}`,
    () => new Intl.DateTimeFormat(tag, opts),
  ).format(date);
}

const BYTE_UNITS = ['byte', 'kilobyte', 'megabyte', 'gigabyte', 'terabyte'] as const;

/** Binary-scaled (1024) byte count with the locale's unit names, e.g. `1.5 MB`. */
export function formatBytes(locale: Locale, bytes: number) {
  let value = Math.abs(bytes);
  let unit = 0;
  while (value >= 1024 && unit < BYTE_UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return formatNumber(locale, Math.sign(bytes) * value, {
    style: 'unit',
    unit: BYTE_UNITS[unit],
    unitDisplay: 'short',
    maximumFractionDigits: unit === 0 ? 0 : 1,
  });
}

/** Bits per second, e.g. `12.4 Mb/s`. */
export function formatBitrate(locale: Locale, bitsPerSecond: number) {
  const mbps = bitsPerSecond / 1_000_000;
  return formatNumber(locale, mbps, {
    style: 'unit',
    unit: 'megabit-per-second',
    unitDisplay: 'short',
    maximumFractionDigits: mbps < 10 ? 1 : 0,
  });
}

/** A countdown such as `9:05` (m:ss) or `1:02:03` for long spans. */
export function formatClock(totalSeconds: number) {
  const s = Math.max(0, Math.floor(totalSeconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`;
}

/** A human duration such as `2 h 5 min` using the locale's unit names. */
export function formatDuration(locale: Locale, totalSeconds: number) {
  const s = Math.max(0, Math.round(totalSeconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const part = (v: number, unit: 'hour' | 'minute' | 'second') =>
    formatNumber(locale, v, { style: 'unit', unit, unitDisplay: 'short' });
  const parts: string[] = [];
  if (h > 0) parts.push(part(h, 'hour'));
  if (m > 0) parts.push(part(m, 'minute'));
  if (sec > 0 || parts.length === 0) parts.push(part(sec, 'second'));
  return parts.join(' ');
}

const RELATIVE_STEPS: [Intl.RelativeTimeFormatUnit, number][] = [
  ['second', 60],
  ['minute', 60],
  ['hour', 24],
  ['day', 7],
  ['week', 4.348],
  ['month', 12],
  ['year', Number.POSITIVE_INFINITY],
];

/** `3 minutes ago` / `acum 3 minute`, relative to `now`. */
export function formatRelative(
  locale: Locale,
  date: Date | number,
  now: Date | number = Date.now(),
) {
  const tag = LOCALE_TAGS[locale];
  const rtf = memo(
    relativeCache,
    tag,
    () => new Intl.RelativeTimeFormat(tag, { numeric: 'auto', style: 'long' }),
  );
  let delta = (Number(date) - Number(now)) / 1000;
  for (const [unit, size] of RELATIVE_STEPS) {
    if (Math.abs(delta) < size) return rtf.format(Math.round(delta), unit);
    delta /= size;
  }
  return rtf.format(Math.round(delta), 'year');
}

/** `123456789` → `123 456 789`; partial input is grouped as typed. */
export function formatScrinId(id: string) {
  return id
    .replace(/\D/g, '')
    .slice(0, 9)
    .replace(/(\d{3})(?=\d)/g, '$1 ');
}

/** `ABCD2345` → `ABCD 2345`. */
export function formatCode(code: string) {
  return code
    .toUpperCase()
    .replace(/[^A-Z0-9]/g, '')
    .slice(0, 8)
    .replace(/^(.{4})(?=.)/, '$1 ');
}
