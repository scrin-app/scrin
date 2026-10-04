// scrin domain tables. Every table is tenant-scoped by `org_id`; internal
// identity PKs never leave the server, `public_id` (prefix_ + random) is what
// the API exposes.
import { sql } from 'drizzle-orm';
import {
  bigint,
  boolean,
  index,
  integer,
  jsonb,
  pgTable,
  text,
  timestamp,
  uniqueIndex,
} from 'drizzle-orm/pg-core';
import { organization, user } from './auth.ts';

const tz = { withTimezone: true } as const;

const orgId = () =>
  text('org_id')
    .notNull()
    .references(() => organization.id, { onDelete: 'cascade' });

export const deviceGroup = pgTable(
  'device_group',
  {
    id: bigint('id', { mode: 'number' }).primaryKey().generatedAlwaysAsIdentity(),
    publicId: text('public_id').notNull().unique(),
    orgId: orgId(),
    name: text('name').notNull(),
    description: text('description'),
    createdAt: timestamp('created_at', tz).notNull().defaultNow(),
    updatedAt: timestamp('updated_at', tz).notNull().defaultNow(),
  },
  (t) => [
    index('device_group_org_id_idx').on(t.orgId),
    uniqueIndex('device_group_org_name_uq').on(t.orgId, t.name),
  ],
);

export const deviceChallenge = pgTable(
  'device_challenge',
  {
    id: bigint('id', { mode: 'number' }).primaryKey().generatedAlwaysAsIdentity(),
    publicId: text('public_id').notNull().unique(),
    orgId: orgId(),
    userId: text('user_id')
      .notNull()
      .references(() => user.id, { onDelete: 'cascade' }),
    devicePub: text('device_pub').notNull(),
    scrinId: text('scrin_id').notNull(),
    nonce: text('nonce').notNull(),
    expiresAt: timestamp('expires_at', tz).notNull(),
    usedAt: timestamp('used_at', tz),
    createdAt: timestamp('created_at', tz).notNull().defaultNow(),
  },
  (t) => [
    index('device_challenge_org_id_idx').on(t.orgId),
    index('device_challenge_user_id_idx').on(t.userId),
  ],
);

export const device = pgTable(
  'device',
  {
    id: bigint('id', { mode: 'number' }).primaryKey().generatedAlwaysAsIdentity(),
    publicId: text('public_id').notNull().unique(),
    orgId: orgId(),
    /** Ed25519 public key, 64 lowercase hex chars. */
    devicePub: text('device_pub').notNull(),
    /** 9-digit scrin ID bound to the key on the rendezvous server. */
    scrinId: text('scrin_id').notNull(),
    name: text('name').notNull(),
    platform: text('platform').notNull(),
    groupId: bigint('group_id', { mode: 'number' }).references(() => deviceGroup.id, {
      onDelete: 'set null',
    }),
    tags: text('tags')
      .array()
      .notNull()
      .default(sql`'{}'::text[]`),
    lastSeenAt: timestamp('last_seen_at', tz),
    registeredBy: text('registered_by').references(() => user.id, { onDelete: 'set null' }),
    createdAt: timestamp('created_at', tz).notNull().defaultNow(),
    updatedAt: timestamp('updated_at', tz).notNull().defaultNow(),
  },
  (t) => [
    index('device_org_id_idx').on(t.orgId),
    index('device_group_id_idx').on(t.groupId),
    index('device_registered_by_idx').on(t.registeredBy),
    uniqueIndex('device_org_pub_uq').on(t.orgId, t.devicePub),
  ],
);

