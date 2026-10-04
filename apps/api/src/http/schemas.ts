import { z } from '@hono/zod-openapi';
import { PERMISSIONS } from '../authz.ts';

const ErrorSchema = z
  .object({
    error: z.object({
      code: z.string().openapi({ example: 'not_found' }),
      message: z.string().openapi({ example: 'Device not found' }),
      details: z.unknown().optional(),
    }),
  })
  .openapi('Error');

const err = (description: string) => ({
  description,
  content: { 'application/json': { schema: ErrorSchema } },
});

/** Error responses shared by every authenticated route. */
export const commonErrors = {
  400: err('Invalid request'),
  401: err('Not authenticated'),
  403: err('Authenticated but not allowed'),
  404: err('Not found (or belongs to another organisation)'),
} as const;

export const json = <T extends z.ZodType>(schema: T, description: string) => ({
  description,
  content: { 'application/json': { schema } },
});

export const body = <T extends z.ZodType>(schema: T) => ({
  required: true,
  content: { 'application/json': { schema } },
});

export const IdParam = z.object({
  id: z
    .string()
    .min(4)
    .max(64)
    .openapi({ param: { name: 'id', in: 'path' }, example: 'dev_3hT9xQ' }),
});

const Hex32 = z
  .string()
  .regex(/^[0-9a-f]{64}$/, 'expected 64 lowercase hex chars (Ed25519 public key)');
const Hex64 = z
  .string()
  .regex(/^[0-9a-f]{128}$/, 'expected 128 lowercase hex chars (Ed25519 signature)');
const ScrinIdSchema = z
  .string()
  .regex(/^\d{9}$/, 'expected a 9-digit scrin ID')
  .openapi({ example: '482913075' });
const Tag = z.string().trim().min(1).max(40);
const Tags = z.array(Tag).max(32);
const Iso = z.iso.datetime({ offset: true });

export const Limit = z.coerce.number().int().min(1).max(200).default(50);
const Offset = z.coerce.number().int().min(0).max(100_000).default(0);

const PermissionSchema = z.enum(PERMISSIONS);

/** Session permissions, matching proto/scrin/v1/session.proto `Permission`. */
const SESSION_PERMISSIONS = [
  'view',
  'input',
  'clipboard',
  'files_in',
  'files_out',
  'audio',
  'microphone',
  'restart',
  'terminal',
  'record',
  'privacy_mode',
  'block_input',
  'tunnel',
  'chat',
  'whiteboard',
] as const;
const SessionPermission = z.enum(SESSION_PERMISSIONS);

// ---- me ---------------------------------------------------------------------

export const MeSchema = z
  .object({
    kind: z.enum(['user', 'api_key']),
    userId: z.string(),
    orgId: z.string(),
    role: z.string().nullable(),
    apiKeyId: z.string().nullable(),
    permissions: z.array(PermissionSchema),
  })
  .openapi('Me');

// ---- devices ----------------------------------------------------------------

export const PLATFORMS = ['windows', 'android', 'web', 'macos', 'linux', 'ios'] as const;

export const DeviceSchema = z
  .object({
    id: z.string(),
    scrinId: ScrinIdSchema,
    devicePub: Hex32,
    name: z.string(),
    platform: z.enum(PLATFORMS),
    groupId: z.string().nullable(),
    tags: z.array(z.string()),
    lastSeenAt: z.string().nullable(),
    createdAt: z.string(),
  })
  .openapi('Device');

export const DeviceListSchema = z
  .object({ items: z.array(DeviceSchema), total: z.number().int() })
  .openapi('DeviceList');

export const DeviceChallengeRequest = z
  .object({ devicePub: Hex32, scrinId: ScrinIdSchema })
  .openapi('DeviceChallengeRequest');

export const DeviceChallengeSchema = z
  .object({
    challengeId: z.string(),
    nonce: z.string(),
    /** The exact UTF-8 text the device must sign with its Ed25519 key. */
    message: z.string(),
    expiresAt: z.string(),
  })
  .openapi('DeviceChallenge');

export const RegisterDeviceRequest = z
  .object({
    challengeId: z.string().min(4).max(64),
    signature: Hex64,
    name: z.string().trim().min(1).max(100),
    platform: z.enum(PLATFORMS),
    groupId: z.string().min(4).max(64).optional(),
    tags: Tags.optional(),
  })
  .openapi('RegisterDeviceRequest');

export const UpdateDeviceRequest = z
  .object({
    name: z.string().trim().min(1).max(100).optional(),
    groupId: z.string().min(4).max(64).nullable().optional(),
    tags: Tags.optional(),
  })
  .openapi('UpdateDeviceRequest');

export const DeviceListQuery = z.object({
  group: z.string().optional(),
  tag: z.string().optional(),
  q: z.string().max(100).optional(),
  limit: Limit,
  offset: Offset,
});

