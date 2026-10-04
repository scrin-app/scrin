import { createRoute, z } from '@hono/zod-openapi';
import { arrayContains, count, desc, eq, gt, ilike, isNull, or } from 'drizzle-orm';
import { actorRef, authorize } from '../../auth/actor.ts';
import type { AppDeps } from '../../context.ts';
import { bytesToHex, utf8, verifyEd25519 } from '../../crypto.ts';
import type { Db } from '../../db/client.ts';
import type { Scope } from '../../db/scoped.ts';
import { device, deviceChallenge, deviceGroup } from '../../db/schema/index.ts';
import { ApiError, conflict, notFound } from '../../errors.ts';
import { newId } from '../../ids.ts';
import { recordAudit } from '../../services/audit.ts';
import { rateLimit } from '../rate-limit.ts';
import {
  body,
  commonErrors,
  DeviceChallengeRequest,
  DeviceChallengeSchema,
  DeviceListQuery,
  DeviceListSchema,
  DeviceSchema,
  IdParam,
  json,
  PLATFORMS,
  RegisterDeviceRequest,
  UpdateDeviceRequest,
} from '../schemas.ts';
import { iso, isoOrNull, security, type App } from '../util.ts';

const CHALLENGE_TTL_MS = 5 * 60 * 1000;

/**
 * The exact text a device signs to join an organisation. Binds the key to
 * this organisation, this challenge and this scrin ID; the nonce makes it
 * single-use.
 */
function registrationMessage(c: {
  orgId: string;
  challengeId: string;
  nonce: string;
  devicePub: string;
  scrinId: string;
}): string {
  return [
    'scrin-device-register/v1',
    `org:${c.orgId}`,
    `challenge:${c.challengeId}`,
    `nonce:${c.nonce}`,
    `device:${c.devicePub}`,
    `scrin-id:${c.scrinId}`,
  ].join('\n');
}

const Platform = z.enum(PLATFORMS);

type DeviceRow = typeof device.$inferSelect;

function toDto(row: DeviceRow, groupPublicId: string | null) {
  return {
    id: row.publicId,
    scrinId: row.scrinId,
    devicePub: row.devicePub,
    name: row.name,
    platform: Platform.parse(row.platform),
    groupId: groupPublicId,
    tags: row.tags,
    lastSeenAt: isoOrNull(row.lastSeenAt),
    createdAt: iso(row.createdAt),
  };
}

async function resolveGroup(scope: Scope, publicId: string): Promise<number> {
  const [g] = await scope.db
    .select({ id: deviceGroup.id })
    .from(deviceGroup)
    .where(scope.where(deviceGroup, eq(deviceGroup.publicId, publicId)))
    .limit(1);
  if (g === undefined) throw notFound('Device group');
  return g.id;
}

export async function findDevice(scope: Scope, publicId: string): Promise<DeviceRow> {
  const [d] = await scope.db
    .select()
    .from(device)
    .where(scope.where(device, eq(device.publicId, publicId)))
    .limit(1);
  if (d === undefined) throw notFound('Device');
  return d;
}

async function loadDto(db: Db, scope: Scope, id: number) {
  const [r] = await db
    .select({ d: device, g: deviceGroup.publicId })
    .from(device)
    .leftJoin(deviceGroup, eq(deviceGroup.id, device.groupId))
    .where(scope.where(device, eq(device.id, id)))
    .limit(1);
  if (r === undefined) throw notFound('Device');
  return toDto(r.d, r.g);
}

const tag = ['devices'];

const listRoute = createRoute({
  method: 'get',
  path: '/v1/devices',
  operationId: 'listDevices',
  summary: 'List devices in the organisation',
  tags: tag,
  security,
  request: { query: DeviceListQuery },
  responses: { 200: json(DeviceListSchema, 'Devices'), ...commonErrors },
});

const getRoute = createRoute({
  method: 'get',
  path: '/v1/devices/{id}',
  operationId: 'getDevice',
  summary: 'Get one device',
  tags: tag,
  security,
  request: { params: IdParam },
  responses: { 200: json(DeviceSchema, 'Device'), ...commonErrors },
});

