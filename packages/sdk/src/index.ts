import createClient, { type Client } from 'openapi-fetch';
import type { components, paths } from './schema.d.ts';

export type { components, paths };
export type Schemas = components['schemas'];
export type Device = Schemas['Device'];
export type DeviceGroup = Schemas['DeviceGroup'];
export type AuditEvent = Schemas['AuditEvent'];
export type AuditVerification = Schemas['AuditVerification'];
export type JitGrant = Schemas['JitGrant'];
export type SessionLog = Schemas['SessionLog'];
export type Policy = Schemas['Policy'];
export type Me = Schemas['Me'];
export type ApiErrorBody = Schemas['Error'];

export interface ScrinClientOptions {
  /** API origin, e.g. `https://api.scrin.dragoscatalin.ro`. */
  baseUrl: string;
  /** `sk_scrin_…` automation key. Omit to rely on cookies (browser console). */
  apiKey?: string | undefined;
  /** Organisation for cookie sessions (`X-Scrin-Org`); API keys are already bound to one. */
  orgId?: string | undefined;
  fetch?: typeof globalThis.fetch;
  headers?: Record<string, string>;
}

export type ScrinClient = Client<paths>;

/** Typed client generated from the API's OpenAPI document. */
export function createScrinClient(opts: ScrinClientOptions): ScrinClient {
  const headers: Record<string, string> = { ...opts.headers };
  if (opts.apiKey !== undefined) headers.authorization = `Bearer ${opts.apiKey}`;
  if (opts.orgId !== undefined) headers['x-scrin-org'] = opts.orgId;
  const client = createClient<paths>({
    baseUrl: opts.baseUrl.replace(/\/+$/, ''),
    headers,
    ...(opts.fetch === undefined ? {} : { fetch: opts.fetch }),
  });
  return client;
}

/** A non-2xx answer, carrying the API's `{ error: { code, message, details? } }`. */
export class ScrinApiError extends Error {
  readonly status: number;
  readonly code: string;
  readonly details: unknown;

  constructor(status: number, body: unknown) {
    const err = isErrorBody(body) ? body.error : { code: 'http_error', message: `HTTP ${status}` };
    super(err.message);
    this.name = 'ScrinApiError';
    this.status = status;
    this.code = err.code;
    this.details = 'details' in err ? err.details : undefined;
  }
}

function isErrorBody(v: unknown): v is ApiErrorBody {
  if (typeof v !== 'object' || v === null || !('error' in v)) return false;
  const e: unknown = v.error;
  return typeof e === 'object' && e !== null && 'code' in e && 'message' in e;
}

/**
 * Unwraps an openapi-fetch result: returns `data` or throws `ScrinApiError`.
 * `await unwrap(client.GET('/v1/devices'))`
 */
export async function unwrap<T>(
  call: Promise<{ data?: T; error?: unknown; response: Response }>,
): Promise<T> {
  const { data, error, response } = await call;
  if (!response.ok || error !== undefined || data === undefined) {
    throw new ScrinApiError(response.status, error);
  }
  return data;
}
