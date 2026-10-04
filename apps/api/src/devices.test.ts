import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import {
  as,
  createOrg,
  createTestRuntime,
  deviceKey,
  registerDevice,
  scrinId,
  setRole,
  signUp,
  type TestRuntime,
  type TestUser,
} from './test/harness.ts';

let rt: TestRuntime;
let alice: TestUser;
let bob: TestUser;
let orgA: string;
let orgB: string;

beforeAll(async () => {
  rt = await createTestRuntime();
  alice = await signUp(rt, 'alice');
  bob = await signUp(rt, 'bob');
  orgA = await createOrg(rt, [{ user: alice, role: 'owner' }]);
  orgB = await createOrg(rt, [{ user: bob, role: 'owner' }]);
});
afterAll(async () => {
  await rt.handle.close();
});

describe('device registration (Ed25519 challenge)', () => {
  it('registers a device with a valid signature and lists it', async () => {
    const d = await registerDevice(rt, as(alice, orgA), { name: 'Front desk' });
    expect(d.id).toMatch(/^dev_[0-9A-Za-z]{20}$/);
    const res = await rt.request('/v1/devices', { headers: as(alice, orgA) });
    expect(res.status).toBe(200);
    const list = (await res.json()) as {
      items: { id: string; name: string; devicePub: string }[];
      total: number;
    };
    expect(list.items.map((i) => i.id)).toContain(d.id);
    expect(list.items.find((i) => i.id === d.id)?.devicePub).toBe(d.key.pubHex);
  });

  it('rejects a signature by a different key', async () => {
    const key = await deviceKey();
    const other = await deviceKey();
    const ch = await rt.request('/v1/devices/challenge', {
      method: 'POST',
      headers: as(alice, orgA),
      json: { devicePub: key.pubHex, scrinId: scrinId() },
    });
    const { challengeId, message } = (await ch.json()) as { challengeId: string; message: string };
    const res = await rt.request('/v1/devices', {
      method: 'POST',
      headers: as(alice, orgA),
      json: { challengeId, signature: await other.sign(message), name: 'x', platform: 'windows' },
    });
    expect(res.status).toBe(400);
    expect(((await res.json()) as { error: { code: string } }).error.code).toBe(
      'invalid_signature',
    );
  });

  it('rejects a signature over a different message and a reused challenge', async () => {
    const key = await deviceKey();
    const ch = await rt.request('/v1/devices/challenge', {
      method: 'POST',
      headers: as(alice, orgA),
      json: { devicePub: key.pubHex, scrinId: scrinId() },
    });
    const { challengeId, message } = (await ch.json()) as { challengeId: string; message: string };
    const tampered = await rt.request('/v1/devices', {
      method: 'POST',
      headers: as(alice, orgA),
      json: {
        challengeId,
        signature: await key.sign(`${message}x`),
        name: 'x',
        platform: 'windows',
      },
    });
    expect(tampered.status).toBe(400);
    // A failed verification rolls back the claim, so the right signature still works once.
    const good = await rt.request('/v1/devices', {
      method: 'POST',
      headers: as(alice, orgA),
      json: { challengeId, signature: await key.sign(message), name: 'x', platform: 'windows' },
    });
    expect(good.status).toBe(201);
    const replay = await rt.request('/v1/devices', {
      method: 'POST',
      headers: as(alice, orgA),
      json: { challengeId, signature: await key.sign(message), name: 'x', platform: 'windows' },
    });
    expect(replay.status).toBe(400);
    expect(((await replay.json()) as { error: { code: string } }).error.code).toBe(
      'invalid_challenge',
    );
  });

  it('refuses to register the same key twice in one organisation', async () => {
    const d = await registerDevice(rt, as(alice, orgA));
    await expect(registerDevice(rt, as(alice, orgA), { key: d.key })).rejects.toThrow(
      /register 409/,
    );
  });

  it('lets technicians register but not delete; viewers cannot register', async () => {
    const tech = await signUp(rt, 'tech');
    const viewer = await signUp(rt, 'viewer');
    const org = await createOrg(rt, [
      { user: tech, role: 'technician' },
      { user: viewer, role: 'viewer' },
    ]);
    const d = await registerDevice(rt, as(tech, org));
    const del = await rt.request(`/v1/devices/${d.id}`, {
      method: 'DELETE',
      headers: as(tech, org),
    });
    expect(del.status).toBe(403);
    await expect(registerDevice(rt, as(viewer, org))).rejects.toThrow(/challenge 403/);
    await setRole(rt, org, tech, 'admin');
    const del2 = await rt.request(`/v1/devices/${d.id}`, {
      method: 'DELETE',
      headers: as(tech, org),
    });
    expect(del2.status).toBe(204);
  });
});

