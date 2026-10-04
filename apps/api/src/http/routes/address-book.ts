import { createRoute } from '@hono/zod-openapi';
import { asc, eq, isNull, or } from 'drizzle-orm';
import { authorize } from '../../auth/actor.ts';
import type { Actor, AppDeps } from '../../context.ts';
import type { Scope } from '../../db/scoped.ts';
import { addressBookEntry, device } from '../../db/schema/index.ts';
import { forbidden, notFound } from '../../errors.ts';
import { newId } from '../../ids.ts';
import {
  AddressBookEntrySchema,
  AddressBookInput,
  AddressBookListSchema,
  AddressBookUpdate,
  body,
  commonErrors,
  IdParam,
  json,
} from '../schemas.ts';
import { iso, security, type App } from '../util.ts';
import { findDevice } from './devices.ts';

const tag = ['address-book'];

/** Personal entries of the caller plus the organisation's shared entries. */
const visible = (actor: Actor) =>
  or(isNull(addressBookEntry.ownerUserId), eq(addressBookEntry.ownerUserId, actor.userId));

async function rows(scope: Scope, actor: Actor, publicId?: string) {
  return scope.db
    .select({ e: addressBookEntry, d: device.publicId })
    .from(addressBookEntry)
    .leftJoin(device, eq(device.id, addressBookEntry.deviceId))
    .where(
      scope.where(
        addressBookEntry,
        visible(actor),
        publicId === undefined ? undefined : eq(addressBookEntry.publicId, publicId),
      ),
    )
    .orderBy(asc(addressBookEntry.label));
}

type Row = Awaited<ReturnType<typeof rows>>[number];

const toDto = (r: Row) => ({
  id: r.e.publicId,
  label: r.e.label,
  scrinId: r.e.scrinId,
  shared: r.e.ownerUserId === null,
  deviceId: r.d,
  notes: r.e.notes,
  tags: r.e.tags,
  createdAt: iso(r.e.createdAt),
});

async function findOne(scope: Scope, actor: Actor, id: string): Promise<Row> {
  const [r] = await rows(scope, actor, id);
  if (r === undefined) throw notFound('Address book entry');
  // Shared entries are org data: only admins (who may write policies) edit them.
  return r;
}

function assertCanEdit(actor: Actor, r: Row): void {
  if (r.e.ownerUserId === null && !actor.permissions.has('groups:write')) {
    throw forbidden('Only admins can change shared address book entries');
  }
}

export function registerAddressBookRoutes(app: App, deps: AppDeps): void {
  app.openapi(
    createRoute({
      method: 'get',
      path: '/v1/address-book',
      operationId: 'listAddressBook',
      summary: 'List your address book entries and the shared ones',
      tags: tag,
      security,
      responses: { 200: json(AddressBookListSchema, 'Entries'), ...commonErrors },
    }),
    async (c) => {
      const scope = authorize(c.var.actor, deps, 'address_book:read');
      return c.json({ items: (await rows(scope, c.var.actor)).map(toDto) }, 200);
    },
  );

  app.openapi(
    createRoute({
      method: 'post',
      path: '/v1/address-book',
      operationId: 'createAddressBookEntry',
      summary: 'Add an address book entry (personal, or shared with the organisation)',
      tags: tag,
      security,
      request: { body: body(AddressBookInput) },
      responses: { 201: json(AddressBookEntrySchema, 'Created'), ...commonErrors },
    }),
    async (c) => {
      const actor = c.var.actor;
      const scope = authorize(actor, deps, 'address_book:write');
      const input = c.req.valid('json');
      if (input.shared && !actor.permissions.has('groups:write')) {
        throw forbidden('Only admins can add shared address book entries');
      }
      const dev =
        input.deviceId === undefined ? undefined : await findDevice(scope, input.deviceId);
      const publicId = newId('abk');
      await deps.db.insert(addressBookEntry).values({
        publicId,
        orgId: scope.orgId,
        ownerUserId: input.shared ? null : actor.userId,
        label: input.label,
        scrinId: input.scrinId,
        deviceId: dev?.id ?? null,
        notes: input.notes ?? null,
        tags: input.tags ?? [],
      });
      return c.json(toDto(await findOne(scope, actor, publicId)), 201);
    },
  );

  app.openapi(
    createRoute({
      method: 'patch',
      path: '/v1/address-book/{id}',
      operationId: 'updateAddressBookEntry',
      summary: 'Update an address book entry',
      tags: tag,
      security,
      request: { params: IdParam, body: body(AddressBookUpdate) },
      responses: { 200: json(AddressBookEntrySchema, 'Updated'), ...commonErrors },
    }),
    async (c) => {
      const actor = c.var.actor;
      const scope = authorize(actor, deps, 'address_book:write');
      const input = c.req.valid('json');
      const r = await findOne(scope, actor, c.req.valid('param').id);
      assertCanEdit(actor, r);
      await deps.db
        .update(addressBookEntry)
        .set({
          ...(input.label === undefined ? {} : { label: input.label }),
          ...(input.notes === undefined ? {} : { notes: input.notes }),
          ...(input.tags === undefined ? {} : { tags: input.tags }),
        })
        .where(scope.where(addressBookEntry, eq(addressBookEntry.id, r.e.id)));
      return c.json(toDto(await findOne(scope, actor, r.e.publicId)), 200);
    },
  );

  app.openapi(
    createRoute({
      method: 'delete',
      path: '/v1/address-book/{id}',
      operationId: 'deleteAddressBookEntry',
      summary: 'Delete an address book entry',
      tags: tag,
      security,
      request: { params: IdParam },
      responses: { 204: { description: 'Deleted' }, ...commonErrors },
    }),
    async (c) => {
      const actor = c.var.actor;
      const scope = authorize(actor, deps, 'address_book:write');
      const r = await findOne(scope, actor, c.req.valid('param').id);
      assertCanEdit(actor, r);
      await deps.db
        .delete(addressBookEntry)
        .where(scope.where(addressBookEntry, eq(addressBookEntry.id, r.e.id)));
      return c.body(null, 204);
    },
  );
}
