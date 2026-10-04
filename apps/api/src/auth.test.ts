import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createTestRuntime, ORIGIN, sessionCookie, type TestRuntime } from './test/harness.ts';

let rt: TestRuntime;
beforeAll(async () => {
  rt = await createTestRuntime();
});
afterAll(async () => {
  await rt.handle.close();
});

describe('better-auth email + password', () => {
  const email = `alice-${Date.now()}@example.test`;
  const password = 'correct horse battery staple';

  it('signs up, returns a session cookie, and /get-session sees the user', async () => {
    const res = await rt.request('/api/auth/sign-up/email', {
      method: 'POST',
      json: { email, password, name: 'Alice' },
    });
    expect(res.status).toBe(200);
    const cookie = sessionCookie(res);
    expect(cookie).toMatch(/^scrin\.session_token=/);

    const s = await rt.request('/api/auth/get-session', { headers: { cookie } });
    expect(s.status).toBe(200);
    const body = (await s.json()) as { user: { email: string } };
    expect(body.user.email).toBe(email);
  });

  it('signs in with the right password and rejects a wrong one', async () => {
    const ok = await rt.request('/api/auth/sign-in/email', {
      method: 'POST',
      json: { email, password },
    });
    expect(ok.status).toBe(200);
    expect(sessionCookie(ok)).toContain('session_token');

    const bad = await rt.request('/api/auth/sign-in/email', {
      method: 'POST',
      json: { email, password: 'wrong password entirely' },
    });
    expect(bad.status).toBe(401);
  });

  it('gives the same answer for an unknown email as for a wrong password', async () => {
    const res = await rt.request('/api/auth/sign-in/email', {
      method: 'POST',
      json: { email: `nobody-${Date.now()}@example.test`, password: 'whatever whatever' },
    });
    expect(res.status).toBe(401);
  });

  it('rejects passwords shorter than 12 characters', async () => {
    const res = await rt.request('/api/auth/sign-up/email', {
      method: 'POST',
      json: { email: `short-${Date.now()}@example.test`, password: 'short', name: 'S' },
    });
    expect(res.status).toBe(400);
  });

  it('stores only a password hash, never the password', async () => {
    const rows = await rt.handle.db.query.account.findMany();
    expect(rows.length).toBeGreaterThan(0);
    for (const r of rows) {
      expect(r.password).not.toBe(password);
      expect(r.password).toMatch(/^[0-9a-f]+:[0-9a-f]+$/);
    }
  });

  it('requires a session for /v1 routes', async () => {
    const res = await rt.app.request(`${ORIGIN}/v1/me`);
    expect(res.status).toBe(401);
    expect(await res.json()).toEqual({
      error: { code: 'unauthorized', message: 'Authentication required' },
    });
  });

  it('mounts passkey, two-factor and organization endpoints', async () => {
    const signIn = await rt.request('/api/auth/sign-in/email', {
      method: 'POST',
      json: { email, password },
    });
    const cookie = sessionCookie(signIn);
    const passkeys = await rt.request('/api/auth/passkey/list-user-passkeys', {
      headers: { cookie },
    });
    expect(passkeys.status).toBe(200);
    const orgs = await rt.request('/api/auth/organization/list', { headers: { cookie } });
    expect(orgs.status).toBe(200);
    const create = await rt.request('/api/auth/organization/create', {
      method: 'POST',
      headers: { cookie },
      json: { name: 'Acme IT', slug: `acme-${Date.now()}` },
    });
    expect(create.status).toBe(200);
    const org = (await create.json()) as { id: string; members: { role: string }[] };
    expect(org.members[0]?.role).toBe('owner');
    const me = await rt.request('/v1/me', { headers: { cookie, 'x-scrin-org': org.id } });
    expect(me.status).toBe(200);
    expect(((await me.json()) as { role: string }).role).toBe('owner');
    // TOTP enable needs the password; a wrong one must not enable 2FA.
    const tf = await rt.request('/api/auth/two-factor/enable', {
      method: 'POST',
      headers: { cookie },
      json: { password: 'not the password at all' },
    });
    expect(tf.status).toBeGreaterThanOrEqual(400);
  });
});