export const addressBookEntry = pgTable(
  'address_book_entry',
  {
    id: bigint('id', { mode: 'number' }).primaryKey().generatedAlwaysAsIdentity(),
    publicId: text('public_id').notNull().unique(),
    orgId: orgId(),
    /** NULL = shared with the whole organisation. */
    ownerUserId: text('owner_user_id').references(() => user.id, { onDelete: 'cascade' }),
    label: text('label').notNull(),
    scrinId: text('scrin_id').notNull(),
    deviceId: bigint('device_id', { mode: 'number' }).references(() => device.id, {
      onDelete: 'set null',
    }),
    notes: text('notes'),
    tags: text('tags')
      .array()
      .notNull()
      .default(sql`'{}'::text[]`),
    createdAt: timestamp('created_at', tz).notNull().defaultNow(),
  },
  (t) => [
    index('address_book_entry_org_id_idx').on(t.orgId),
    index('address_book_entry_owner_idx').on(t.ownerUserId),
    index('address_book_entry_device_id_idx').on(t.deviceId),
  ],
);

export const policy = pgTable(
  'policy',
  {
    id: bigint('id', { mode: 'number' }).primaryKey().generatedAlwaysAsIdentity(),
    publicId: text('public_id').notNull().unique(),
    orgId: orgId(),
    name: text('name').notNull(),
    document: jsonb('document').notNull(),
    createdAt: timestamp('created_at', tz).notNull().defaultNow(),
    updatedAt: timestamp('updated_at', tz).notNull().defaultNow(),
  },
  (t) => [
    index('policy_org_id_idx').on(t.orgId),
    uniqueIndex('policy_org_name_uq').on(t.orgId, t.name),
  ],
);

export const sessionLog = pgTable(
  'session_log',
  {
    id: bigint('id', { mode: 'number' }).primaryKey().generatedAlwaysAsIdentity(),
    publicId: text('public_id').notNull().unique(),
    orgId: orgId(),
    deviceId: bigint('device_id', { mode: 'number' })
      .notNull()
      .references(() => device.id, { onDelete: 'cascade' }),
    /** Host-chosen session identifier; unique per device (replay guard). */
    sessionKey: text('session_key').notNull(),
    controllerPub: text('controller_pub'),
    controllerScrinId: text('controller_scrin_id'),
    anonymous: boolean('anonymous').notNull(),
    permissions: text('permissions').array().notNull(),
    startedAt: timestamp('started_at', tz).notNull(),
    endedAt: timestamp('ended_at', tz),
    endReason: text('end_reason'),
    /** The exact signed log object, for independent re-verification. */
    payload: jsonb('payload').notNull(),
    /** Ed25519 signature by the host device key, 128 hex chars. */
    signature: text('signature').notNull(),
    createdAt: timestamp('created_at', tz).notNull().defaultNow(),
  },
  (t) => [
    index('session_log_org_id_idx').on(t.orgId),
    index('session_log_device_id_idx').on(t.deviceId),
    uniqueIndex('session_log_device_session_uq').on(t.deviceId, t.sessionKey),
  ],
);

/** Append-only, hash-chained per organisation. Never UPDATE or DELETE. */
export const auditEvent = pgTable(
  'audit_event',
  {
    id: bigint('id', { mode: 'number' }).primaryKey().generatedAlwaysAsIdentity(),
    publicId: text('public_id').notNull().unique(),
    orgId: orgId(),
    seq: bigint('seq', { mode: 'number' }).notNull(),
    actorType: text('actor_type').notNull(),
    actorId: text('actor_id').notNull(),
    action: text('action').notNull(),
    targetType: text('target_type'),
    targetId: text('target_id'),
    data: jsonb('data').notNull(),
    createdAt: timestamp('created_at', tz).notNull(),
    prevHash: text('prev_hash').notNull(),
    hash: text('hash').notNull(),
  },
  (t) => [
    index('audit_event_org_id_idx').on(t.orgId),
    uniqueIndex('audit_event_org_seq_uq').on(t.orgId, t.seq),
  ],
);

