import { readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import { en } from './en';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../../../..');

function leafKeys(obj: unknown, prefix = ''): string[] {
  if (typeof obj !== 'object' || obj === null) return [prefix];
  return Object.entries(obj).flatMap(([k, v]) => leafKeys(v, prefix ? `${prefix}.${k}` : k));
}

const EN_KEYS = new Set(leafKeys(en));

/** A group key (`t('sasEmoji', { returnObjects: true })`) names a non-empty group. */
function isGroup(k: string): boolean {
  return [...EN_KEYS].some((e) => e.startsWith(`${k}.`));
}

function sourceFiles(dir: string): string[] {
  let entries: string[];
  try {
    entries = readdirSync(dir);
  } catch {
    return [];
  }
  return entries.flatMap((name) => {
    if (name === 'node_modules' || name === 'dist') return [];
    const full = join(dir, name);
    if (statSync(full).isDirectory()) return sourceFiles(full);
    if (!/\.tsx?$/.test(name) || /\.(test|spec)\./.test(name) || name.endsWith('.gen.ts'))
      return [];
    return [full];
  });
}

const FILES = [join(ROOT, 'packages', 'ui', 'src'), join(ROOT, 'apps', 'web', 'src')].flatMap(
  sourceFiles,
);

interface Hit {
  key: string;
  file: string;
}

// `t('a.b')` — `\b` keeps `.at(` / `parseInt(` out (word char before `t`).
const LITERAL = /\bt\(\s*(['"])([A-Za-z][\w.-]*)\1/g;
// `labelKey: 'a.b'` style declarations in data tables.
const DECLARED = /\b\w*Key:\s*(['"])([a-z][\w]*\.[\w.]+)\1/g;
// `t(`theme.${x}`)` — only the static prefix is knowable.
const TEMPLATE = /\bt\(\s*`([A-Za-z][\w.-]*)\$\{/g;

const literal: Hit[] = [];
const prefixes: Hit[] = [];
for (const file of FILES) {
  const text = readFileSync(file, 'utf8');
  const rel = file.slice(ROOT.length + 1).replaceAll('\\', '/');
  for (const re of [LITERAL, DECLARED]) {
    for (const m of text.matchAll(re)) literal.push({ key: m[2] ?? '', file: rel });
  }
  for (const m of text.matchAll(TEMPLATE)) prefixes.push({ key: m[1] ?? '', file: rel });
}

describe('i18n call sites', () => {
  it('found call sites to check at all', () => {
    expect(FILES.length).toBeGreaterThan(30);
    expect(literal.length).toBeGreaterThan(150);
  });

  it('every t() literal key exists in en', () => {
    const missing = literal
      .filter((h) => !EN_KEYS.has(h.key) && !isGroup(h.key))
      .map((h) => `${h.key} (${h.file})`);
    expect([...new Set(missing)].sort()).toEqual([]);
  });

  it('every dynamic key prefix has at least one en key under it', () => {
    const orphan = prefixes
      .filter((h) => ![...EN_KEYS].some((k) => k.startsWith(h.key)))
      .map((h) => `${h.key}* (${h.file})`);
    expect([...new Set(orphan)].sort()).toEqual([]);
  });
});
