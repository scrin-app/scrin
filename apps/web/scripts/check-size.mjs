// Enforces the gzip budgets in /size-budget.json against apps/web/dist.
//
// "Initial JS" = the entry chunk plus every chunk it statically imports
// (transitively), i.e. what the browser must download before first render.
// Route chunks loaded through dynamic import() are measured separately.
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';

const here = dirname(fileURLToPath(import.meta.url));
const app = resolve(here, '..');
const dist = join(app, 'dist');
const budget = JSON.parse(readFileSync(resolve(app, '../../size-budget.json'), 'utf8')).web;

if (!existsSync(dist)) {
  console.error('dist/ not found — run `pnpm --filter web build` first.');
  process.exit(1);
}

const html = readFileSync(join(dist, 'index.html'), 'utf8');
const assets = join(dist, 'assets');
const files = readdirSync(assets);
const gz = (file) => gzipSync(readFileSync(join(assets, file)), { level: 9 }).length;
const kb = (n) => `${(n / 1024).toFixed(1)} KB`;

const entry = [...html.matchAll(/<script[^>]+src="\/assets\/([^"]+\.js)"/g)].map((m) => m[1]);
const preloads = [
  ...html.matchAll(/<link[^>]+rel="modulepreload"[^>]+href="\/assets\/([^"]+\.js)"/g),
].map((m) => m[1]);
const css = [...html.matchAll(/<link[^>]+rel="stylesheet"[^>]+href="\/assets\/([^"]+\.css)"/g)].map(
  (m) => m[1],
);

// Walk static imports so nothing the entry needs synchronously is missed.
const initial = new Set([...entry, ...preloads]);
const queue = [...initial];
while (queue.length > 0) {
  const file = queue.pop();
  const src = readFileSync(join(assets, file), 'utf8');
  for (const m of src.matchAll(
    /(?:^|[;\n}])\s*import\s*(?:[\w*{}\s,$]+from\s*)?["']\.\/([^"']+\.js)["']/g,
  )) {
    if (!initial.has(m[1])) {
      initial.add(m[1]);
      queue.push(m[1]);
    }
  }
}

const initialJs = [...initial].reduce((sum, f) => sum + gz(f), 0);
const initialCss = css.reduce((sum, f) => sum + gz(f), 0);
const lazy = files.filter((f) => f.endsWith('.js') && !initial.has(f));
const lazySizes = lazy.map((f) => ({ f, size: gz(f) })).sort((a, b) => b.size - a.size);
const largestLazy = lazySizes[0]?.size ?? 0;

const rows = [
  ['initial JS (gzip)', initialJs, budget.initialJsGzip],
  ['initial CSS (gzip)', initialCss, budget.initialCssGzip],
  ['largest route chunk (gzip)', largestLazy, budget.largestRouteChunkGzip],
];

console.log(`initial chunks (${initial.size}):`);
for (const f of [...initial].sort((a, b) => gz(b) - gz(a)))
  console.log(`  ${f.padEnd(48)} ${kb(gz(f))}`);
console.log(`lazy chunks (${lazy.length}):`);
for (const { f, size } of lazySizes) console.log(`  ${f.padEnd(48)} ${kb(size)}`);
console.log('');
let failed = false;
for (const [name, size, limit] of rows) {
  const ok = size <= limit;
  if (!ok) failed = true;
  console.log(
    `${ok ? 'PASS' : 'FAIL'}  ${name.padEnd(28)} ${kb(size).padStart(10)} / ${kb(limit)}`,
  );
}
process.exit(failed ? 1 : 0);