export const webhook = pgTable(
  'webhook',
  {
    id: bigint('id', { mode: 'number' }).primaryKey().generatedAlwaysAsIdentity(),
    publicId: text('public_id').notNull().unique(),
    orgId: orgId(),
    url: text('url').notNull(),
    /** HMAC-SHA256 key; returned once at creation, never again. */
    secret: text('secret').notNull(),
    events: text('events').array().notNull(),
    active: boolean('active').notNull().default(true),
    createdAt: timestamp('created_at', tz).notNull().defaultNow(),
  },
  (t) => [index('webhook_org_id_idx').on(t.orgId)],
);

export const webhookDelivery = pgTable(
  'webhook_delivery',
  {
    id: bigint('id', { mode: 'number' }).primaryKey().generatedAlwaysAsIdentity(),
    publicId: text('public_id').notNull().unique(),
    orgId: orgId(),
    webhookId: bigint('webhook_id', { mode: 'number' })
      .notNull()
      .references(() => webhook.id, { onDelete: 'cascade' }),
    event: text('event').notNull(),
    payload: jsonb('payload').notNull(),
    /** pending | succeeded | failed */
    status: text('status').notNull().default('pending'),
    attempts: integer('attempts').notNull().default(0),
    nextAttemptAt: timestamp('next_attempt_at', tz).notNull().defaultNow(),
    lastStatusCode: integer('last_status_code'),
    lastError: text('last_error'),
    deliveredAt: timestamp('delivered_at', tz),
    createdAt: timestamp('created_at', tz).notNull().defaultNow(),
  },
  (t) => [
    index('webhook_delivery_org_id_idx').on(t.orgId),
    index('webhook_delivery_webhook_id_idx').on(t.webhookId),
    index('webhook_delivery_due_idx').on(t.status, t.nextAttemptAt),
  ],
);

export const jitGrant = pgTable(
  'jit_grant',
  {
    id: bigint('id', { mode: 'number' }).primaryKey().generatedAlwaysAsIdentity(),
    publicId: text('public_id').notNull().unique(),
    orgId: orgId(),
    requesterUserId: text('requester_user_id')
      .notNull()
      .references(() => user.id, { onDelete: 'cascade' }),
    deviceId: bigint('device_id', { mode: 'number' })
      .notNull()
      .references(() => device.id, { onDelete: 'cascade' }),
    reason: text('reason').notNull(),
    windowStart: timestamp('window_start', tz).notNull(),
    windowEnd: timestamp('window_end', tz).notNull(),
    /** pending | approved | denied | expired */
    status: text('status').notNull().default('pending'),
    approverUserId: text('approver_user_id').references(() => user.id, { onDelete: 'set null' }),
    decisionNote: text('decision_note'),
    decidedAt: timestamp('decided_at', tz),
    createdAt: timestamp('created_at', tz).notNull().defaultNow(),
  },
  (t) => [
    index('jit_grant_org_id_idx').on(t.orgId),
    index('jit_grant_requester_idx').on(t.requesterUserId),
    index('jit_grant_device_id_idx').on(t.deviceId),
    index('jit_grant_approver_idx').on(t.approverUserId),
  ],
);

export const apiKey = pgTable(
  'api_key',
  {
    id: bigint('id', { mode: 'number' }).primaryKey().generatedAlwaysAsIdentity(),
    publicId: text('public_id').notNull().unique(),
    orgId: orgId(),
    createdBy: text('created_by')
      .notNull()
      .references(() => user.id, { onDelete: 'cascade' }),
    name: text('name').notNull(),
    /** First characters of the token, safe to display. */
    prefix: text('prefix').notNull(),
    /** SHA-256 of the full token, hex. The token itself is never stored. */
    hash: text('hash').notNull().unique(),
    scopes: text('scopes').array().notNull(),
    lastUsedAt: timestamp('last_used_at', tz),
    expiresAt: timestamp('expires_at', tz),
    revokedAt: timestamp('revoked_at', tz),
    createdAt: timestamp('created_at', tz).notNull().defaultNow(),
  },
  (t) => [index('api_key_org_id_idx').on(t.orgId), index('api_key_created_by_idx').on(t.createdBy)],
);
