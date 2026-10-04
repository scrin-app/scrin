/** Organisation roles, least to most privileged. */
const ROLES = ['viewer', 'technician', 'admin', 'owner'] as const;
type Role = (typeof ROLES)[number];

export const PERMISSIONS = [
  'devices:read',
  'devices:write',
  'devices:delete',
  'groups:read',
  'groups:write',
  'address_book:read',
  'address_book:write',
  'policies:read',
  'policies:write',
  'sessions:read',
  'audit:read',
  'webhooks:manage',
  'jit:read',
  'jit:request',
  'jit:approve',
  'api_keys:manage',
] as const;
export type Permission = (typeof PERMISSIONS)[number];

const VIEWER: readonly Permission[] = [
  'devices:read',
  'groups:read',
  'address_book:read',
  'policies:read',
  'sessions:read',
  'jit:read',
];
const TECHNICIAN: readonly Permission[] = [
  ...VIEWER,
  'devices:write',
  'address_book:write',
  'jit:request',
];
const ADMIN: readonly Permission[] = [
  ...TECHNICIAN,
  'devices:delete',
  'groups:write',
  'policies:write',
  'audit:read',
  'webhooks:manage',
  'jit:approve',
  'api_keys:manage',
];

const ROLE_PERMISSIONS: Readonly<Record<Role, ReadonlySet<Permission>>> = {
  viewer: new Set(VIEWER),
  technician: new Set(TECHNICIAN),
  admin: new Set(ADMIN),
  owner: new Set(PERMISSIONS),
};

function isRole(value: string): value is Role {
  return (ROLES as readonly string[]).includes(value);
}

export function isPermission(value: string): value is Permission {
  return (PERMISSIONS as readonly string[]).includes(value);
}

/** Unknown roles (e.g. a role string written by another tool) get no permissions. */
export function permissionsForRole(role: string): ReadonlySet<Permission> {
  return isRole(role) ? ROLE_PERMISSIONS[role] : new Set();
}
