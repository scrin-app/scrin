// Relational-query metadata. drizzle-orm 0.45 (latest stable) ships only the
// v1 `relations()` API; `defineRelations` (RQB v2) exists in the 1.0 RC line.
// Move to `defineRelations` together with the drizzle 1.0 upgrade.
import { relations } from 'drizzle-orm';
import { member, organization, user } from './auth.ts';
import {
  addressBookEntry,
  apiKey,
  auditEvent,
  device,
  deviceGroup,
  jitGrant,
  policy,
  sessionLog,
  webhook,
  webhookDelivery,
} from './domain.ts';

export const organizationRelations = relations(organization, ({ many }) => ({
  members: many(member),
  devices: many(device),
  groups: many(deviceGroup),
  policies: many(policy),
  auditEvents: many(auditEvent),
  webhooks: many(webhook),
}));

export const memberRelations = relations(member, ({ one }) => ({
  organization: one(organization, {
    fields: [member.organizationId],
    references: [organization.id],
  }),
  user: one(user, { fields: [member.userId], references: [user.id] }),
}));

export const deviceGroupRelations = relations(deviceGroup, ({ one, many }) => ({
  organization: one(organization, { fields: [deviceGroup.orgId], references: [organization.id] }),
  devices: many(device),
}));

export const deviceRelations = relations(device, ({ one, many }) => ({
  organization: one(organization, { fields: [device.orgId], references: [organization.id] }),
  group: one(deviceGroup, { fields: [device.groupId], references: [deviceGroup.id] }),
  sessions: many(sessionLog),
  jitGrants: many(jitGrant),
}));

export const addressBookEntryRelations = relations(addressBookEntry, ({ one }) => ({
  device: one(device, { fields: [addressBookEntry.deviceId], references: [device.id] }),
  owner: one(user, { fields: [addressBookEntry.ownerUserId], references: [user.id] }),
}));

export const policyRelations = relations(policy, ({ one }) => ({
  organization: one(organization, { fields: [policy.orgId], references: [organization.id] }),
}));

export const sessionLogRelations = relations(sessionLog, ({ one }) => ({
  device: one(device, { fields: [sessionLog.deviceId], references: [device.id] }),
}));

export const auditEventRelations = relations(auditEvent, ({ one }) => ({
  organization: one(organization, { fields: [auditEvent.orgId], references: [organization.id] }),
}));

export const webhookRelations = relations(webhook, ({ one, many }) => ({
  organization: one(organization, { fields: [webhook.orgId], references: [organization.id] }),
  deliveries: many(webhookDelivery),
}));

export const webhookDeliveryRelations = relations(webhookDelivery, ({ one }) => ({
  webhook: one(webhook, { fields: [webhookDelivery.webhookId], references: [webhook.id] }),
}));

export const jitGrantRelations = relations(jitGrant, ({ one }) => ({
  device: one(device, { fields: [jitGrant.deviceId], references: [device.id] }),
  requester: one(user, { fields: [jitGrant.requesterUserId], references: [user.id] }),
}));

export const apiKeyRelations = relations(apiKey, ({ one }) => ({
  organization: one(organization, { fields: [apiKey.orgId], references: [organization.id] }),
  creator: one(user, { fields: [apiKey.createdBy], references: [user.id] }),
}));