const challengeRoute = createRoute({
  method: 'post',
  path: '/v1/devices/challenge',
  operationId: 'createDeviceChallenge',
  summary: 'Start device registration: get a nonce for the device key to sign',
  tags: tag,
  security,
  request: { body: body(DeviceChallengeRequest) },
  responses: { 201: json(DeviceChallengeSchema, 'Challenge'), ...commonErrors },
});

const registerRoute = createRoute({
  method: 'post',
  path: '/v1/devices',
  operationId: 'registerDevice',
  summary: 'Register a device with an Ed25519 signature over the challenge message',
  tags: tag,
  security,
  request: { body: body(RegisterDeviceRequest) },
  responses: {
    201: json(DeviceSchema, 'Registered device'),
    ...commonErrors,
    409: json(
      z.object({ error: z.object({ code: z.string(), message: z.string() }) }),
      'Already registered',
    ),
  },
});

const updateRoute = createRoute({
  method: 'patch',
  path: '/v1/devices/{id}',
  operationId: 'updateDevice',
  summary: 'Rename, regroup or retag a device',
  tags: tag,
  security,
  request: { params: IdParam, body: body(UpdateDeviceRequest) },
  responses: { 200: json(DeviceSchema, 'Updated device'), ...commonErrors },
});

const deleteRoute = createRoute({
  method: 'delete',
  path: '/v1/devices/{id}',
  operationId: 'deleteDevice',
  summary: 'Remove a device from the organisation',
  tags: tag,
  security,
  request: { params: IdParam },
  responses: { 204: { description: 'Deleted' }, ...commonErrors },
});

