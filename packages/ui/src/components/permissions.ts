/**
 * Session permissions as the UI sees them. Names and order mirror
 * `crates/scrin-session/src/permissions.rs::Permission::{ALL, name}`; the
 * anonymous caps mirror `policy.rs::Policy::default()` (ADR-0009). The host
 * engine stays the authority: it sends the `allowed` ceiling with every
 * request, and these tables only describe and pre-check it.
 */

export const PERMISSIONS = [
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

export type PermissionName = (typeof PERMISSIONS)[number];

export type SessionKindName = 'anonymous' | 'verified' | 'trusted' | 'org';

/** Never granted to an anonymous controller. */
export const ANONYMOUS_FORBIDDEN: ReadonlySet<PermissionName> = new Set<PermissionName>([
  'files_in',
  'files_out',
  'privacy_mode',
  'tunnel',
  'terminal',
  'block_input',
]);

/** `ANONYMOUS_MAX_DURATION_MS` in minutes. */
export const ANONYMOUS_MAX_MINUTES = 60;

export function isPermissionName(v: unknown): v is PermissionName {
  return typeof v === 'string' && (PERMISSIONS as readonly string[]).includes(v);
}

export function isSessionKind(v: unknown): v is SessionKindName {
  return v === 'anonymous' || v === 'verified' || v === 'trusted' || v === 'org';
}

/**
 * The default ceiling for a kind (used when the engine did not send one).
 * Privacy mode is only for unattended kinds, as in `Policy::allowed`.
 */
export function defaultAllowed(kind: SessionKindName): PermissionName[] {
  const unattended = kind === 'trusted' || kind === 'org';
  return PERMISSIONS.filter((p) => {
    if (kind === 'anonymous' && ANONYMOUS_FORBIDDEN.has(p)) return false;
    if (!unattended && p === 'privacy_mode') return false;
    return true;
  });
}

/** i18n key of each permission's label (keys are camelCase). */
export const PERMISSION_LABEL_KEY = {
  view: 'permissions.view',
  input: 'permissions.input',
  clipboard: 'permissions.clipboard',
  files_in: 'permissions.filesIn',
  files_out: 'permissions.filesOut',
  audio: 'permissions.audio',
  microphone: 'permissions.microphone',
  restart: 'permissions.restart',
  terminal: 'permissions.terminal',
  record: 'permissions.record',
  privacy_mode: 'permissions.privacyMode',
  block_input: 'permissions.blockInput',
  tunnel: 'permissions.tunnel',
  chat: 'permissions.chat',
  whiteboard: 'permissions.whiteboard',
} as const satisfies Record<PermissionName, `permissions.${string}`>;
