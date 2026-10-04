import { and, eq } from 'drizzle-orm';
import { bootstrap, type Runtime } from '../bootstrap.ts';
import { bytesToHex, utf8 } from '../crypto.ts';
import { openDb } from '../db/client.ts';
import { member, organization } from '../db/schema/index.ts';
import { loadEnv } from '../env.ts';
import { createLogger } from '../logger.ts';
import type { MailMessage, Mailer } from '../mailer.ts';

export const ORIGIN = 'http://localhost:8787';

class CapturingMailer implements Mailer {
  readonly enabled = true;
  readonly sent: MailMessage[] = [];
  send(m: MailMessage): Promise<void> {
    this.sent.push(m);
    return Promise.resolve();
  }
}

export interface TestRuntime extends Runtime {
  mailer: CapturingMailer;
  request: (path: string, init?: RequestInit & { json?: unknown }) => Promise<Response>;
}

/** One isolated in-process Postgres (PGlite) with migrations applied. */
export async function createTestRuntime(extra: Record<string, string> = {}): Promise<TestRuntime> {
  const env = loadEnv({
    NODE_ENV: 'test',
    DATABASE_URL: 'pglite:memory',
    BETTER_AUTH_SECRET: 'test-secret-test-secret-test-secret-0123',
    PUBLIC_URL: ORIGIN,
    REQUIRE_EMAIL_VERIFICATION: 'false',
    WEBHOOK_ALLOW_INSECURE: 'true',
    LOG_LEVEL: 'silent',
    ...extra,
  });
  const mailer = new CapturingMailer();
  const handle = await openDb('pglite:memory');
  const rt = await bootstrap(env, {
    logger: createLogger('silent'),
    mailer,
    handle,
    migrate: true,
  });
  const request = (path: string, init: RequestInit & { json?: unknown } = {}) => {
    const headers = new Headers(init.headers);
    headers.set('origin', ORIGIN);
    let reqBody = init.body;
    if (init.json !== undefined) {
      headers.set('content-type', 'application/json');
      reqBody = JSON.stringify(init.json);
    }
    return Promise.resolve(
      rt.app.request(`${ORIGIN}${path}`, {
        ...init,
        headers,
        ...(reqBody === undefined ? {} : { body: reqBody }),
      }),
    );
  };
  return { ...rt, mailer, request };
}

export interface TestUser {
  id: string;
  email: string;
  cookie: string;
}

let seq = 0;

export async function signUp(rt: TestRuntime, name = 'user'): Promise<TestUser> {
  seq += 1;
  const email = `${name}-${seq}-${Date.now()}@example.test`;
  const res = await rt.request('/api/auth/sign-up/email', {
    method: 'POST',
    json: { email, password: 'correct horse battery staple', name },
  });
  if (res.status !== 200) throw new Error(`sign-up failed ${res.status}: ${await res.text()}`);
  const cookie = sessionCookie(res);
  const data = (await res.json()) as { user: { id: string } };
  return { id: data.user.id, email, cookie };
}

export function sessionCookie(res: Response): string {
  const cookies = res.headers.getSetCookie();
  const parts = cookies
    .map((c) => c.split(';')[0] ?? '')
    .filter((c) => c.includes('session_token'));
  if (parts.length === 0) throw new Error('no session cookie');
  return parts.join('; ');
}

/** Creates an organisation directly (bypassing the org plugin UI flow) and adds members. */
export async function createOrg(
  rt: TestRuntime,
  members: { user: TestUser; role: string }[],
): Promise<string> {
  seq += 1;
  const id = `org_test_${seq}_${Date.now()}`;
  await rt.deps.db.insert(organization).values({ id, name: `Org ${seq}`, slug: id });
  for (const m of members) {
    seq += 1;
    await rt.deps.db.insert(member).values({
      id: `mem_${seq}_${Date.now()}`,
      organizationId: id,
      userId: m.user.id,
      role: m.role,
    });
  }
  return id;
}

export async function setRole(
  rt: TestRuntime,
  orgId: string,
  user: TestUser,
  role: string,
): Promise<void> {
  await rt.deps.db
    .update(member)
    .set({ role })
    .where(and(eq(member.organizationId, orgId), eq(member.userId, user.id)));
}

export function as(user: TestUser, orgId: string): HeadersInit {
  return { cookie: user.cookie, 'x-scrin-org': orgId };
}

// ---- device keys ------------------------------------------------------------

export interface DeviceKey {
  pubHex: string;
  sign: (message: string) => Promise<string>;
}

export async function deviceKey(): Promise<DeviceKey> {
  const kp = await crypto.subtle.generateKey({ name: 'Ed25519' }, true, ['sign', 'verify']);
  const raw = new Uint8Array(await crypto.subtle.exportKey('raw', kp.publicKey));
  return {
    pubHex: bytesToHex(raw),
    sign: async (message) =>
      bytesToHex(
        new Uint8Array(await crypto.subtle.sign({ name: 'Ed25519' }, kp.privateKey, utf8(message))),
      ),
  };
}

export function scrinId(): string {
  return String(100_000_000 + Math.floor(Math.random() * 899_999_999));
}

export async function registerDevice(
  rt: TestRuntime,
  headers: HeadersInit,
  opts: { name?: string; key?: DeviceKey; groupId?: string } = {},
): Promise<{ id: string; key: DeviceKey; scrinId: string }> {
  const key = opts.key ?? (await deviceKey());
  const sid = scrinId();
  const chRes = await rt.request('/v1/devices/challenge', {
    method: 'POST',
    headers,
    json: { devicePub: key.pubHex, scrinId: sid },
  });
  if (chRes.status !== 201) throw new Error(`challenge ${chRes.status}: ${await chRes.text()}`);
  const ch = (await chRes.json()) as { challengeId: string; message: string };
  const res = await rt.request('/v1/devices', {
    method: 'POST',
    headers,
    json: {
      challengeId: ch.challengeId,
      signature: await key.sign(ch.message),
      name: opts.name ?? 'Workstation',
      platform: 'windows',
      ...(opts.groupId === undefined ? {} : { groupId: opts.groupId }),
    },
  });
  if (res.status !== 201) throw new Error(`register ${res.status}: ${await res.text()}`);
  const d = (await res.json()) as { id: string };
  return { id: d.id, key, scrinId: sid };
}
