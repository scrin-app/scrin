/**
 * Tiny search-param validators. Route files are part of the entry chunk, so
 * they deliberately avoid zod (23 KB gzip) — each one is a few lines.
 */
const SCRIN_ID = /^\d{9}$/;

function str(v: unknown, max: number): string | undefined {
  return typeof v === 'string' && v.length <= max ? v : undefined;
}

export function isScrinId(v: unknown): v is string {
  return typeof v === 'string' && SCRIN_ID.test(v);
}

export function homeSearch(raw: Record<string, unknown>): { id?: string } {
  const id = raw.id;
  // A numeric-looking query value may be parsed as a number by the router.
  const s = typeof id === 'number' ? String(id) : id;
  return isScrinId(s) ? { id: s } : {};
}

export function sessionSearch(raw: Record<string, unknown>): { s?: string } {
  const s = str(raw.s, 64);
  return s ? { s } : {};
}

export type DevicesView = 'grid' | 'list';

export function devicesSearch(raw: Record<string, unknown>): {
  q?: string | undefined;
  group?: string | undefined;
  view?: DevicesView | undefined;
} {
  const out: { q?: string; group?: string; view?: DevicesView } = {};
  const q = str(raw.q, 100);
  const group = str(raw.group, 40);
  if (q) out.q = q;
  if (group) out.group = group;
  if (raw.view === 'grid' || raw.view === 'list') out.view = raw.view;
  return out;
}