describe('organisation isolation', () => {
  it('a member of org B gets 404 for a device of org A, for every verb', async () => {
    const d = await registerDevice(rt, as(alice, orgA));
    const get = await rt.request(`/v1/devices/${d.id}`, { headers: as(bob, orgB) });
    expect(get.status).toBe(404);
    expect(((await get.json()) as { error: { code: string } }).error.code).toBe('not_found');
    const patch = await rt.request(`/v1/devices/${d.id}`, {
      method: 'PATCH',
      headers: as(bob, orgB),
      json: { name: 'pwned' },
    });
    expect(patch.status).toBe(404);
    const del = await rt.request(`/v1/devices/${d.id}`, {
      method: 'DELETE',
      headers: as(bob, orgB),
    });
    expect(del.status).toBe(404);
    const list = await rt.request('/v1/devices', { headers: as(bob, orgB) });
    const items = ((await list.json()) as { items: { id: string }[] }).items;
    expect(items.map((i) => i.id)).not.toContain(d.id);
    // Still intact for its owner.
    const mine = await rt.request(`/v1/devices/${d.id}`, { headers: as(alice, orgA) });
    expect(((await mine.json()) as { name: string }).name).toBe('Workstation');
  });

  it('naming an organisation you are not a member of is a 404', async () => {
    const res = await rt.request('/v1/devices', { headers: as(bob, orgA) });
    expect(res.status).toBe(404);
  });

  it('JIT, policies and audit of org A are invisible to org B', async () => {
    const d = await registerDevice(rt, as(alice, orgA));
    const jit = await rt.request('/v1/jit', {
      method: 'POST',
      headers: as(bob, orgB),
      json: { deviceId: d.id, reason: 'try cross-org', durationMinutes: 30 },
    });
    expect(jit.status).toBe(404);
    const audit = await rt.request('/v1/audit', { headers: as(bob, orgB) });
    const events = ((await audit.json()) as { items: { targetId: string | null }[] }).items;
    expect(events.some((e) => e.targetId === d.id)).toBe(false);
  });
});

describe('groups', () => {
  it('creates a group, assigns a device, filters by group', async () => {
    const g = await rt.request('/v1/groups', {
      method: 'POST',
      headers: as(alice, orgA),
      json: { name: 'Kiosks' },
    });
    expect(g.status).toBe(201);
    const group = (await g.json()) as { id: string };
    const d = await registerDevice(rt, as(alice, orgA), { groupId: group.id });
    const res = await rt.request(`/v1/devices?group=${group.id}`, { headers: as(alice, orgA) });
    const items = ((await res.json()) as { items: { id: string; groupId: string }[] }).items;
    expect(items.map((i) => i.id)).toEqual([d.id]);
    const groups = await rt.request('/v1/groups', { headers: as(alice, orgA) });
    const kiosk = (
      (await groups.json()) as { items: { id: string; deviceCount: number }[] }
    ).items.find((x) => x.id === group.id);
    expect(kiosk?.deviceCount).toBe(1);
  });
});
