import { and, eq, sql } from 'drizzle-orm';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { canonicalJson, sha256Hex } from './crypto.ts';
import { auditEvent } from './db/schema/index.ts';
import { chainHash, GENESIS_HASH, recordAudit, verifyChain } from './services/audit.ts';
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
let org: string;

beforeAll(async () => {
  rt = await createTestRuntime();
  owner = await signUp(rt, 'owner');
  org = await createOrg(rt, [{ user: owner, role: 'owner' }]);
});
afterAll(async () => {
  await rt.handle.close();
});

describe('canonicalJson', () => {
  it('sorts keys recursively and drops undefined', () => {
    expect(canonicalJson({ b: 1, a: { d: [1, { z: true, y: null }], c: 'x' }, u: undefined })).toBe(
      '{"a":{"c":"x","d":[1,{"y":null,"z":true}]},"b":1}',
    );
  });
  it('rejects non-finite numbers', () => {
    expect(() => canonicalJson({ n: Number.NaN })).toThrow();
  });
});

describe('audit hash chain', () => {
  it('links each record to the previous hash, starting from the genesis hash', async () => {
    await registerDevice(rt, as(owner, org));
    await registerDevice(rt, as(owner, org));
    const res = await rt.request('/v1/audit', { headers: as(owner, org) });
    const items = (
      (await res.json()) as { items: { seq: number; prevHash: string; hash: string }[] }
    ).items;
    expect(items.length).toBeGreaterThanOrEqual(2);
    expect(items[0]?.seq).toBe(1);
    expect(items[0]?.prevHash).toBe(GENESIS_HASH);
    for (let i = 1; i < items.length; i++) expect(items[i]?.prevHash).toBe(items[i - 1]?.hash);
  });

  it('hash = sha256(prev_hash || canonical json) (known answer)', () => {
    const fields = {
      orgId: 'org_1',
      seq: 1,
      actorType: 'user',
      actorId: 'u1',
      action: 'device.registered',
      targetType: 'device',
      targetId: 'dev_1',
      data: { b: 2, a: 1 },
      createdAt: new Date('2026-10-04T00:00:00.000Z'),
    };
    const expected = sha256Hex(
      `${GENESIS_HASH}{"action":"device.registered","actorId":"u1","actorType":"user","createdAt":"2026-10-04T00:00:00.000Z","data":{"a":1,"b":2},"orgId":"org_1","seq":1,"targetId":"dev_1","targetType":"device"}`,
    );
    expect(chainHash(GENESIS_HASH, fields)).toBe(expected);
  });

  it('verify endpoint reports a valid chain', async () => {
    const res = await rt.request('/v1/audit/verify', { headers: as(owner, org) });
    expect(res.status).toBe(200);
    const v = (await res.json()) as { valid: boolean; count: number; brokenAt: unknown };
    expect(v).toMatchObject({ valid: true, brokenAt: null });
    expect(v.count).toBeGreaterThanOrEqual(2);
  });

  it('detects a tampered payload', async () => {
    const otherOrg = await createOrg(rt, [{ user: owner, role: 'owner' }]);
    for (let i = 0; i < 3; i++) {
      await rt.deps.db.transaction((tx) =>
        recordAudit(tx, otherOrg, {
          actorType: 'system',
          actorId: 't',
          action: 'test.event',
          data: { i },
        }),
      );
    }
    expect((await verifyChain(rt.deps.db, otherOrg)).valid).toBe(true);
    await rt.deps.db
      .update(auditEvent)
      .set({ data: { i: 99 } })
      .where(and(eq(auditEvent.orgId, otherOrg), eq(auditEvent.seq, 2)));
    const res = await rt.request('/v1/audit/verify', { headers: as(owner, otherOrg) });
    const v = (await res.json()) as { valid: boolean; brokenAt: { seq: number; reason: string } };
    expect(v.valid).toBe(false);
    expect(v.brokenAt).toMatchObject({ seq: 2, reason: 'hash_mismatch' });
  });

  it('detects a deleted record and a rewritten link', async () => {
    const o = await createOrg(rt, [{ user: owner, role: 'owner' }]);
    for (let i = 0; i < 4; i++) {
      await rt.deps.db.transaction((tx) =>
        recordAudit(tx, o, {
          actorType: 'system',
          actorId: 't',
          action: 'test.event',
          data: { i },
        }),
      );
    }
    await rt.deps.db.execute(sql`delete from audit_event where org_id = ${o} and seq = 3`);
    expect((await verifyChain(rt.deps.db, o)).brokenAt).toMatchObject({
      seq: 4,
      reason: 'seq_gap',
    });

    const o2 = await createOrg(rt, [{ user: owner, role: 'owner' }]);
    for (let i = 0; i < 2; i++) {
      await rt.deps.db.transaction((tx) =>
        recordAudit(tx, o2, {
          actorType: 'system',
          actorId: 't',
          action: 'test.event',
          data: { i },
        }),
      );
    }
    await rt.deps.db
      .update(auditEvent)
      .set({ prevHash: 'f'.repeat(64) })
      .where(and(eq(auditEvent.orgId, o2), eq(auditEvent.seq, 2)));
    expect((await verifyChain(rt.deps.db, o2)).brokenAt).toMatchObject({
      seq: 2,
      reason: 'prev_hash_mismatch',
    });
  });

  it('viewers cannot read the audit log', async () => {
    const viewer = await signUp(rt, 'viewer');
    const o = await createOrg(rt, [{ user: viewer, role: 'viewer' }]);
    const res = await rt.request('/v1/audit/verify', { headers: as(viewer, o) });
    expect(res.status).toBe(403);
  });
});
