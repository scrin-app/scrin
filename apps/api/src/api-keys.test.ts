import { eq } from 'drizzle-orm';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { sha256Hex } from './crypto.ts';
import { apiKey } from './db/schema/index.ts';
import {
  as,
  createOrg,
  createTestRuntime,
  registerDevice,
  setRole,
  signUp,
  type TestRuntime,
  type TestUser,
} from './test/harness.ts';

let rt: TestRuntime;
let owner: TestUser;
let org: string;

beforeAll(async () => {
  rt = await createTestRuntime();
  owner = await signUp(rt, 'owner');
  org = await createOrg(rt, [{ user: owner, role: 'owner' }]);
});
afterAll(async () => {
  await rt.handle.close();
});

async function createKey(user: TestUser, o: string, scopes: string[]) {
  const res = await rt.request('/v1/api-keys', {
    method: 'POST',
    headers: as(user, o),
    json: { name: 'ci', scopes },
  });
  return res;
}

const bearer = (token: string) => ({ authorization: `Bearer ${token}` });

describe('API keys', () => {
  it('creates a key shown once, stores only its SHA-256, and authenticates with it', async () => {
    const res = await createKey(owner, org, ['devices:read']);
    expect(res.status).toBe(201);
    const k = (await res.json()) as { id: string; token: string; prefix: string };
    expect(k.token).toMatch(/^sk_scrin_[0-9A-Za-z]{40}$/);
    expect(k.token.startsWith(k.prefix)).toBe(true);
    const [row] = await rt.deps.db.select().from(apiKey).where(eq(apiKey.publicId, k.id));
    expect(row?.hash).toBe(sha256Hex(k.token));
    expect(JSON.stringify(row)).not.toContain(k.token);

    const me = await rt.request('/v1/me', { headers: bearer(k.token) });
    expect(me.status).toBe(200);
    expect(await me.json()).toMatchObject({
      kind: 'api_key',
      orgId: org,
      apiKeyId: k.id,
      permissions: ['devices:read'],
    });
    const list = await rt.request('/v1/api-keys', { headers: as(owner, org) });
    expect(JSON.stringify(await list.json())).not.toContain(k.token);
  });

  it('enforces scopes', async () => {
    const k = (await (await createKey(owner, org, ['devices:read'])).json()) as { token: string };
    expect((await rt.request('/v1/devices', { headers: bearer(k.token) })).status).toBe(200);
    expect((await rt.request('/v1/audit/verify', { headers: bearer(k.token) })).status).toBe(403);
    const write = await rt.request('/v1/devices/challenge', {
      method: 'POST',
      headers: bearer(k.token),
      json: { devicePub: 'a'.repeat(64), scrinId: '123456789' },
    });
    expect(write.status).toBe(403);
  });

  it('a key with devices:write can register a device', async () => {
    const k = (await (await createKey(owner, org, ['devices:read', 'devices:write'])).json()) as {
      token: string;
    };
    const d = await registerDevice(rt, bearer(k.token));
    expect(d.id).toMatch(/^dev_/);
  });

  it('rejects unknown, revoked and malformed keys with 401', async () => {
    expect((await rt.request('/v1/me', { headers: bearer('sk_scrin_nope') })).status).toBe(401);
    const k = (await (await createKey(owner, org, ['devices:read'])).json()) as {
      id: string;
      token: string;
    };
    const del = await rt.request(`/v1/api-keys/${k.id}`, {
      method: 'DELETE',
      headers: as(owner, org),
    });
    expect(del.status).toBe(204);
    const after = await rt.request('/v1/me', { headers: bearer(k.token) });
    expect(after.status).toBe(401);
    expect(await after.json()).toEqual({
      error: { code: 'unauthorized', message: 'Invalid or expired API key' },
    });
  });

  it('keys cannot mint keys, and cannot exceed the creator', async () => {
    const k = (await (await createKey(owner, org, ['api_keys:manage', 'devices:read'])).json()) as {
      token: string;
    };
    const nested = await rt.request('/v1/api-keys', {
      method: 'POST',
      headers: bearer(k.token),
      json: { name: 'x', scopes: ['devices:read'] },
    });
    expect(nested.status).toBe(403);

    const admin = await signUp(rt, 'admin');
    const o = await createOrg(rt, [{ user: admin, role: 'admin' }]);
    const tooMuch = await createKey(admin, o, ['devices:read', 'groups:write', 'jit:approve']);
    expect(tooMuch.status).toBe(201);
    const ak = (await tooMuch.json()) as { token: string };
    // Demote the creator: the key loses what the creator lost.
    await setRole(rt, o, admin, 'viewer');
    const me = (await (await rt.request('/v1/me', { headers: bearer(ak.token) })).json()) as {
      permissions: string[];
    };
    expect(me.permissions).toEqual(['devices:read']);
  });
});