// ---- groups -----------------------------------------------------------------

export const GroupSchema = z
  .object({
    id: z.string(),
    name: z.string(),
    description: z.string().nullable(),
    deviceCount: z.number().int(),
    createdAt: z.string(),
  })
  .openapi('DeviceGroup');

export const GroupListSchema = z.object({ items: z.array(GroupSchema) }).openapi('DeviceGroupList');

export const GroupInput = z
  .object({
    name: z.string().trim().min(1).max(80),
    description: z.string().max(500).nullable().optional(),
  })
  .openapi('DeviceGroupInput');

// ---- address book -------------------------------------------------------------

export const AddressBookEntrySchema = z
  .object({
    id: z.string(),
    label: z.string(),
    scrinId: ScrinIdSchema,
    shared: z.boolean(),
    deviceId: z.string().nullable(),
    notes: z.string().nullable(),
    tags: z.array(z.string()),
    createdAt: z.string(),
  })
  .openapi('AddressBookEntry');

export const AddressBookListSchema = z
  .object({ items: z.array(AddressBookEntrySchema) })
  .openapi('AddressBookList');

export const AddressBookInput = z
  .object({
    label: z.string().trim().min(1).max(100),
    scrinId: ScrinIdSchema,
    shared: z.boolean().default(false),
    deviceId: z.string().min(4).max(64).optional(),
    notes: z.string().max(2000).optional(),
    tags: Tags.optional(),
  })
  .openapi('AddressBookInput');

export const AddressBookUpdate = z
  .object({
    label: z.string().trim().min(1).max(100).optional(),
    notes: z.string().max(2000).nullable().optional(),
    tags: Tags.optional(),
  })
  .openapi('AddressBookUpdate');

// ---- policies -----------------------------------------------------------------

export const PolicyDocument = z
  .object({
    allowedPermissions: z.array(SessionPermission).min(1),
    recordingRequired: z.boolean(),
    privacyModeAllowed: z.boolean(),
    unattendedAllowed: z.boolean(),
    sessionMaxMinutes: z
      .number()
      .int()
      .min(1)
      .max(24 * 60),
  })
  .strict()
  .refine((d) => !(d.recordingRequired && !d.allowedPermissions.includes('record')), {
    message: 'recordingRequired needs the "record" permission in allowedPermissions',
    path: ['allowedPermissions'],
  })
  .refine((d) => !(d.privacyModeAllowed && !d.allowedPermissions.includes('privacy_mode')), {
    message: 'privacyModeAllowed needs the "privacy_mode" permission in allowedPermissions',
    path: ['allowedPermissions'],
  })
  .openapi('PolicyDocument');

export const PolicySchema = z
  .object({
    id: z.string(),
    name: z.string(),
    document: PolicyDocument,
    createdAt: z.string(),
    updatedAt: z.string(),
  })
  .openapi('Policy');

export const PolicyListSchema = z.object({ items: z.array(PolicySchema) }).openapi('PolicyList');

export const PolicyInput = z
  .object({ name: z.string().trim().min(1).max(80), document: PolicyDocument })
  .openapi('PolicyInput');

// ---- session logs ---------------------------------------------------------------

export const SessionLogPayload = z
  .object({
    sessionKey: z.string().min(8).max(128),
    controllerPub: Hex32.nullable(),
    controllerScrinId: ScrinIdSchema.nullable(),
    anonymous: z.boolean(),
    permissions: z.array(SessionPermission).max(SESSION_PERMISSIONS.length),
    startedAt: Iso,
    endedAt: Iso.nullable(),
    endReason: z.string().max(64).nullable(),
  })
  .strict()
  .openapi('SessionLogPayload');

export const AppendSessionLogRequest = z
  .object({
    deviceId: z.string().min(4).max(64),
    payload: SessionLogPayload,
    signature: Hex64,
  })
  .openapi('AppendSessionLogRequest');

export const SessionLogSchema = z
  .object({
    id: z.string(),
    deviceId: z.string(),
    sessionKey: z.string(),
    controllerPub: z.string().nullable(),
    controllerScrinId: z.string().nullable(),
    anonymous: z.boolean(),
    permissions: z.array(z.string()),
    startedAt: z.string(),
    endedAt: z.string().nullable(),
    endReason: z.string().nullable(),
    signature: z.string(),
    createdAt: z.string(),
  })
  .openapi('SessionLog');

export const SessionLogListSchema = z
  .object({ items: z.array(SessionLogSchema) })
  .openapi('SessionLogList');

// ---- audit -----------------------------------------------------------------------

const AuditEventSchema = z
  .object({
    id: z.string(),
    seq: z.number().int(),
    actorType: z.string(),
    actorId: z.string(),
    action: z.string(),
    targetType: z.string().nullable(),
    targetId: z.string().nullable(),
    data: z.unknown(),
    createdAt: z.string(),
    prevHash: z.string(),
    hash: z.string(),
  })
  .openapi('AuditEvent');

