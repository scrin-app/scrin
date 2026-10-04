import { and, eq, gt, isNull, or } from 'drizzle-orm';
import type { MiddlewareHandler } from 'hono';
import { isPermission, permissionsForRole, type Permission } from '../authz.ts';
import type { Actor, AppDeps, AppEnv } from '../context.ts';
import { sha256Hex } from '../crypto.ts';
import { scoped, type Scope } from '../db/scoped.ts';
import { apiKey, member } from '../db/schema/index.ts';
import { ApiError, forbidden, notFound, unauthorized } from '../errors.ts';

export const API_KEY_PREFIX = 'sk_scrin_';
const ORG_HEADER = 'x-scrin-org';

function bearer(header: string | undefined): string | undefined {
  if (header === undefined) return undefined;
  const m = /^Bearer\s+(\S+)$/i.exec(header);
  return m?.[1];
}

async function apiKeyActor(deps: AppDeps, token: string, now: Date): Promise<Actor> {
  const [row] = await deps.db
    .select()
    .from(apiKey)
    .where(
      and(
        eq(apiKey.hash, sha256Hex(token)),
        isNull(apiKey.revokedAt),
        or(isNull(apiKey.expiresAt), gt(apiKey.expiresAt, now)),
      ),
    )
    .limit(1);
  if (row === undefined) throw unauthorized('Invalid or expired API key');
  await deps.db.update(apiKey).set({ lastUsedAt: now }).where(eq(apiKey.id, row.id));
  const scopes = new Set<Permission>();
  for (const s of row.scopes) if (isPermission(s)) scopes.add(s);
  // A key never exceeds what its creator can currently do in the organisation.
  const [m] = await deps.db
    .select({ role: member.role })
    .from(member)
    .where(and(eq(member.organizationId, row.orgId), eq(member.userId, row.createdBy)))
    .limit(1);
  const creatorPerms = permissionsForRole(m?.role ?? '');
  const permissions = new Set([...scopes].filter((p) => creatorPerms.has(p)));
  return {
    kind: 'api_key',
    keyId: row.publicId,
    userId: row.createdBy,
    orgId: row.orgId,
    permissions,
  };
}

/**
 * Resolves the caller: `Authorization: Bearer sk_scrin_…` (API key, bound to one
 * organisation) or a better-auth session cookie plus the organisation from the
 * `X-Scrin-Org` header or the session's active organisation.
 */
export function requireActor(deps: AppDeps): MiddlewareHandler<AppEnv> {
  return async (c, next) => {
    const now = deps.now?.() ?? new Date();
    const token = bearer(c.req.header('authorization'));
    if (token?.startsWith(API_KEY_PREFIX) === true) {
      c.set('actor', await apiKeyActor(deps, token, now));
      await next();
      return;
    }
    const session = await deps.auth.getSession(c.req.raw.headers);
    if (session === null) throw unauthorized();
    const orgId = c.req.header(ORG_HEADER) ?? session.session.activeOrganizationId ?? undefined;
    if (orgId === undefined || orgId === '') {
      throw new ApiError(
        400,
        'no_active_organization',
        `Select an organisation (set the ${ORG_HEADER} header or an active organisation)`,
      );
    }
    const [m] = await deps.db
      .select({ role: member.role })
      .from(member)
      .where(and(eq(member.organizationId, orgId), eq(member.userId, session.user.id)))
      .limit(1);
    // Not a member is indistinguishable from "no such organisation".
    if (m === undefined) throw notFound('Organization');
    c.set('actor', {
      kind: 'user',
      userId: session.user.id,
      orgId,
      role: m.role,
      permissions: permissionsForRole(m.role),
    });
    await next();
  };
}

/** Per-route authorisation. Returns the tenant scope for the caller's organisation. */
export function authorize(actor: Actor, deps: AppDeps, permission: Permission): Scope {
  if (!actor.permissions.has(permission)) {
    throw forbidden(`Missing permission ${permission}`, 'insufficient_permission');
  }
  return scoped(deps.db, actor.orgId);
}

export function actorRef(actor: Actor): { actorType: string; actorId: string } {
  return actor.kind === 'user'
    ? { actorType: 'user', actorId: actor.userId }
    : { actorType: 'api_key', actorId: actor.keyId };
}
