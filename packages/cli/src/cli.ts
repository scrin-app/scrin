import { parseArgs } from 'node:util';
import { createScrinClient, ScrinApiError, unwrap, type ScrinClient } from '@scrin/sdk';
import { configPath, DEFAULT_BASE_URL, readConfig, writeConfig } from './config.ts';

export interface Io {
  out: (s: string) => void;
  err: (s: string) => void;
  env: NodeJS.ProcessEnv;
  fetch?: typeof fetch;
}

export const USAGE = `scrin — command-line client for the scrin accounts API

Usage:
  scrin login --api-key <sk_scrin_…> [--base-url <url>]
  scrin whoami
  scrin devices list [--group <id>] [--tag <tag>] [--search <text>] [--limit <n>]
  scrin devices show <device-id>
  scrin groups list
  scrin audit verify
  scrin jit request <device-id> --reason <text> [--minutes <n>]
  scrin jit approve <grant-id> [--note <text>]
  scrin jit deny <grant-id> [--note <text>]
  scrin jit list [--status pending|approved|denied|expired]

Global options:
  --json             machine-readable output
  --base-url <url>   API origin (env SCRIN_API_URL, default ${DEFAULT_BASE_URL})
  --api-key <key>    API key (env SCRIN_API_KEY, or saved by \`scrin login\`)
  -h, --help         this help

Exit codes: 0 ok · 1 API or usage error · 2 audit chain broken`;

const OPTIONS = {
  json: { type: 'boolean' },
  help: { type: 'boolean', short: 'h' },
  'base-url': { type: 'string' },
  'api-key': { type: 'string' },
  group: { type: 'string' },
  tag: { type: 'string' },
  search: { type: 'string' },
  limit: { type: 'string' },
  reason: { type: 'string' },
  minutes: { type: 'string' },
  note: { type: 'string' },
  status: { type: 'string' },
} as const;

class UsageError extends Error {}

function table(rows: string[][]): string {
  const widths = rows[0]?.map((_, i) => Math.max(...rows.map((r) => (r[i] ?? '').length))) ?? [];
  return rows
    .map((r) =>
      r
        .map((c, i) => c.padEnd(widths[i] ?? 0))
        .join('  ')
        .trimEnd(),
    )
    .join('\n');
}

function intOption(v: string | undefined, name: string, fallback: number): number {
  if (v === undefined) return fallback;
  const n = Number.parseInt(v, 10);
  if (!Number.isInteger(n) || n <= 0) throw new UsageError(`--${name} must be a positive integer`);
  return n;
}

const JIT_STATUSES = ['pending', 'approved', 'denied', 'expired'] as const;
type JitStatus = (typeof JIT_STATUSES)[number];
const isJitStatus = (s: string): s is JitStatus => (JIT_STATUSES as readonly string[]).includes(s);