export function registerDeviceRoutes(app: App, deps: AppDeps): void {
  const now = () => deps.now?.() ?? new Date();

  app.openapi(listRoute, async (c) => {
    const scope = authorize(c.var.actor, deps, 'devices:read');
    const q = c.req.valid('query');
    const groupId = q.group === undefined ? undefined : await resolveGroup(scope, q.group);
    const where = scope.where(
      device,
      groupId === undefined ? undefined : eq(device.groupId, groupId),
      q.tag === undefined ? undefined : arrayContains(device.tags, [q.tag]),
      q.q === undefined
        ? undefined
        : or(ilike(device.name, `%${q.q.replace(/[%_\\]/g, '\\$&')}%`), eq(device.scrinId, q.q)),
    );
    const rows = await deps.db
      .select({ d: device, g: deviceGroup.publicId })
      .from(device)
      .leftJoin(deviceGroup, eq(deviceGroup.id, device.groupId))
      .where(where)
      .orderBy(desc(device.createdAt), desc(device.id))
      .limit(q.limit)
      .offset(q.offset);
    const [total] = await deps.db.select({ n: count() }).from(device).where(where);
    return c.json({ items: rows.map((r) => toDto(r.d, r.g)), total: total?.n ?? 0 }, 200);
  });

  app.openapi(getRoute, async (c) => {
    const scope = authorize(c.var.actor, deps, 'devices:read');
    const d = await findDevice(scope, c.req.valid('param').id);
    return c.json(await loadDto(deps.db, scope, d.id), 200);
  });

  app.use(
    '/v1/devices/challenge',
    rateLimit({ name: 'device-challenge', limit: 30, windowMs: 60_000 }),
  );
  app.openapi(challengeRoute, async (c) => {
    const actor = c.var.actor;
    const scope = authorize(actor, deps, 'devices:write');
    const input = c.req.valid('json');
    const challengeId = newId('chl');
    const nonce = bytesToHex(crypto.getRandomValues(new Uint8Array(32)));
    const expiresAt = new Date(now().getTime() + CHALLENGE_TTL_MS);
    await deps.db.insert(deviceChallenge).values({
      publicId: challengeId,
      orgId: scope.orgId,
      userId: actor.userId,
      devicePub: input.devicePub,
      scrinId: input.scrinId,
      nonce,
      expiresAt,
    });
    const message = registrationMessage({ orgId: scope.orgId, challengeId, nonce, ...input });
    return c.json({ challengeId, nonce, message, expiresAt: iso(expiresAt) }, 201);
  });

  app.openapi(registerRoute, async (c) => {
    const actor = c.var.actor;
    const scope = authorize(actor, deps, 'devices:write');
    const input = c.req.valid('json');
    const t = now();
    const dto = await deps.db.transaction(async (tx) => {
      // Claim the challenge atomically: single use, unexpired, issued to this caller.
      const [ch] = await tx
        .update(deviceChallenge)
        .set({ usedAt: t })
        .where(
          scope.where(
            deviceChallenge,
            eq(deviceChallenge.publicId, input.challengeId),
            eq(deviceChallenge.userId, actor.userId),
            isNull(deviceChallenge.usedAt),
            gt(deviceChallenge.expiresAt, t),
          ),
        )
        .returning();
      if (ch === undefined) {
        throw new ApiError(400, 'invalid_challenge', 'Challenge is unknown, used or expired');
      }
      const message = registrationMessage({
        orgId: ch.orgId,
        challengeId: ch.publicId,
        nonce: ch.nonce,
        devicePub: ch.devicePub,
        scrinId: ch.scrinId,
      });
      if (!(await verifyEd25519(ch.devicePub, utf8(message), input.signature))) {
        throw new ApiError(
          400,
          'invalid_signature',
          'Signature does not verify for this device key',
        );
      }
      const [existing] = await tx
        .select({ id: device.id })
        .from(device)
        .where(scope.where(device, eq(device.devicePub, ch.devicePub)))
        .limit(1);
      if (existing !== undefined) throw conflict('Device key already registered', 'device_exists');
      const groupId =
        input.groupId === undefined
          ? null
          : await resolveGroup({ ...scope, db: tx }, input.groupId);
      const publicId = newId('dev');
      const [row] = await tx
        .insert(device)
        .values({
          publicId,
          orgId: scope.orgId,
          devicePub: ch.devicePub,
          scrinId: ch.scrinId,
          name: input.name,
          platform: input.platform,
          groupId,
          tags: input.tags ?? [],
          lastSeenAt: t,
          registeredBy: actor.userId,
        })
        .returning();
      if (row === undefined) throw new Error('insert returned no row');
      await recordAudit(
        tx,
        scope.orgId,
        {
          ...actorRef(actor),
          action: 'device.registered',
          targetType: 'device',
          targetId: publicId,
          data: { scrinId: ch.scrinId, name: input.name, platform: input.platform },
        },
        t,
      );
      return toDto(row, input.groupId ?? null);
    });
    return c.json(dto, 201);
  });

  app.openapi(updateRoute, async (c) => {
    const actor = c.var.actor;
    const scope = authorize(actor, deps, 'devices:write');
    const input = c.req.valid('json');
    const d = await findDevice(scope, c.req.valid('param').id);
    await deps.db.transaction(async (tx) => {
      const groupId =
        input.groupId === undefined
          ? undefined
          : input.groupId === null
            ? null
            : await resolveGroup({ ...scope, db: tx }, input.groupId);
      await tx
        .update(device)
        .set({
          ...(input.name === undefined ? {} : { name: input.name }),
          ...(groupId === undefined ? {} : { groupId }),
          ...(input.tags === undefined ? {} : { tags: input.tags }),
          updatedAt: now(),
        })
        .where(scope.where(device, eq(device.id, d.id)));
      await recordAudit(
        tx,
        scope.orgId,
        {
          ...actorRef(actor),
          action: 'device.updated',
          targetType: 'device',
          targetId: d.publicId,
          data: { fields: Object.keys(input) },
        },
        now(),
      );
    });
    return c.json(await loadDto(deps.db, scope, d.id), 200);
  });

  app.openapi(deleteRoute, async (c) => {
    const actor = c.var.actor;
    const scope = authorize(actor, deps, 'devices:delete');
    const d = await findDevice(scope, c.req.valid('param').id);
    await deps.db.transaction(async (tx) => {
      await tx.delete(device).where(scope.where(device, eq(device.id, d.id)));
      await recordAudit(
        tx,
        scope.orgId,
        {
          ...actorRef(actor),
          action: 'device.deleted',
          targetType: 'device',
          targetId: d.publicId,
          data: { scrinId: d.scrinId },
        },
        now(),
      );
    });
    return c.body(null, 204);
  });
}
