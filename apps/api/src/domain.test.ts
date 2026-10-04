import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { sessionLogMessage } from './http/routes/sessions.ts';
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
let owner: TestUser;
let tech: TestUser;
let org: string;

beforeAll(async () => {
  rt = await createTestRuntime();
  owner = await signUp(rt, 'owner');
  tech = await signUp(rt, 'tech');
  org = await createOrg(rt, [
    { user: owner, role: 'owner' },
    { user: tech, role: 'technician' },
  ]);
});
afterAll(async () => {
  await rt.handle.close();
});

const policyDoc = {
  allowedPermissions: ['view', 'input', 'record'],
  recordingRequired: true,
  privacyModeAllowed: false,
  unattendedAllowed: true,
  sessionMaxMinutes: 120,
};

describe('policies', () => {
  it('creates, reads, replaces and deletes a policy', async () => {
    const res = await rt.request('/v1/policies', {
      method: 'POST',
      headers: as(owner, org),
      json: { name: 'Helpdesk', document: policyDoc },
    });
    expect(res.status).toBe(201);
    const p = (await res.json()) as { id: string; document: typeof policyDoc };
    expect(p.document).toEqual(policyDoc);
    const put = await rt.request(`/v1/policies/${p.id}`, {
      method: 'PUT',
      headers: as(owner, org),
      json: { name: 'Helpdesk', document: { ...policyDoc, sessionMaxMinutes: 30 } },
    });
    expect(
      ((await put.json()) as { document: { sessionMaxMinutes: number } }).document
        .sessionMaxMinutes,
    ).toBe(30);
    expect(
      (await rt.request(`/v1/policies/${p.id}`, { method: 'DELETE', headers: as(owner, org) }))
        .status,
    ).toBe(204);
    expect((await rt.request(`/v1/policies/${p.id}`, { headers: as(owner, org) })).status).toBe(
      404,
    );
  });

  it('rejects inconsistent or unknown policy fields', async () => {
    const bad = await rt.request('/v1/policies', {
      method: 'POST',
      headers: as(owner, org),
      json: { name: 'Bad', document: { ...policyDoc, allowedPermissions: ['view'] } },
    });
    expect(bad.status).toBe(400);
    const extra = await rt.request('/v1/policies', {
      method: 'POST',
      headers: as(owner, org),
      json: { name: 'Extra', document: { ...policyDoc, superpower: true } },
    });
    expect(extra.status).toBe(400);
    const perm = await rt.request('/v1/policies', {
      method: 'POST',
      headers: as(owner, org),
      json: {
        name: 'Perm',
        document: { ...policyDoc, allowedPermissions: ['view', 'record', 'teleport'] },
      },
    });
    expect(perm.status).toBe(400);
  });
});

describe('address book', () => {
  it('personal entries are private; shared entries are admin-only to write', async () => {
    const mine = await rt.request('/v1/address-book', {
      method: 'POST',
      headers: as(tech, org),
      json: { label: 'Mum laptop', scrinId: '123456789' },
    });
    expect(mine.status).toBe(201);
    const shared = await rt.request('/v1/address-book', {
      method: 'POST',
      headers: as(tech, org),
      json: { label: 'Reception', scrinId: '987654321', shared: true },
    });
    expect(shared.status).toBe(403);
    const sharedByOwner = await rt.request('/v1/address-book', {
      method: 'POST',
      headers: as(owner, org),
      json: { label: 'Reception', scrinId: '987654321', shared: true },
    });
    expect(sharedByOwner.status).toBe(201);
    const sharedId = ((await sharedByOwner.json()) as { id: string }).id;

    const ownerView = (await (
      await rt.request('/v1/address-book', { headers: as(owner, org) })
    ).json()) as {
      items: { label: string }[];
    };
    expect(ownerView.items.map((i) => i.label)).toEqual(['Reception']);
    const techView = (await (
      await rt.request('/v1/address-book', { headers: as(tech, org) })
    ).json()) as {
      items: { label: string; shared: boolean }[];
    };
    expect(techView.items.map((i) => i.label).sort()).toEqual(['Mum laptop', 'Reception']);

    const edit = await rt.request(`/v1/address-book/${sharedId}`, {
      method: 'PATCH',
      headers: as(tech, org),
      json: { label: 'hacked' },
    });
    expect(edit.status).toBe(403);
  });
});

describe('session logs (signed by the host key)', () => {
  it('accepts a correctly signed log, rejects a forged one and a replay', async () => {
    const d = await registerDevice(rt, as(owner, org));
    const payload = {
      sessionKey: 'sess-0001-abcdef',
      controllerPub: null,
      controllerScrinId: '555666777',
      anonymous: true,
      permissions: ['view', 'input'] as ('view' | 'input')[],
      startedAt: '2026-10-04T10:00:00.000Z',
      endedAt: '2026-10-04T10:30:00.000Z',
      endReason: 'controller_left',
    };
    const forged = await rt.request('/v1/sessions', {
      method: 'POST',
      headers: as(owner, org),
      json: { deviceId: d.id, payload, signature: 'ab'.repeat(64) },
    });
    expect(forged.status).toBe(400);

    const signature = await d.key.sign(sessionLogMessage(org, d.id, payload));
    const ok = await rt.request('/v1/sessions', {
      method: 'POST',
      headers: as(owner, org),
      json: { deviceId: d.id, payload, signature },
    });
    expect(ok.status).toBe(201);

    const tampered = await rt.request('/v1/sessions', {
      method: 'POST',
      headers: as(owner, org),
      json: { deviceId: d.id, payload: { ...payload, sessionKey: 'sess-0002-abcdef' }, signature },
    });
    expect(tampered.status).toBe(400);

    const replay = await rt.request('/v1/sessions', {
      method: 'POST',
      headers: as(owner, org),
      json: { deviceId: d.id, payload, signature },
    });
    expect(replay.status).toBe(409);

    const list = await rt.request(`/v1/sessions?device=${d.id}`, { headers: as(tech, org) });
    const items = ((await list.json()) as { items: { sessionKey: string; anonymous: boolean }[] })
      .items;
    expect(items).toEqual([
      expect.objectContaining({ sessionKey: payload.sessionKey, anonymous: true }),
    ]);
  });
});