/** Runs one CLI invocation. Returns the process exit code. */
export async function run(argv: string[], io: Io): Promise<number> {
  let parsed;
  try {
    parsed = parseArgs({ args: argv, options: OPTIONS, allowPositionals: true, strict: true });
  } catch (e) {
    io.err(`${e instanceof Error ? e.message : String(e)}\n\n${USAGE}\n`);
    return 1;
  }
  const { values: v, positionals: pos } = parsed;
  const asJson = v.json === true;
  const print = (data: unknown, human: () => string) => {
    io.out(`${asJson ? JSON.stringify(data, null, 2) : human()}\n`);
  };

  if (v.help === true || pos.length === 0) {
    io.out(`${USAGE}\n`);
    return 0;
  }

  const cfgFile = configPath(io.env);
  const saved = await readConfig(cfgFile);
  const baseUrl = v['base-url'] ?? io.env.SCRIN_API_URL ?? saved.baseUrl ?? DEFAULT_BASE_URL;
  const apiKey = v['api-key'] ?? io.env.SCRIN_API_KEY ?? saved.apiKey;
  const [cmd, sub, arg] = pos;

  const client = (): ScrinClient => {
    if (apiKey === undefined)
      throw new UsageError('Not logged in: run `scrin login --api-key <key>`');
    return createScrinClient({
      baseUrl,
      apiKey,
      ...(io.fetch === undefined ? {} : { fetch: io.fetch }),
    });
  };

  try {
    switch (`${cmd ?? ''} ${sub ?? ''}`.trim()) {
      case 'login': {
        const key = v['api-key'];
        if (key?.startsWith('sk_scrin_') !== true)
          throw new UsageError('login needs --api-key sk_scrin_…');
        const me = await unwrap(
          createScrinClient({
            baseUrl,
            apiKey: key,
            ...(io.fetch === undefined ? {} : { fetch: io.fetch }),
          }).GET('/v1/me'),
        );
        await writeConfig(cfgFile, { baseUrl, apiKey: key });
        print(
          { ok: true, orgId: me.orgId, config: cfgFile },
          () => `Logged in to ${baseUrl} (org ${me.orgId}).`,
        );
        return 0;
      }
      case 'whoami': {
        const me = await unwrap(client().GET('/v1/me'));
        print(
          me,
          () =>
            `${me.kind} ${me.apiKeyId ?? me.userId} in org ${me.orgId}\n${me.permissions.join(', ')}`,
        );
        return 0;
      }
      case 'devices list': {
        const res = await unwrap(
          client().GET('/v1/devices', {
            params: {
              query: {
                limit: intOption(v.limit, 'limit', 50),
                offset: 0,
                ...(v.group === undefined ? {} : { group: v.group }),
                ...(v.tag === undefined ? {} : { tag: v.tag }),
                ...(v.search === undefined ? {} : { q: v.search }),
              },
            },
          }),
        );
        print(res, () =>
          res.items.length === 0
            ? 'No devices.'
            : table([
                ['ID', 'SCRIN ID', 'NAME', 'PLATFORM', 'LAST SEEN'],
                ...res.items.map((d) => [d.id, d.scrinId, d.name, d.platform, d.lastSeenAt ?? '-']),
              ]) + `\n${res.items.length} of ${res.total}`,
        );
        return 0;
      }
      case 'devices show': {
        if (arg === undefined) throw new UsageError('devices show <device-id>');
        const d = await unwrap(client().GET('/v1/devices/{id}', { params: { path: { id: arg } } }));
        print(d, () =>
          table([
            ['id', d.id],
            ['scrin id', d.scrinId],
            ['name', d.name],
            ['platform', d.platform],
            ['group', d.groupId ?? '-'],
            ['tags', d.tags.join(', ') || '-'],
            ['public key', d.devicePub],
            ['last seen', d.lastSeenAt ?? '-'],
            ['registered', d.createdAt],
          ]),
        );
        return 0;
      }
      case 'groups list': {
        const res = await unwrap(client().GET('/v1/groups'));
        print(res, () =>
          res.items.length === 0
            ? 'No groups.'
            : table([
                ['ID', 'NAME', 'DEVICES'],
                ...res.items.map((g) => [g.id, g.name, String(g.deviceCount)]),
              ]),
        );
        return 0;
      }
      case 'audit verify': {
        const r = await unwrap(client().GET('/v1/audit/verify'));
        print(r, () =>
          r.valid
            ? `Audit chain valid: ${r.count} events, head ${r.headHash.slice(0, 16)}…`
            : `AUDIT CHAIN BROKEN at seq ${r.brokenAt?.seq ?? '?'} (${r.brokenAt?.id ?? '?'}): ${r.brokenAt?.reason ?? '?'}`,
        );
        return r.valid ? 0 : 2;
      }
      case 'jit request': {
        if (arg === undefined || v.reason === undefined) {
          throw new UsageError('jit request <device-id> --reason <text> [--minutes <n>]');
        }
        const g = await unwrap(
          client().POST('/v1/jit', {
            body: {
              deviceId: arg,
              reason: v.reason,
              durationMinutes: intOption(v.minutes, 'minutes', 60),
            },
          }),
        );
        print(g, () => `Requested ${g.id} (${g.status}) ${g.windowStart} → ${g.windowEnd}`);
        return 0;
      }
      case 'jit approve':
      case 'jit deny': {
        if (arg === undefined) throw new UsageError(`jit ${sub ?? ''} <grant-id> [--note <text>]`);
        const path = sub === 'approve' ? '/v1/jit/{id}/approve' : '/v1/jit/{id}/deny';
        const g = await unwrap(
          client().POST(path, {
            params: { path: { id: arg } },
            body: v.note === undefined ? {} : { note: v.note },
          }),
        );
        print(g, () => `${g.id}: ${g.status}`);
        return 0;
      }
      case 'jit list': {
        const status = v.status;
        if (status !== undefined && !isJitStatus(status)) {
          throw new UsageError(`--status must be one of ${JIT_STATUSES.join(', ')}`);
        }
        const res = await unwrap(
          client().GET('/v1/jit', {
            params: {
              query: {
                limit: intOption(v.limit, 'limit', 50),
                ...(status === undefined ? {} : { status }),
              },
            },
          }),
        );
        print(res, () =>
          res.items.length === 0
            ? 'No grants.'
            : table([
                ['ID', 'DEVICE', 'STATUS', 'WINDOW END', 'REASON'],
                ...res.items.map((g) => [g.id, g.deviceId, g.status, g.windowEnd, g.reason]),
              ]),
        );
        return 0;
      }
      default:
        throw new UsageError(`Unknown command: ${pos.join(' ')}`);
    }
  } catch (e) {
    if (e instanceof UsageError) {
      io.err(`${e.message}\n`);
      return 1;
    }
    if (e instanceof ScrinApiError) {
      if (asJson)
        io.err(
          `${JSON.stringify({ error: { status: e.status, code: e.code, message: e.message } })}\n`,
        );
      else io.err(`error ${e.status} ${e.code}: ${e.message}\n`);
      return 1;
    }
    io.err(`error: ${e instanceof Error ? e.message : String(e)}\n`);
    return 1;
  }
}