export const AuditListSchema = z
  .object({ items: z.array(AuditEventSchema) })
  .openapi('AuditEventList');

export const AuditVerifySchema = z
  .object({
    valid: z.boolean(),
    count: z.number().int(),
    headHash: z.string(),
    brokenAt: z
      .object({
        seq: z.number().int(),
        id: z.string(),
        reason: z.enum(['prev_hash_mismatch', 'hash_mismatch', 'seq_gap']),
      })
      .nullable(),
  })
  .openapi('AuditVerification');

// ---- webhooks --------------------------------------------------------------------

const WebhookSchema = z
  .object({
    id: z.string(),
    url: z.string(),
    events: z.array(z.string()),
    active: z.boolean(),
    createdAt: z.string(),
  })
  .openapi('Webhook');

export const WebhookCreatedSchema = WebhookSchema.extend({
  /** HMAC-SHA256 signing secret. Shown once. */
  secret: z.string(),
}).openapi('WebhookCreated');

export const WebhookListSchema = z.object({ items: z.array(WebhookSchema) }).openapi('WebhookList');

export const WebhookInput = z
  .object({
    url: z.url().max(2048),
    events: z
      .array(z.string().regex(/^(\*|[a-z_]+\.[a-z_]+)$/, 'event like "device.registered" or "*"'))
      .min(1)
      .max(50),
  })
  .openapi('WebhookInput');

const WebhookDeliverySchema = z
  .object({
    id: z.string(),
    event: z.string(),
    status: z.enum(['pending', 'succeeded', 'failed']),
    attempts: z.number().int(),
    lastStatusCode: z.number().int().nullable(),
    lastError: z.string().nullable(),
    nextAttemptAt: z.string(),
    deliveredAt: z.string().nullable(),
    createdAt: z.string(),
  })
  .openapi('WebhookDelivery');

export const WebhookDeliveryListSchema = z
  .object({ items: z.array(WebhookDeliverySchema) })
  .openapi('WebhookDeliveryList');

// ---- JIT grants ------------------------------------------------------------------

export const JIT_STATUSES = ['pending', 'approved', 'denied', 'expired'] as const;

export const JitGrantSchema = z
  .object({
    id: z.string(),
    deviceId: z.string(),
    requesterUserId: z.string(),
    reason: z.string(),
    windowStart: z.string(),
    windowEnd: z.string(),
    status: z.enum(JIT_STATUSES),
    approverUserId: z.string().nullable(),
    decisionNote: z.string().nullable(),
    decidedAt: z.string().nullable(),
    createdAt: z.string(),
  })
  .openapi('JitGrant');

export const JitListSchema = z.object({ items: z.array(JitGrantSchema) }).openapi('JitGrantList');

export const JitRequestInput = z
  .object({
    deviceId: z.string().min(4).max(64),
    reason: z.string().trim().min(3).max(500),
    windowStart: Iso.optional(),
    durationMinutes: z
      .number()
      .int()
      .min(5)
      .max(24 * 60),
  })
  .openapi('JitRequestInput');

export const JitDecisionInput = z
  .object({ note: z.string().max(500).optional() })
  .openapi('JitDecisionInput');

export const JitListQuery = z.object({
  status: z.enum(JIT_STATUSES).optional(),
  device: z.string().optional(),
  limit: Limit,
});

// ---- API keys --------------------------------------------------------------------

const ApiKeySchema = z
  .object({
    id: z.string(),
    name: z.string(),
    prefix: z.string(),
    scopes: z.array(PermissionSchema),
    lastUsedAt: z.string().nullable(),
    expiresAt: z.string().nullable(),
    revokedAt: z.string().nullable(),
    createdAt: z.string(),
  })
  .openapi('ApiKey');

export const ApiKeyCreatedSchema = ApiKeySchema.extend({
  /** The full key. Shown once; only its SHA-256 is stored. */
  token: z.string(),
}).openapi('ApiKeyCreated');

export const ApiKeyListSchema = z.object({ items: z.array(ApiKeySchema) }).openapi('ApiKeyList');

export const ApiKeyInput = z
  .object({
    name: z.string().trim().min(1).max(80),
    scopes: z.array(PermissionSchema).min(1),
    expiresInDays: z.number().int().min(1).max(3650).optional(),
  })
  .openapi('ApiKeyInput');

// ---- system ----------------------------------------------------------------------

export const HealthSchema = z.object({ status: z.literal('ok') }).openapi('Health');
export const ReadySchema = z
  .object({ status: z.enum(['ready', 'unavailable']), db: z.enum(['ok', 'down']) })
  .openapi('Ready');
