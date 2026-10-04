import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import {
  as,
  createOrg,
  createTestRuntime,
  registerDevice,
  signUp,
  type TestRuntime,
  type TestUser,
} from './test/harness.ts';

let rt: TestRuntime;
let admin: TestUser;
let tech: TestUser;
let viewer: TestUser;
let org: string;
let deviceId: string;

beforeAll(async () => {
  rt = await createTestRuntime();
  admin = await signUp(rt, 'admin');
  tech = await signUp(rt, 'tech');
  viewer = await signUp(rt, 'viewer');
  org = await createOrg(rt, [
    { user: admin, role: 'admin' },
    { user: tech, role: 'technician' },
    { user: viewer, role: 'viewer' },
  ]);
  deviceId = (await registerDevice(rt, as(admin, org))).id;
});
afterAll(async () => {
  await rt.handle.close();
});

interface Grant {
  id: string;
  status: string;
  approverUserId: string | null;
  windowStart: string;
  windowEnd: string;
}

async function request(user: TestUser, minutes = 60): Promise<Response> {
  return rt.request('/v1/jit', {
    method: 'POST',
    headers: as(user, org),
    json: { deviceId, reason: 'printer driver ticket #42', durationMinutes: minutes },
  });
}

describe('JIT access', () => {
  it('technician requests, admin approves', async () => {
    const res = await request(tech);
    expect(res.status).toBe(201);
    const g = (await res.json()) as Grant;
    expect(g.status).toBe('pending');
    expect(new Date(g.windowEnd).getTime() - new Date(g.windowStart).getTime()).toBe(60 * 60_000);

    const ok = await rt.request(`/v1/jit/${g.id}/approve`, {
      method: 'POST',
      headers: as(admin, org),
      json: { note: 'go ahead' },
    });
    expect(ok.status).toBe(200);
    const approved = (await ok.json()) as Grant;
    expect(approved).toMatchObject({ status: 'approved', approverUserId: admin.id });

    const again = await rt.request(`/v1/jit/${g.id}/deny`, {
      method: 'POST',
      headers: as(admin, org),
      json: {},
    });
    expect(again.status).toBe(409);

    const audit = await rt.request('/v1/audit', { headers: as(admin, org) });
    const actions = ((await audit.json()) as { items: { action: string }[] }).items.map(
      (e) => e.action,
    );
    expect(actions).toEqual(expect.arrayContaining(['jit.requested', 'jit.approved']));
  });

  it('admin denies', async () => {
    const g = (await (await request(tech)).json()) as Grant;
    const res = await rt.request(`/v1/jit/${g.id}/deny`, {
      method: 'POST',
      headers: as(admin, org),
      json: {},
    });
    expect(((await res.json()) as Grant).status).toBe('denied');
  });

  it('technicians cannot approve, and nobody approves their own request', async () => {
    const g = (await (await request(tech)).json()) as Grant;
    const byTech = await rt.request(`/v1/jit/${g.id}/approve`, {
      method: 'POST',
      headers: as(tech, org),
      json: {},
    });
    expect(byTech.status).toBe(403);
    const own = (await (await request(admin)).json()) as Grant;
    const self = await rt.request(`/v1/jit/${own.id}/approve`, {
      method: 'POST',
      headers: as(admin, org),
      json: {},
    });
    expect(self.status).toBe(403);
    expect(((await self.json()) as { error: { code: string } }).error.code).toBe('self_approval');
  });

  it('viewers can list but not request', async () => {
    expect((await request(viewer)).status).toBe(403);
    const list = await rt.request('/v1/jit?status=pending', { headers: as(viewer, org) });
    expect(list.status).toBe(200);
    const items = ((await list.json()) as { items: Grant[] }).items;
    expect(items.every((i) => i.status === 'pending')).toBe(true);
  });

  it('a pending request whose window has passed expires and cannot be approved', async () => {
    const g = (await (await request(tech, 5)).json()) as Grant;
    rt.deps.now = () => new Date(Date.now() + 10 * 60_000);
    try {
      const res = await rt.request(`/v1/jit/${g.id}/approve`, {
        method: 'POST',
        headers: as(admin, org),
        json: {},
      });
      expect(res.status).toBe(409);
      const list = await rt.request('/v1/jit?status=expired', { headers: as(admin, org) });
      expect(((await list.json()) as { items: Grant[] }).items.map((i) => i.id)).toContain(g.id);
    } finally {
      delete rt.deps.now;
    }
  });

  it('rejects a window that starts in the past', async () => {
    const res = await rt.request('/v1/jit', {
      method: 'POST',
      headers: as(tech, org),
      json: { deviceId, reason: 'late', durationMinutes: 30, windowStart: '2020-01-01T00:00:00Z' },
    });
    expect(res.status).toBe(400);
    expect(((await res.json()) as { error: { code: string } }).error.code).toBe('invalid_window');
  });
});
