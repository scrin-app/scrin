import { createHash, createHmac } from 'node:crypto';

export function sha256Hex(data: string | Uint8Array): string {
  return createHash('sha256').update(data).digest('hex');
}

export function hmacSha256Hex(key: string, message: string): string {
  return createHmac('sha256', key).update(message).digest('hex');
}

const HEX = /^(?:[0-9a-f]{2})*$/;

function hexToBytes(hex: string): Uint8Array<ArrayBuffer> {
  if (!HEX.test(hex)) throw new TypeError('invalid hex');
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return out;
}

export function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

/** Ed25519 verification through WebCrypto. Malformed keys or signatures verify as false. */
export async function verifyEd25519(
  publicKeyHex: string,
  message: Uint8Array<ArrayBuffer>,
  signatureHex: string,
): Promise<boolean> {
  try {
    const key = await crypto.subtle.importKey(
      'raw',
      hexToBytes(publicKeyHex),
      { name: 'Ed25519' },
      false,
      ['verify'],
    );
    return await crypto.subtle.verify({ name: 'Ed25519' }, key, hexToBytes(signatureHex), message);
  } catch {
    return false;
  }
}

/**
 * Deterministic JSON: object keys sorted by UTF-16 code unit, no whitespace,
 * `undefined` members dropped. Used for the audit hash chain and for every
 * message a device signs, so both sides serialise identically.
 */
export function canonicalJson(value: unknown): string {
  if (value === null) return 'null';
  switch (typeof value) {
    case 'string':
    case 'boolean':
      return JSON.stringify(value);
    case 'number':
      if (!Number.isFinite(value)) throw new TypeError('canonicalJson: non-finite number');
      return JSON.stringify(value);
    case 'object': {
      if (Array.isArray(value)) {
        return `[${value.map((v: unknown) => canonicalJson(v)).join(',')}]`;
      }
      if (value instanceof Date) return JSON.stringify(value.toISOString());
      const entries = Object.entries(value)
        .filter(([, v]) => v !== undefined)
        .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
      return `{${entries.map(([k, v]) => `${JSON.stringify(k)}:${canonicalJson(v)}`).join(',')}}`;
    }
    default:
      throw new TypeError(`canonicalJson: unsupported type ${typeof value}`);
  }
}

export const utf8 = (s: string): Uint8Array<ArrayBuffer> => new TextEncoder().encode(s);
